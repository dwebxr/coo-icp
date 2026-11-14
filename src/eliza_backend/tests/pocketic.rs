//! Integration tests against a local PocketIC server.
//!
//! Ignored by default; run them with `make test-canister`, which builds the wasm modules they load (the canister,
//! its two test-only variants, the mock LLM canister, and the version deployed on mainnet) and points POCKET_IC_BIN
//! at the server bundled with dfx.
//!
//! The on-chain model is replaced by `mock_llm`, installed at the LLM canister's real id, so the canister's own
//! `v1_chat` call runs unchanged and the tests can hold, release and inspect it.
//!
//! An update call that `canister_inspect_message` refuses never reaches the method: the tests tell "refused at
//! ingress by inspect_message" (`refused_by_inspect`, which checks the rejection is inspect_message's own) from "the
//! method answered with an error" (`Ok(Err(..))`).

use candid::{decode_one, encode_args, encode_one, CandidType, Nat, Principal};
use eliza_backend::{Character, ChatError, Config, Conversation, LlmProvider, MessageStatus, Metrics, Sent};
use pocket_ic::common::rest::{RawEffectivePrincipal, RawMessageId};
use pocket_ic::{ErrorCode, PocketIc};
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::time::Duration;

macro_rules! wasm_path {
    ($dir:literal) => {
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/", $dir, "wasm32-unknown-unknown/release/eliza_backend.wasm")
    };
}
/// The canister as it is deployed.
const NEW_WASM: &str = wasm_path!("");
/// With `test-hooks`: `test_fill`, and a trap after the model replied to "trap after reply".
const HOOKS_WASM: &str = wasm_path!("test-hooks/");
/// With `small-limits` (2 MiB of storage, 8 entries, 2 chats at once, 5 model calls a minute) and `test-hooks`.
const SMALL_WASM: &str = wasm_path!("small-limits/");
const MOCK_LLM_WASM: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/wasm32-unknown-unknown/release/mock_llm.wasm");
/// The LLM canister the backend calls (mainnet id).
const LLM_CANISTER: &str = "w36hm-eqaaa-aaaal-qr76a-cai";

fn admin() -> Principal {
    Principal::self_authenticating(b"admin")
}
fn alice() -> Principal {
    Principal::self_authenticating(b"alice")
}
fn bob() -> Principal {
    Principal::self_authenticating(b"bob")
}
fn carol() -> Principal {
    Principal::self_authenticating(b"carol")
}
fn user(n: u32) -> Principal {
    Principal::self_authenticating(n.to_be_bytes())
}

fn read(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {path}: {e} (run `make test-canister`)"))
}

type SendResult = Result<Sent, ChatError>;

/// The canister alone: the LLM canister does not exist, so calls to it are rejected.
fn setup_without_llm(wasm: Vec<u8>) -> (PocketIc, Principal) {
    let pic = PocketIc::new();
    let id = pic.create_canister_with_settings(Some(admin()), None);
    pic.add_cycles(id, 20_000_000_000_000);
    pic.install_canister(id, wasm, encode_args(()).unwrap(), Some(admin()));
    (pic, id)
}

/// The canister plus the mock LLM canister at the real LLM canister id (PocketIC adds a subnet for it).
fn setup(wasm: Vec<u8>) -> (PocketIc, Principal) {
    let (pic, id) = setup_without_llm(wasm);
    install_llm(&pic);
    (pic, id)
}

fn install_llm(pic: &PocketIc) {
    let llm = pic
        .create_canister_with_id(Some(admin()), None, llm())
        .expect("cannot create the mock LLM canister at the LLM canister id");
    pic.add_cycles(llm, 20_000_000_000_000);
    pic.install_canister(llm, read(MOCK_LLM_WASM), encode_args(()).unwrap(), Some(admin()));
}

fn llm() -> Principal {
    Principal::from_text(LLM_CANISTER).unwrap()
}

/// How the mock answers `v1_chat` (mirrors `mock_llm::Mode`).
#[derive(CandidType)]
enum Mode {
    Reply(String),
    Hold(String),
    HoldThenReject,
    Reject,
    Empty,
}

/// A request the mock received (mirrors `mock_llm::Seen`): (role, content) per message.
#[derive(CandidType, Deserialize, Debug, Clone, PartialEq, Eq)]
struct Seen {
    model: String,
    messages: Vec<(String, String)>,
}

fn llm_mode(pic: &PocketIc, mode: Mode) {
    let () = update(pic, llm(), admin(), "mock_set_mode", encode_one(mode).unwrap());
}

/// Lets the held model calls finish.
fn llm_release(pic: &PocketIc) {
    let () = update(pic, llm(), admin(), "mock_release", encode_args(()).unwrap());
}

fn llm_seen(pic: &PocketIc) -> Vec<Seen> {
    query(pic, llm(), admin(), "mock_seen", encode_args(()).unwrap())
}

/// Ticks until the mock has received `count` requests, i.e. the canister's call is waiting for its answer.
fn llm_pending(pic: &PocketIc, count: usize) -> Seen {
    for _ in 0..50 {
        let seen = llm_seen(pic);
        assert!(seen.len() <= count, "more model calls than expected: {seen:?}");
        if seen.len() == count {
            return seen[count - 1].clone();
        }
        pic.tick();
    }
    panic!("the model was not called {count} time(s)");
}

fn upgrade(pic: &PocketIc, id: Principal, wasm: Vec<u8>) {
    pic.stop_canister(id, Some(admin())).unwrap();
    pic.upgrade_canister(id, wasm, encode_args(()).unwrap(), Some(admin())).unwrap();
    pic.start_canister(id, Some(admin())).unwrap();
    assert_logged_sizes_match(pic, id);
}

const PRE_UPGRADE_LINE: &str = "pre_upgrade: ";
const POST_UPGRADE_LINE: &str = "State restored from stable memory successfully (";

fn logs(pic: &PocketIc, id: Principal) -> Vec<String> {
    pic.fetch_canister_logs(id, admin())
        .unwrap()
        .into_iter()
        .map(|r| String::from_utf8_lossy(&r.content).into_owned())
        .collect()
}

/// The last line of the canister log that contains `marker`.
fn last_line(pic: &PocketIc, id: Principal, marker: &str) -> String {
    let logs = logs(pic, id);
    logs.iter().rev().find(|l| l.contains(marker)).cloned().unwrap_or_else(|| panic!("no {marker:?} in {logs:?}"))
}

/// The number right after the first `label` in `line`.
fn number_after(line: &str, label: &str) -> u64 {
    let at = line.find(label).unwrap_or_else(|| panic!("no {label:?} in {line:?}")) + label.len();
    line[at..].chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap()
}

/// The README's post-upgrade check: the last saved and restored state sizes in the canister log are equal.
fn assert_logged_sizes_match(pic: &PocketIc, id: Principal) {
    assert_eq!(
        number_after(&last_line(pic, id, PRE_UPGRADE_LINE), PRE_UPGRADE_LINE),
        number_after(&last_line(pic, id, POST_UPGRADE_LINE), POST_UPGRADE_LINE)
    );
}

/// What a hook logged it used: the Wasm memory size at its end (memory only grows, so the hook's peak is at most
/// this) and the instructions it had executed.
#[derive(Debug, Clone, Copy)]
struct HookCost {
    memory: u64,
    instructions: u64,
}

fn hook_cost(line: &str) -> HookCost {
    HookCost { memory: number_after(line, "wasm memory "), instructions: number_after(line, "instructions ") }
}

/// What the last upgrade's pre_upgrade (of this version) and post_upgrade logged they used.
fn upgrade_cost(pic: &PocketIc, id: Principal) -> (HookCost, HookCost) {
    (hook_cost(&last_line(pic, id, PRE_UPGRADE_LINE)), hook_cost(&last_line(pic, id, POST_UPGRADE_LINE)))
}

/// The Wasm memory limit the README tells operators to set. It is enforced for update calls and post_upgrade, but
/// not for pre_upgrade (interface spec, canister settings: "The WASM heap memory limit is ignored for query methods,
/// response callback handlers, global timers, heartbeats, and canister pre_upgrade"), so pre_upgrade's use is read
/// from its log line instead.
const RECOMMENDED_WASM_MEMORY_LIMIT: u64 = 512 * 1024 * 1024;
/// "Per canister install or upgrade | 300 billion" instructions, and "Per update call, heartbeat, or timer | 40
/// billion" (https://docs.internetcomputer.org/building-apps/canister-management/resource-limits, 2026-11). Whether
/// the two hooks share the upgrade's budget is not documented, so their sum is held to the ceiling.
const UPGRADE_INSTRUCTION_LIMIT: u64 = 300_000_000_000;
const UPDATE_INSTRUCTION_LIMIT: u64 = 40_000_000_000;

/// An upgrade stays well within the limits: each hook under a quarter of the recommended Wasm memory limit, and
/// both together under a tenth of the instruction limit.
fn assert_upgrade_well_within_limits(shape: &str, pre: Option<HookCost>, post: HookCost) {
    eprintln!("{shape}: pre_upgrade {pre:?}, post_upgrade {post:?}");
    for (hook, cost) in [("pre_upgrade", pre), ("post_upgrade", Some(post))] {
        if let Some(HookCost { memory, .. }) = cost {
            assert!(
                memory < RECOMMENDED_WASM_MEMORY_LIMIT / 4,
                "{shape}: {hook} grew the Wasm memory to {memory} bytes"
            );
        }
    }
    let instructions = pre.map_or(0, |c| c.instructions) + post.instructions;
    assert!(instructions < UPGRADE_INSTRUCTION_LIMIT / 10, "{shape}: the upgrade took {instructions} instructions");
}

/// The instructions the last `send_message` took up to the model call (logged by the test-hooks builds), which
/// includes making room.
fn send_instructions(pic: &PocketIc, id: Principal) -> u64 {
    number_after(&last_line(pic, id, "test: send_message took "), "test: send_message took ")
}

fn set_recommended_memory_limit(pic: &PocketIc, id: Principal) {
    let settings = ic_management_canister_types::CanisterSettings {
        wasm_memory_limit: Some(Nat::from(RECOMMENDED_WASM_MEMORY_LIMIT)),
        ..Default::default()
    };
    pic.update_canister_settings(id, Some(admin()), settings).unwrap();
}

fn update<T: CandidType + DeserializeOwned>(
    pic: &PocketIc,
    id: Principal,
    from: Principal,
    method: &str,
    arg: Vec<u8>,
) -> T {
    let bytes = pic.update_call(id, from, method, arg).unwrap_or_else(|e| panic!("{method} rejected: {e:?}"));
    decode_one(&bytes).unwrap()
}

fn query<T: CandidType + DeserializeOwned>(
    pic: &PocketIc,
    id: Principal,
    from: Principal,
    method: &str,
    arg: Vec<u8>,
) -> T {
    let bytes = pic.query_call(id, from, method, arg).unwrap_or_else(|e| panic!("{method} rejected: {e:?}"));
    decode_one(&bytes).unwrap()
}

fn send(pic: &PocketIc, id: Principal, from: Principal, request_id: &str, base: u64, text: &str) -> SendResult {
    update(pic, id, from, "send_message", encode_args((request_id, base, text)).unwrap())
}

fn submit(pic: &PocketIc, id: Principal, from: Principal, request_id: &str, base: u64, text: &str) -> RawMessageId {
    pic.submit_call(id, from, "send_message", encode_args((request_id, base, text)).unwrap()).unwrap()
}

fn outcome(pic: &PocketIc, call: RawMessageId) -> SendResult {
    decode_one(&pic.await_call(call).unwrap()).unwrap()
}

fn status(pic: &PocketIc, id: Principal, from: Principal, request_id: &str) -> MessageStatus {
    query(pic, id, from, "get_message_status", encode_one(request_id).unwrap())
}

fn conversation(pic: &PocketIc, id: Principal, from: Principal) -> Conversation {
    query(pic, id, from, "get_conversation", encode_args(()).unwrap())
}

fn chat_messages(pic: &PocketIc, id: Principal, from: Principal) -> Vec<(String, String)> {
    conversation(pic, id, from)
        .messages
        .into_iter()
        .filter(|m| m.role != "system")
        .map(|m| (m.role, m.content))
        .collect()
}

fn count(pic: &PocketIc, id: Principal) -> u64 {
    query(pic, id, admin(), "get_conversation_count", encode_args(()).unwrap())
}

fn set_provider(pic: &PocketIc, id: Principal, provider: LlmProvider) {
    let r: Result<(), String> = update(pic, id, admin(), "set_llm_provider", encode_one(provider).unwrap());
    r.unwrap();
}

fn sent(reply: &str, revision: u64) -> SendResult {
    Ok(Sent { reply: reply.to_string(), revision })
}

/// What `inspect_message` traps with when it refuses a call.
const INSPECT_REFUSAL: &str = "not accepted: sign in, call queries as queries, and keep arguments small";

/// True if `canister_inspect_message` refused the call (the rejection is its trap); false if the call was accepted
/// and reached the method. Any other rejection at submission fails the test.
fn refused_by_inspect(pic: &PocketIc, id: Principal, from: Principal, method: &str, arg: Vec<u8>) -> bool {
    match pic.submit_call(id, from, method, arg) {
        Err(reject) => {
            assert!(
                reject.error_code == ErrorCode::CanisterCalledTrap && reject.reject_message.contains(INSPECT_REFUSAL),
                "{method}: rejected at submission, but not by inspect_message: {reject}"
            );
            true
        }
        Ok(message) => {
            let _ = pic.await_call(message);
            false
        }
    }
}

/// A clear that traps (no room for the caller), with the trap message.
fn clear_refused(pic: &PocketIc, id: Principal, from: Principal) -> String {
    match pic.update_call(id, from, "clear_conversation", encode_args(()).unwrap()) {
        Err(reject) => reject.reject_message,
        Ok(bytes) => panic!("the clear was accepted: revision {:?}", decode_one::<u64>(&bytes)),
    }
}

/// Calls the canister from the mock LLM canister, i.e. as another canister: such calls do not pass through
/// `canister_inspect_message`, so the method's own checks answer. Returns the reply bytes, or the reject text.
fn via_canister(pic: &PocketIc, id: Principal, method: &str, arg: Vec<u8>) -> Result<Vec<u8>, String> {
    update(pic, llm(), admin(), "mock_proxy", encode_args((id, method, arg)).unwrap())
}

fn metrics(pic: &PocketIc, id: Principal) -> Metrics {
    query(pic, id, admin(), "get_metrics", encode_args(()).unwrap())
}

fn character(pic: &PocketIc, id: Principal) -> Character {
    query(pic, id, admin(), "get_character", encode_args(()).unwrap())
}

fn clear(pic: &PocketIc, id: Principal, from: Principal) -> u64 {
    update(pic, id, from, "clear_conversation", encode_args(()).unwrap())
}

/// The wasm of the version running on mainnet (v0.6.0), built by `make test-canister` from its commit.
fn old_wasm() -> Vec<u8> {
    let path = std::env::var("ELIZA_OLD_WASM")
        .expect("ELIZA_OLD_WASM must point at a build of the deployed version (make test-canister sets it)");
    read(&path)
}

/// Like `send`, but a rejected call is an `Err` instead of a panic.
fn send_raw(
    pic: &PocketIc,
    id: Principal,
    from: Principal,
    request_id: &str,
    base: u64,
    text: &str,
) -> Result<SendResult, String> {
    pic.update_call(id, from, "send_message", encode_args((request_id, base, text)).unwrap())
        .map(|bytes| decode_one(&bytes).unwrap())
        .map_err(|e| format!("{e:?}"))
}

// ========== Upgrades ==========

#[test]
#[ignore]
fn upgrade_from_the_deployed_version_keeps_every_conversation() {
    let (pic, id) = setup(old_wasm());
    set_provider(&pic, id, LlmProvider::Fallback);
    let first = send(&pic, id, alice(), "a-1", 0, "hello").unwrap();
    let second = send(&pic, id, alice(), "a-2", 1, "tell me about icp").unwrap();
    // Longer than 0.7.0 accepts, and than the 8 KiB its ingress filter first allowed; 0.6.0 had no limit.
    let long_text = "long ".repeat(1_800);
    assert_eq!(long_text.len(), 9_000);
    let long = send(&pic, id, alice(), "a-long", 2, &long_text).unwrap();
    let legacy: Result<String, String> = update(&pic, id, bob(), "chat", encode_one("who are you").unwrap());
    legacy.unwrap();
    send(&pic, id, carol(), "c-1", 0, "hello").unwrap();
    assert_eq!(clear(&pic, id, carol()), 2);
    let stored: Result<(), String> =
        update(&pic, id, admin(), "store_encrypted_api_key", encode_one(b"sk-test".to_vec()).unwrap());
    stored.unwrap();
    let (alice_before, bob_before) = (chat_messages(&pic, id, alice()), chat_messages(&pic, id, bob()));
    assert_eq!((alice_before.len(), bob_before.len()), (6, 2));

    upgrade(&pic, id, read(NEW_WASM));

    // Every turn is still there; the system message that 0.6.0 kept in each conversation is not.
    assert_eq!(chat_messages(&pic, id, alice()), alice_before);
    assert_eq!(chat_messages(&pic, id, bob()), bob_before);
    assert!(conversation(&pic, id, alice()).messages.iter().all(|m| m.role != "system"));
    assert!(conversation(&pic, id, carol()).messages.is_empty());
    assert_eq!(count(&pic, id), 2);
    assert_eq!(conversation(&pic, id, alice()).revision, 3);
    assert_eq!(conversation(&pic, id, bob()).revision, 1);
    assert_eq!(conversation(&pic, id, carol()).revision, 2);
    let m = metrics(&pic, id);
    assert_eq!((m.entries, m.revision_floor, m.version.as_str()), (3, 0, env!("CARGO_PKG_VERSION")));
    // Requests saved before the upgrade replay instead of running again — the too-long one included — and
    // stale ones stay refused.
    assert_eq!(
        status(&pic, id, alice(), "a-2"),
        MessageStatus::Done(Sent { reply: second.reply.clone(), revision: 3 })
    );
    assert_eq!(send(&pic, id, alice(), "a-1", 0, "hello"), Ok(Sent { reply: first.reply.clone(), revision: 3 }));
    assert_eq!(send(&pic, id, alice(), "a-long", 2, &long_text), Ok(Sent { reply: long.reply.clone(), revision: 3 }));
    assert_eq!(send(&pic, id, carol(), "c-1", 0, "hello"), Err(ChatError::Conflict));
    assert_eq!(llm_seen(&pic).len(), 0, "nothing was run again");
    // The provider is kept (0.6.0 forced OnChain on every upgrade); the untouched default character is replaced.
    let config: Config = query(&pic, id, admin(), "get_config", encode_args(()).unwrap());
    assert_eq!(config.llm_provider, LlmProvider::Fallback);
    let coo = character(&pic, id);
    assert!(!coo.system_prompt.contains("elizaOS"), "{}", coo.system_prompt);
    assert!(coo.bio.iter().any(|line| line.contains("inspired by elizaOS")));

    // The upgraded canister keeps working, through another upgrade as well.
    let third = send(&pic, id, alice(), "a-3", 3, "hi").unwrap();
    assert_eq!(third.revision, 4);
    upgrade(&pic, id, read(NEW_WASM));
    assert_eq!(status(&pic, id, alice(), "a-3"), MessageStatus::Done(third.clone()));
    assert_eq!(conversation(&pic, id, alice()).revision, 4);
    assert_eq!(chat_messages(&pic, id, alice()).len(), 8);
    assert_eq!(chat_messages(&pic, id, bob()), bob_before);
    let config: Config = query(&pic, id, admin(), "get_config", encode_args(()).unwrap());
    assert_eq!(config.llm_provider, LlmProvider::Fallback);
}

/// 0.6.0 had no limits: its state may be over this version's, as a whole and per conversation, and bigger than this
/// version ever writes. The upgrade reads it, cuts it to the limits, and every caller can go on.
#[test]
#[ignore]
fn an_oversized_state_from_the_deployed_version_is_cut_to_the_limits() {
    // A message as long as fits one ingress message (2 MiB in all), with a reply as long that fits one response.
    const BIG: usize = 1_900_000;
    // Each conversation holds the message and 0.6.0's fallback reply, which echoes it: 18 make a saved state of
    // more than the 64 MiB this version writes at most.
    const CALLERS: u32 = 18;
    let (pic, id) = setup(old_wasm());
    set_recommended_memory_limit(&pic, id);
    set_provider(&pic, id, LlmProvider::Fallback);
    let hello = send(&pic, id, alice(), "a-1", 0, "hello").unwrap();
    let alice_before = chat_messages(&pic, id, alice());
    let big = "x".repeat(BIG);
    for n in 0..CALLERS {
        assert_eq!(send(&pic, id, user(n), "big", 0, &big).map(|s| (s.revision, s.reply.len() > BIG)), Ok((1, true)));
    }

    upgrade(&pic, id, read(NEW_WASM));

    let saved = number_after(&last_line(&pic, id, PRE_UPGRADE_LINE), PRE_UPGRADE_LINE);
    assert!(saved > 64 * 1024 * 1024 && saved > CALLERS as u64 * 2 * BIG as u64, "{saved}");
    let cut = last_line(&pic, id, "Brought the state of schema version 2 within this version's limits: ");
    assert_eq!(number_after(&cut, "limits: "), 2 * CALLERS as u64, "{cut}");
    assert_eq!(number_after(&cut, "turns and "), 0, "{cut}");
    // Reading needs the saved state and its decoding at once, about twice the payload: at the most 0.6.0 could have
    // saved (100 000 000 bytes) still under half the recommended limit (which post_upgrade is held to; it was set).
    let post = hook_cost(&last_line(&pic, id, POST_UPGRADE_LINE));
    eprintln!("0.6.0 state of {saved} bytes with {CALLERS} oversized conversations: post_upgrade {post:?}");
    assert!(post.memory < 2 * saved + 16 * 1024 * 1024, "{post:?}");
    assert!(post.instructions < UPGRADE_INSTRUCTION_LIMIT / 10, "{post:?}");

    // The small conversation is intact and still replays.
    assert_eq!(chat_messages(&pic, id, alice()), alice_before);
    assert_eq!(send(&pic, id, alice(), "a-1", 0, "hello"), Ok(hello.clone()));
    // The oversized ones were cut whole, their revisions kept: the cut request is gone, and can never run again.
    let m = metrics(&pic, id);
    assert_eq!((m.entries, m.conversations), (CALLERS as u64 + 1, 1));
    assert!(m.stored_bytes <= 32 * 1024 * 1024, "{m:?}");
    for n in 0..CALLERS {
        assert_eq!(conversation(&pic, id, user(n)), Conversation { messages: vec![], revision: 1 });
        assert_eq!(status(&pic, id, user(n), "big"), MessageStatus::Unknown);
        assert_eq!(send(&pic, id, user(n), "again", 0, "x"), Err(ChatError::Conflict));
    }
    // Every caller can chat on.
    for n in 0..CALLERS {
        assert_eq!(send(&pic, id, user(n), "next", 1, "hello again").map(|s| s.revision), Ok(2));
    }
    assert_eq!(send(&pic, id, alice(), "a-2", 1, "and me").map(|s| s.revision), Ok(2));
}

#[test]
#[ignore]
fn a_customised_character_survives_the_upgrade() {
    let (pic, id) = setup(old_wasm());
    let zoe = Character { name: "Zoe".into(), system_prompt: "You are Zoe.".into(), bio: vec![], style: vec![] };
    let set: Result<(), String> = update(&pic, id, admin(), "update_character", encode_one(&zoe).unwrap());
    set.unwrap();
    upgrade(&pic, id, read(NEW_WASM));
    assert_eq!(character(&pic, id), zoe);
}

#[test]
#[ignore]
fn an_upgrade_without_saved_state_is_refused() {
    // An empty module has no pre_upgrade, so nothing reaches stable memory.
    let (pic, id) = setup_without_llm(b"\0asm\x01\0\0\0".to_vec());
    pic.stop_canister(id, Some(admin())).unwrap();
    let result = pic.upgrade_canister(id, read(NEW_WASM), encode_args(()).unwrap(), Some(admin()));
    assert!(result.is_err(), "post_upgrade must trap instead of starting empty");
}

const BUDGET: u64 = 32 * 1024 * 1024;

/// A canister filled to its storage budget still upgrades, under the Wasm memory limit the README tells
/// operators to set. (If this fails after raising a limit in limits.rs, the README's number must change too.)
/// The three shapes below cover few large conversations, many entries, and many turns.
#[test]
#[ignore]
fn a_canister_at_its_storage_budget_upgrades_within_the_recommended_memory_limit() {
    let (pic, id) = setup(read(HOOKS_WASM));
    set_recommended_memory_limit(&pic, id);

    // 1 663 conversations of one 2 × 10 000-byte exchange: the 32 MiB budget, to within one exchange.
    let filled: u64 = update(&pic, id, admin(), "test_fill", encode_args((1_663u32, 1u32, 10_000u32)).unwrap());
    assert!(filled <= BUDGET && filled + 20_000 > BUDGET, "{filled}");
    let before = metrics(&pic, id);

    upgrade(&pic, id, read(HOOKS_WASM));

    let after = metrics(&pic, id);
    assert_eq!((after.entries, after.stored_bytes), (1_663, filled));
    assert_eq!(after.conversations, before.conversations);
    let (pre, post) = upgrade_cost(&pic, id);
    assert_upgrade_well_within_limits("1 663 conversations of one 20 000-byte exchange", Some(pre), post);
    // Saving and restoring each need the state twice (the conversations and their encoding): about 70 MiB here.
    assert!(after.wasm_memory_bytes < RECOMMENDED_WASM_MEMORY_LIMIT / 4, "a quarter of the limit is not reached");
    // And it still chats: making room for the next exchange evicts down to the target.
    set_provider(&pic, id, LlmProvider::Fallback);
    assert!(send(&pic, id, alice(), "r-1", 0, "hello").is_ok());
    let evicted = metrics(&pic, id);
    assert!(evicted.stored_bytes <= BUDGET * 90 / 100 && evicted.revision_floor == 2, "{evicted:?}");
    eprintln!("  the send that made room took {} instructions", send_instructions(&pic, id));
}

/// The entry cap: 100 000 entries of one tiny exchange each. The upgrade handles them all, and the first newcomer
/// after it makes room by sorting all 100 000 inside its update call — the most `make_room` ever has to do.
#[test]
#[ignore]
fn many_small_entries_at_the_entry_cap_upgrade_and_make_way() {
    const MAX_ENTRIES: u64 = 100_000;
    let (pic, id) = setup(read(HOOKS_WASM));
    set_recommended_memory_limit(&pic, id);
    let filled: u64 = update(&pic, id, admin(), "test_fill", encode_args((MAX_ENTRIES as u32, 1u32, 1u32)).unwrap());
    assert_eq!(metrics(&pic, id).entries, MAX_ENTRIES);

    upgrade(&pic, id, read(HOOKS_WASM));

    let after = metrics(&pic, id);
    assert_eq!((after.entries, after.stored_bytes), (MAX_ENTRIES, filled));
    let (pre, post) = upgrade_cost(&pic, id);
    assert_upgrade_well_within_limits("100 000 entries of one tiny exchange", Some(pre), post);

    set_provider(&pic, id, LlmProvider::Fallback);
    assert_eq!(send(&pic, id, alice(), "r-1", 0, "hello").map(|s| s.revision), Ok(1));
    let made_way = metrics(&pic, id);
    // Down to 90 % of the cap, plus the newcomer.
    assert_eq!((made_way.entries, made_way.revision_floor), (MAX_ENTRIES * 9 / 10 + 1, 2));
    let took = send_instructions(&pic, id);
    eprintln!("  the send that sorted {MAX_ENTRIES} entries took {took} instructions");
    assert!(took < UPDATE_INSTRUCTION_LIMIT / 20, "{took}");
}

/// Conversations at their turn limit, with one-byte texts: the most turns the budget can hold.
#[test]
#[ignore]
fn conversations_at_their_turn_limit_upgrade_within_the_limits() {
    let (pic, id) = setup(read(HOOKS_WASM));
    set_recommended_memory_limit(&pic, id);
    // 25 exchanges (50 turns) of 1-byte texts count 1 911 bytes per conversation: 17 558 of them fill the budget.
    let filled: u64 = update(&pic, id, admin(), "test_fill", encode_args((17_558u32, 25u32, 1u32)).unwrap());
    assert!(filled <= BUDGET && filled + 20_224 > BUDGET, "{filled}");

    upgrade(&pic, id, read(HOOKS_WASM));

    assert_eq!(metrics(&pic, id).stored_bytes, filled);
    let (pre, post) = upgrade_cost(&pic, id);
    assert_upgrade_well_within_limits("17 558 conversations of 50 one-byte turns", Some(pre), post);
    set_provider(&pic, id, LlmProvider::Fallback);
    assert!(send(&pic, id, alice(), "r-1", 0, "hello").is_ok());
    assert!(metrics(&pic, id).stored_bytes <= BUDGET * 90 / 100);
    eprintln!("  the send that made room took {} instructions", send_instructions(&pic, id));
}

// ========== The chat protocol ==========

#[test]
#[ignore]
fn single_flight_busy_in_progress_and_clear_wins() {
    let (pic, id) = setup(read(NEW_WASM));
    llm_mode(&pic, Mode::Hold("Hi there!".into()));

    let first = submit(&pic, id, alice(), "r-1", 0, "hello");
    let asked = llm_pending(&pic, 1);
    assert_eq!(asked.messages.last().unwrap(), &("user".to_string(), "hello".to_string()));

    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Pending);
    assert_eq!(send(&pic, id, alice(), "r-2", 0, "other"), Err(ChatError::Busy));
    assert_eq!(send(&pic, id, alice(), "r-1", 0, "hello"), Err(ChatError::InProgress));
    // Another user is not blocked by alice's request.
    assert_eq!(status(&pic, id, bob(), "r-1"), MessageStatus::Unknown);

    assert_eq!(clear(&pic, id, alice()), 1);
    llm_release(&pic);
    assert_eq!(outcome(&pic, first), Err(ChatError::Cleared));
    assert_eq!(conversation(&pic, id, alice()), Conversation { messages: vec![], revision: 1 });
    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Unknown);
    // The request from before the clear can never run.
    assert_eq!(send(&pic, id, alice(), "r-1", 0, "hello"), Err(ChatError::Conflict));

    // The lock is released: a new request goes through, and repeating it replays the saved reply.
    let second = submit(&pic, id, alice(), "r-2", 1, "other");
    llm_pending(&pic, 2);
    llm_release(&pic);
    assert_eq!(outcome(&pic, second), sent("Hi there!", 2));
    assert_eq!(status(&pic, id, alice(), "r-2"), MessageStatus::Done(Sent { reply: "Hi there!".into(), revision: 2 }));
    assert_eq!(send(&pic, id, alice(), "r-2", 1, "other"), sent("Hi there!", 2));
    assert_eq!(chat_messages(&pic, id, alice()).len(), 2);
    assert_eq!(llm_seen(&pic).len(), 2, "replay must not call the model");
}

#[test]
#[ignore]
fn a_request_runs_at_most_once_whatever_happens_after_it() {
    let (pic, id) = setup(read(NEW_WASM));
    set_provider(&pic, id, LlmProvider::Fallback);

    // Saved, but suppose the response was lost. Another tab then sends nine more messages.
    let lost = send(&pic, id, alice(), "lost", 0, "hello").unwrap();
    for i in 1..10u64 {
        send(&pic, id, alice(), &format!("tab-{i}"), i, &format!("message {i}")).unwrap();
    }
    assert_eq!(conversation(&pic, id, alice()).revision, 10);
    // Retrying still replays while the exchange is in the conversation...
    assert_eq!(send(&pic, id, alice(), "lost", 0, "hello"), Ok(Sent { reply: lost.reply, revision: 10 }));
    // ...and is never run again once it has been trimmed away.
    for i in 10..30u64 {
        send(&pic, id, alice(), &format!("tab-{i}"), i, &format!("message {i}")).unwrap();
    }
    let before = chat_messages(&pic, id, alice());
    assert_eq!(before.len(), 50);
    assert!(!before.iter().any(|(_, text)| text == "hello"), "trimmed away");
    assert_eq!(send(&pic, id, alice(), "lost", 0, "hello"), Err(ChatError::Conflict));
    assert_eq!(chat_messages(&pic, id, alice()), before);

    // Two clients at the same revision: the second one gets a Conflict and nothing runs.
    assert!(send(&pic, id, alice(), "x", 30, "from tab one").is_ok());
    assert_eq!(send(&pic, id, alice(), "y", 30, "from tab two"), Err(ChatError::Conflict));
    assert_eq!(conversation(&pic, id, alice()).revision, 31);

    // Reusing an id for a different message is refused rather than replayed.
    assert!(matches!(send(&pic, id, alice(), "x", 30, "something else"), Err(ChatError::Invalid(_))));
}

#[test]
#[ignore]
fn clearing_bumps_the_revision_and_keeps_the_count_meaningful() {
    let (pic, id) = setup(read(NEW_WASM));
    set_provider(&pic, id, LlmProvider::Fallback);
    send(&pic, id, alice(), "a1", 0, "hello").unwrap();
    send(&pic, id, bob(), "b1", 0, "hello").unwrap();
    assert_eq!(count(&pic, id), 2);

    assert_eq!(clear(&pic, id, alice()), 2);
    assert_eq!(conversation(&pic, id, alice()), Conversation { messages: vec![], revision: 2 });
    assert_eq!(count(&pic, id), 1);

    // Clearing a conversation that never existed still moves the revision on, so a first message that was
    // delayed in transit cannot arrive after the clear and be saved.
    assert_eq!(clear(&pic, id, carol()), 1);
    assert_eq!(conversation(&pic, id, carol()).revision, 1);
    assert_eq!(send(&pic, id, carol(), "late", 0, "sent before the clear"), Err(ChatError::Conflict));
    assert_eq!(count(&pic, id), 1);
    assert_eq!(metrics(&pic, id).entries, 3);
    assert_eq!(send(&pic, id, alice(), "a2", 2, "again").map(|s| s.revision), Ok(3));
}

#[test]
#[ignore]
fn bad_requests_are_refused_and_save_nothing() {
    let (pic, id) = setup(read(NEW_WASM));
    set_provider(&pic, id, LlmProvider::Fallback);
    for bad in ["", "has space", "ü", &"x".repeat(65)] {
        assert!(matches!(send(&pic, id, alice(), bad, 0, "hi"), Err(ChatError::Invalid(_))), "{bad:?}");
    }
    for (what, text) in [("empty", String::new()), ("blank", " \n".to_string()), ("too long", "x".repeat(4_001))] {
        assert!(matches!(send(&pic, id, alice(), "r-1", 0, &text), Err(ChatError::Invalid(_))), "{what}");
    }
    assert_eq!(conversation(&pic, id, alice()), Conversation { messages: vec![], revision: 0 });
    assert_eq!(metrics(&pic, id).entries, 0);
    // The limits themselves are accepted.
    assert!(send(&pic, id, alice(), &"x".repeat(64), 0, &"x".repeat(4_000)).is_ok());
    assert_eq!(chat_messages(&pic, id, alice()).len(), 2);
}

// ========== The model ==========

#[test]
#[ignore]
fn a_slow_model_call_that_fails_saves_nothing_and_cannot_run_later() {
    let (pic, id) = setup(read(NEW_WASM));
    llm_mode(&pic, Mode::HoldThenReject);

    let slow = submit(&pic, id, alice(), "x", 0, "first try");
    llm_pending(&pic, 1);
    pic.advance_time(Duration::from_secs(60));
    llm_release(&pic);
    assert!(matches!(outcome(&pic, slow), Err(ChatError::Failed(_))));
    assert!(chat_messages(&pic, id, alice()).is_empty());
    assert_eq!(status(&pic, id, alice(), "x"), MessageStatus::Unknown);

    // Not left locked; once something else is saved, the failed request can no longer run.
    llm_mode(&pic, Mode::Reply("Hi there!".into()));
    assert_eq!(send(&pic, id, alice(), "y", 0, "second try"), sent("Hi there!", 1));
    assert_eq!(send(&pic, id, alice(), "x", 0, "first try"), Err(ChatError::Conflict));
    assert_eq!(chat_messages(&pic, id, alice()).len(), 2);
}

/// The model call is a bounded-wait call: it ends after five minutes whatever the LLM canister does.
#[test]
#[ignore]
fn a_model_that_never_answers_times_out_and_nothing_is_saved() {
    let (pic, id) = setup(read(NEW_WASM));
    llm_mode(&pic, Mode::Hold("too late".into()));

    let stuck = submit(&pic, id, alice(), "x", 0, "first try");
    llm_pending(&pic, 1);
    assert_eq!(send(&pic, id, alice(), "y", 0, "second try"), Err(ChatError::Busy));
    pic.advance_time(Duration::from_secs(5 * 60 + 1));
    // The mock is still holding the call; the canister's own deadline ends it.
    assert!(matches!(outcome(&pic, stuck), Err(ChatError::Failed(_))));
    assert_eq!(status(&pic, id, alice(), "x"), MessageStatus::Unknown);
    assert!(chat_messages(&pic, id, alice()).is_empty());
    assert_eq!(metrics(&pic, id).chats_in_flight, 0);

    // The caller is not left locked, and the late answer changes nothing.
    llm_mode(&pic, Mode::Reply("Hi there!".into()));
    llm_release(&pic);
    assert_eq!(send(&pic, id, alice(), "y", 0, "second try"), sent("Hi there!", 1));
    for _ in 0..10 {
        pic.tick();
    }
    assert_eq!(
        chat_messages(&pic, id, alice()),
        [("user".to_string(), "second try".to_string()), ("assistant".to_string(), "Hi there!".to_string())]
    );
    assert_eq!(send(&pic, id, alice(), "x", 0, "first try"), Err(ChatError::Conflict));
}

/// With an unbounded call, a model that hangs would keep the canister in "stopping" for ever, and a canister that
/// cannot stop cannot be upgraded safely.
#[test]
#[ignore]
fn a_model_that_never_answers_does_not_keep_the_canister_from_stopping() {
    let (pic, id) = setup(read(NEW_WASM));
    llm_mode(&pic, Mode::Hold("too late".into()));
    let stuck = submit(&pic, id, alice(), "x", 0, "hello");
    llm_pending(&pic, 1);

    #[derive(CandidType)]
    struct CanisterIdRecord {
        canister_id: Principal,
    }
    let stop = pic
        .submit_call_with_effective_principal(
            Principal::management_canister(),
            RawEffectivePrincipal::CanisterId(id.as_slice().to_vec()),
            admin(),
            "stop_canister",
            encode_one(CanisterIdRecord { canister_id: id }).unwrap(),
        )
        .unwrap();
    for _ in 0..5 {
        pic.tick();
    }
    assert!(send_raw(&pic, id, bob(), "b", 0, "hi").is_err(), "a stopping canister takes no new calls");
    pic.advance_time(Duration::from_secs(5 * 60 + 1));
    pic.await_call(stop).expect("the canister stops once the model call has timed out");
    assert!(matches!(outcome(&pic, stuck), Err(ChatError::Failed(_))));

    pic.upgrade_canister(id, read(NEW_WASM), encode_args(()).unwrap(), Some(admin())).unwrap();
    pic.start_canister(id, Some(admin())).unwrap();
    assert_logged_sizes_match(&pic, id);
    assert_eq!(conversation(&pic, id, alice()), Conversation { messages: vec![], revision: 0 });
}

#[test]
#[ignore]
fn model_failure_is_an_error_not_a_trap() {
    // No LLM canister at all (as on a local replica): the call is rejected.
    let (pic, id) = setup_without_llm(read(NEW_WASM));
    let failed = send(&pic, id, alice(), "r-1", 0, "hello");
    // The user gets a plain sentence; the reject code and message are in the canister log, not in the reply.
    assert_eq!(failed, Err(ChatError::Failed("The model did not answer.".into())));
    assert!(chat_messages(&pic, id, alice()).is_empty());
    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Unknown);
    // Not left locked, and nothing changed the revision.
    assert!(matches!(send(&pic, id, alice(), "r-2", 0, "again"), Err(ChatError::Failed(_))));

    // The LLM canister rejects the call, or answers without content or with blanks: errors too, nothing saved.
    install_llm(&pic);
    for mode in [Mode::Reject, Mode::Empty, Mode::Reply(" \n".into())] {
        llm_mode(&pic, mode);
        assert!(matches!(send(&pic, id, alice(), "r-3", 0, "hello"), Err(ChatError::Failed(_))));
        assert!(chat_messages(&pic, id, alice()).is_empty());
        assert_eq!(conversation(&pic, id, alice()).revision, 0);
    }
    // The same request succeeds once the model answers; a reply is saved trimmed.
    llm_mode(&pic, Mode::Reply("  Hi there!\n".into()));
    assert_eq!(send(&pic, id, alice(), "r-3", 0, "hello"), sent("Hi there!", 1));
}

#[test]
#[ignore]
fn a_trap_in_the_reply_callback_rolls_back_and_releases_the_lock() {
    let (pic, id) = setup(read(HOOKS_WASM));
    let call = submit(&pic, id, alice(), "r-1", 0, "trap after reply");
    assert!(pic.await_call(call).is_err(), "the call is rejected");
    assert_eq!(llm_seen(&pic).len(), 1, "the model was asked before the trap");
    assert!(chat_messages(&pic, id, alice()).is_empty());
    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Unknown);
    // The in-flight entry was released by the cleanup callback: not Busy.
    assert_eq!(metrics(&pic, id).chats_in_flight, 0);
    assert_eq!(send(&pic, id, alice(), "r-2", 0, "next"), sent("Hi there!", 1));
}

#[test]
#[ignore]
fn the_model_gets_the_character_and_the_recent_conversation_within_its_limits() {
    let (pic, id) = setup(read(NEW_WASM));
    assert_eq!(send(&pic, id, alice(), "r-0", 0, "hello"), sent("Hi there!", 1));
    let first = &llm_seen(&pic)[0];
    assert_eq!(first.model, "llama3.1:8b");
    assert_eq!(first.messages.len(), 2);
    let (role, system) = &first.messages[0];
    assert_eq!(role, "system");
    // The whole character is in the system message, name, bio and style included.
    assert!(system.starts_with("You are Coo") && system.contains("About you:\n- Your name is Coo\n"));
    assert!(system.contains("Your style:") && system.contains("- Friendly"));
    assert_eq!(first.messages[1], ("user".to_string(), "hello".to_string()));

    // Twelve exchanges later the system message is still first, and the request has at most 10 messages.
    for i in 1..=12u64 {
        send(&pic, id, alice(), &format!("r-{i}"), i, &format!("question {i}")).unwrap();
    }
    let last = llm_seen(&pic).pop().unwrap();
    assert_eq!(last.messages.len(), 10);
    assert_eq!(last.messages[0], ("system".to_string(), system.clone()));
    assert_eq!(last.messages[1], ("user".to_string(), "question 8".to_string()));
    assert_eq!(last.messages[9], ("user".to_string(), "question 12".to_string()));

    // Long turns are cut so the request stays under the LLM canister's 10 KiB, the new message excepted.
    llm_mode(&pic, Mode::Reply("r".repeat(15_000)));
    send(&pic, id, bob(), "b-1", 0, &"q".repeat(4_000)).unwrap();
    send(&pic, id, bob(), "b-2", 1, &"q".repeat(4_000)).unwrap();
    let last = llm_seen(&pic).pop().unwrap();
    let total: usize = last.messages.iter().map(|(_, text)| text.len()).sum();
    assert!(total <= 9_000, "{total}");
    assert_eq!(last.messages.last().unwrap().1.len(), 4_000);
    assert!(last.messages.len() > 2, "some context is kept");
    // The saved reply is cut to its own limit only.
    assert_eq!(conversation(&pic, id, bob()).messages[1].content.len(), 15_000);

    // A new character reaches conversations that already exist.
    let zoe = Character { name: "Zoe".into(), system_prompt: "You are Zoe.".into(), bio: vec![], style: vec![] };
    let set: Result<(), String> = update(&pic, id, admin(), "update_character", encode_one(&zoe).unwrap());
    set.unwrap();
    llm_mode(&pic, Mode::Reply("Hi there!".into()));
    send(&pic, id, alice(), "r-13", 13, "who are you?").unwrap();
    let zoe_system = "You are Zoe.\n\nAbout you:\n- Your name is Zoe".to_string();
    assert_eq!(llm_seen(&pic).pop().unwrap().messages[0], ("system".to_string(), zoe_system));
}

// ========== Who may call what ==========

#[test]
#[ignore]
fn ingress_is_filtered_before_the_canister_pays_for_it() {
    let (pic, id) = setup(read(NEW_WASM));
    let anonymous = Principal::anonymous();
    let send_args = |text: &str| encode_args(("r-1", 0u64, text)).unwrap();

    // Anonymous update calls.
    assert!(refused_by_inspect(&pic, id, anonymous, "send_message", send_args("hi")));
    assert!(refused_by_inspect(&pic, id, anonymous, "clear_conversation", encode_args(()).unwrap()));
    // Arguments far beyond what the method accepts.
    assert!(refused_by_inspect(&pic, id, alice(), "send_message", send_args(&"x".repeat(17_000))));
    assert!(refused_by_inspect(&pic, id, alice(), "clear_conversation", vec![0u8; 1_000]));
    // Administration by someone who is not a controller.
    let zoe = Character { name: "Zoe".into(), system_prompt: "You are Zoe.".into(), bio: vec![], style: vec![] };
    assert!(refused_by_inspect(&pic, id, alice(), "update_character", encode_one(&zoe).unwrap()));
    assert!(refused_by_inspect(&pic, id, alice(), "set_llm_provider", encode_one(LlmProvider::Fallback).unwrap()));
    // Queries sent as update calls.
    for method in ["health", "get_metrics", "get_character"] {
        assert!(refused_by_inspect(&pic, id, alice(), method, encode_args(()).unwrap()), "{method}");
    }
    // Methods that do not exist (any more) are refused by inspect_message too: it runs before the replica looks
    // the method up. (That they do not exist is checked from another canister, in the next test.)
    for method in ["chat", "set_admin", "store_encrypted_api_key"] {
        assert!(refused_by_inspect(&pic, id, alice(), method, encode_args(()).unwrap()), "{method}");
    }
    // A message longer than the method accepts reaches it, as long as the argument is within 16 KiB: a request saved
    // by 0.6.0 may be that long and replays only this way. The method itself refuses it as new.
    let long = submit(&pic, id, alice(), "r-0", 0, &"x".repeat(10_000));
    assert!(matches!(outcome(&pic, long), Err(ChatError::Invalid(_))));
    assert_eq!(metrics(&pic, id).entries, 0);
    assert_eq!(llm_seen(&pic).len(), 0);

    // What the frontend does is accepted: chatting, clearing, and reading its conversation as an update call
    // (answered by the subnet, not by a single replica).
    assert!(!refused_by_inspect(&pic, id, alice(), "send_message", send_args("hi")));
    let certified: Conversation = update(&pic, id, alice(), "get_conversation", encode_args(()).unwrap());
    assert_eq!((certified.messages.len(), certified.revision), (2, 1));
    let certified: MessageStatus = update(&pic, id, alice(), "get_message_status", encode_one("r-1").unwrap());
    assert!(matches!(certified, MessageStatus::Done(_)));
    assert_eq!(clear(&pic, id, alice()), 2);
}

/// Calls from other canisters never pass through the ingress filter, so each method must refuse on its own.
#[test]
#[ignore]
fn every_method_checks_its_caller_and_input_itself() {
    let (pic, id) = setup(read(NEW_WASM));
    let zoe = Character { name: "Zoe".into(), system_prompt: "You are Zoe.".into(), bio: vec![], style: vec![] };

    // The mock LLM canister is not a controller of the backend.
    let reply = via_canister(&pic, id, "update_character", encode_one(&zoe).unwrap()).unwrap();
    assert!(decode_one::<Result<(), String>>(&reply).unwrap().is_err());
    let reply = via_canister(&pic, id, "set_llm_provider", encode_one(LlmProvider::Fallback).unwrap()).unwrap();
    assert!(decode_one::<Result<(), String>>(&reply).unwrap().is_err());
    assert_eq!(character(&pic, id).name, "Coo");
    let config: Config = query(&pic, id, admin(), "get_config", encode_args(()).unwrap());
    assert_eq!(config.llm_provider, LlmProvider::OnChain);

    // A message far too long for the method, which inspect_message would have stopped at ingress.
    let reply = via_canister(&pic, id, "send_message", encode_args(("r-1", 0u64, "x".repeat(9_000))).unwrap()).unwrap();
    assert!(matches!(decode_one::<SendResult>(&reply).unwrap(), Err(ChatError::Invalid(_))));
    assert_eq!(metrics(&pic, id).entries, 0);
    // Removed methods are gone: called from a canister (no inspect_message on the way), the replica finds no method.
    for method in
        ["chat", "get_conversation_history", "set_admin", "store_encrypted_api_key", "transform_openai_response"]
    {
        let missing = via_canister(&pic, id, method, encode_args(()).unwrap()).unwrap_err();
        assert!(missing.contains(&format!("has no update method '{method}'")), "{method}: {missing}");
    }
    // An existing method answers the same kind of call, so the check above tells a missing method from any failure.
    assert!(via_canister(&pic, id, "get_conversation", encode_args(()).unwrap()).is_ok());

    // A controller is held to the character's limits too.
    for (what, bad) in [
        ("empty name", Character { name: String::new(), ..zoe.clone() }),
        ("long prompt", Character { system_prompt: "p".repeat(2_001), ..zoe.clone() }),
        ("too many bio lines", Character { bio: vec!["b".to_string(); 9], ..zoe.clone() }),
    ] {
        let refused: Result<(), String> = update(&pic, id, admin(), "update_character", encode_one(&bad).unwrap());
        assert!(refused.is_err(), "{what}");
    }
    let accepted: Result<(), String> = update(&pic, id, admin(), "update_character", encode_one(&zoe).unwrap());
    accepted.unwrap();
    assert_eq!(character(&pic, id), zoe);
}

/// Test-only methods and hooks exist in the test builds only.
#[test]
#[ignore]
fn the_deployed_build_has_no_test_hooks() {
    let contains = |wasm: &[u8], needle: &[u8]| wasm.windows(needle.len()).any(|w| w == needle);
    let (deployed, hooks) = (read(NEW_WASM), read(HOOKS_WASM));
    for needle in [&b"test_fill"[..], &b"trap in the reply callback"[..], &b"test: send_message took "[..]] {
        assert!(contains(&hooks, needle), "the check itself must be able to find the hook");
        assert!(!contains(&deployed, needle));
    }
    let (pic, id) = setup(read(NEW_WASM));
    assert!(pic.update_call(id, admin(), "test_fill", encode_args((1u32, 1u32, 1u32)).unwrap()).is_err());
    let _ = (pic, id);
}

// ========== Limits over all callers (the `small-limits` build) ==========

#[test]
#[ignore]
fn chats_at_once_and_model_calls_per_minute_are_limited() {
    let (pic, id) = setup(read(SMALL_WASM));
    llm_mode(&pic, Mode::Hold("Hi there!".into()));

    // Two chats at once is the limit of this build.
    let a = submit(&pic, id, alice(), "a-1", 0, "hello");
    let b = submit(&pic, id, bob(), "b-1", 0, "hello");
    llm_pending(&pic, 2);
    assert_eq!(metrics(&pic, id).chats_in_flight, 2);
    assert_eq!(send(&pic, id, carol(), "c-1", 0, "hello"), Err(ChatError::Busy));
    // What does not depend on the limit keeps its own answer: a repeated request, and a revision that is not the
    // conversation's (carol is at 0; 7 was never handed out, so this Conflict holds only for now).
    assert_eq!(send(&pic, id, alice(), "a-1", 0, "hello"), Err(ChatError::InProgress));
    assert_eq!(send(&pic, id, carol(), "c-1", 7, "hello"), Err(ChatError::Conflict));
    assert_eq!(metrics(&pic, id).entries, 2, "a refused request leaves no entry");
    llm_release(&pic);
    assert_eq!(outcome(&pic, a), sent("Hi there!", 1));
    assert_eq!(outcome(&pic, b), sent("Hi there!", 1));

    // Five model calls a minute: two were made, three more go through, the sixth is refused — for everyone.
    llm_mode(&pic, Mode::Reply("Hi there!".into()));
    for i in 1..=3u64 {
        assert_eq!(send(&pic, id, alice(), &format!("a-{}", i + 1), i, "more"), sent("Hi there!", i + 1));
    }
    assert_eq!(send(&pic, id, alice(), "a-5", 4, "more"), Err(ChatError::Busy));
    assert_eq!(send(&pic, id, carol(), "c-1", 0, "hello"), Err(ChatError::Busy));
    // A saved request still replays (no model call).
    assert_eq!(send(&pic, id, alice(), "a-4", 3, "more"), sent("Hi there!", 4));
    assert_eq!(llm_seen(&pic).len(), 5);
    // A minute later the same request is accepted.
    pic.advance_time(Duration::from_secs(61));
    assert_eq!(send(&pic, id, alice(), "a-5", 4, "more"), sent("Hi there!", 5));
    assert_eq!(send(&pic, id, carol(), "c-1", 0, "hello"), sent("Hi there!", 1));
}

#[test]
#[ignore]
fn when_storage_is_full_the_least_recently_used_conversations_make_way() {
    let (pic, id) = setup(read(SMALL_WASM));
    set_provider(&pic, id, LlmProvider::Fallback);

    // Eight entries is the limit of this build.
    for n in 0..8u32 {
        pic.advance_time(Duration::from_secs(1));
        assert_eq!(send(&pic, id, user(n), "first", 0, "hello").map(|s| s.revision), Ok(1));
    }
    assert_eq!(send(&pic, id, user(0), "second", 1, "again").map(|s| s.revision), Ok(2));
    let full = metrics(&pic, id);
    assert_eq!((full.entries, full.conversations, full.revision_floor), (8, 8, 0));

    // A ninth caller, who has read revision 0, is answered in one call: the least recently used conversation
    // (user 1's — user 0 just wrote) makes way.
    pic.advance_time(Duration::from_secs(1));
    assert_eq!(send(&pic, id, user(8), "first", 0, "hello").map(|s| s.revision), Ok(1));
    let after = metrics(&pic, id);
    assert_eq!((after.entries, after.revision_floor), (8, 2));
    assert_eq!(chat_messages(&pic, id, user(0)).len(), 4);
    // The caller that lost its conversation sees it empty, at a revision past everything it had seen, so nothing
    // it sent before can run again; from there it can go on.
    assert_eq!(conversation(&pic, id, user(1)), Conversation { messages: vec![], revision: 2 });
    assert_eq!(send(&pic, id, user(1), "first", 0, "hello"), Err(ChatError::Conflict));
    assert_eq!(send(&pic, id, user(1), "second", 1, "again"), Err(ChatError::Conflict));
    pic.advance_time(Duration::from_secs(1));
    assert_eq!(send(&pic, id, user(1), "third", 2, "back again").map(|s| s.revision), Ok(3));
    // Clearing without a conversation needs an entry too, but never takes a conversation's place: with only
    // conversations stored, it is refused and nothing changes.
    pic.advance_time(Duration::from_secs(1));
    let before = metrics(&pic, id);
    assert!(clear_refused(&pic, id, user(9)).contains("Coo is at capacity"));
    let after = metrics(&pic, id);
    assert_eq!((after.entries, after.conversations, after.revision_floor), (8, 8, before.revision_floor));

    // The byte budget (2 MiB in this build) works the same way: filled past it behind the canister's back,
    // the next chat first frees room down to 90 %.
    let filled: u64 = update(&pic, id, admin(), "test_fill", encode_args((6u32, 20u32, 9_000u32)).unwrap());
    assert!(filled > 2 * 1024 * 1024, "{filled}");
    pic.advance_time(Duration::from_secs(1));
    assert!(send(&pic, id, user(1), "fourth", 3, "still here").is_ok());
    let freed = metrics(&pic, id);
    assert!(freed.stored_bytes <= 2 * 1024 * 1024 * 90 / 100, "{freed:?}");
    assert_eq!(chat_messages(&pic, id, user(1)).len(), 4, "the caller's own conversation is never evicted");
    // And the state still survives an upgrade.
    upgrade(&pic, id, read(SMALL_WASM));
    assert_eq!(metrics(&pic, id).revision_floor, freed.revision_floor);
    assert_eq!(chat_messages(&pic, id, user(1)).len(), 4);
}

/// Clearing calls no model and has no rate limit: a newcomer clearing, however often and under however many
/// principals, takes only the place of entries without turns, never of a conversation.
#[test]
#[ignore]
fn newcomers_clearing_cannot_delete_conversations() {
    let (pic, id) = setup(read(SMALL_WASM));
    set_provider(&pic, id, LlmProvider::Fallback);
    // Six old conversations and two newer cleared entries: the eight entries this build allows.
    for n in 0..6u32 {
        pic.advance_time(Duration::from_secs(1));
        send(&pic, id, user(n), "first", 0, "hello").unwrap();
    }
    for n in 6..8u32 {
        pic.advance_time(Duration::from_secs(1));
        assert_eq!(clear(&pic, id, user(n)), 1);
    }
    let conversations = |pic: &PocketIc| (0..6u32).map(|n| chat_messages(pic, id, user(n))).collect::<Vec<_>>();
    let before = conversations(&pic);
    assert!(before.iter().all(|c| c.len() == 2));

    // Twenty newcomers clear: each one takes the place of the least recently updated empty entry.
    for n in 100..120u32 {
        pic.advance_time(Duration::from_secs(1));
        let revision = clear(&pic, id, user(n));
        assert_eq!(conversation(&pic, id, user(n)).revision, revision);
        let m = metrics(&pic, id);
        assert_eq!((m.entries, m.conversations), (8, 6), "after newcomer {n}");
    }
    assert_eq!(conversations(&pic), before);

    // The two entries without turns left (the last two newcomers') become conversations...
    for n in 118..120u32 {
        pic.advance_time(Duration::from_secs(1));
        let revision = conversation(&pic, id, user(n)).revision;
        send(&pic, id, user(n), "first", revision, "hello").unwrap();
    }
    let full = metrics(&pic, id);
    assert_eq!((full.entries, full.conversations), (8, 8));
    // ...and then a newcomer's clear is refused, removing nothing.
    pic.advance_time(Duration::from_secs(1));
    assert!(clear_refused(&pic, id, user(200)).contains("Coo is at capacity"));
    let refused = metrics(&pic, id);
    assert_eq!((refused.entries, refused.conversations, refused.revision_floor), (8, 8, full.revision_floor));
    assert_eq!(conversations(&pic), before);

    // A newcomer's message, unlike its clear, takes the place of the least recently used conversation (user 0's).
    let floor = refused.revision_floor;
    assert_eq!(send(&pic, id, user(200), "first", floor, "hello").map(|s| s.revision), Ok(floor + 1));
    assert!(chat_messages(&pic, id, user(0)).is_empty());
    assert!((1..6u32).all(|n| chat_messages(&pic, id, user(n)).len() == 2));
}
