//! Integration tests against a local PocketIC server.
//!
//! Ignored by default; run them with `make test-canister`, which builds the three wasm modules they load
//! (the canister, the canister with `trap-after-reply`, and the mock LLM canister) and points POCKET_IC_BIN at the
//! server bundled with dfx.
//!
//! The on-chain model is replaced by `mock_llm`, installed at the LLM canister's real id, so the canister's own
//! `v1_chat` call runs unchanged and the tests can hold, release and inspect it.

use candid::{decode_one, encode_args, encode_one, CandidType, Principal};
use eliza_backend::{ChatError, Conversation, LlmProvider, Message, MessageStatus, Sent};
use pocket_ic::common::rest::RawMessageId;
use pocket_ic::PocketIc;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use std::time::Duration;

const NEW_WASM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/wasm32-unknown-unknown/release/eliza_backend.wasm"
);
const TRAP_WASM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/trap-after-reply/wasm32-unknown-unknown/release/eliza_backend.wasm"
);
const MOCK_LLM_WASM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/wasm32-unknown-unknown/release/mock_llm.wasm"
);
/// The LLM canister the backend calls (mainnet id).
const LLM_CANISTER: &str = "w36hm-eqaaa-aaaal-qr76a-cai";

type SendResult = Result<Sent, ChatError>;

fn admin() -> Principal {
    Principal::self_authenticating(b"admin")
}
fn alice() -> Principal {
    Principal::self_authenticating(b"alice")
}
fn bob() -> Principal {
    Principal::self_authenticating(b"bob")
}

fn read(path: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {path}: {e} (build the wasm first)"))
}

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
    let llm = pic
        .create_canister_with_id(Some(admin()), None, llm())
        .expect("cannot create the mock LLM canister at the LLM canister id");
    pic.add_cycles(llm, 20_000_000_000_000);
    pic.install_canister(llm, read(MOCK_LLM_WASM), encode_args(()).unwrap(), Some(admin()));
    (pic, id)
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

/// The README's post-upgrade check: the last saved and restored state sizes in the canister log are equal.
fn assert_logged_sizes_match(pic: &PocketIc, id: Principal) {
    let logs: Vec<String> = pic
        .fetch_canister_logs(id, admin())
        .unwrap()
        .into_iter()
        .map(|r| String::from_utf8_lossy(&r.content).into_owned())
        .collect();
    let last_number = |prefix: &str| -> u64 {
        let line = logs.iter().rev().find(|l| l.contains(prefix)).unwrap_or_else(|| panic!("no {prefix:?} in {logs:?}"));
        let rest = &line[line.find(prefix).unwrap() + prefix.len()..];
        rest.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().unwrap()
    };
    assert_eq!(last_number("pre_upgrade: "), last_number("State restored from stable memory successfully ("));
}

fn update<T: CandidType + DeserializeOwned>(pic: &PocketIc, id: Principal, from: Principal, method: &str, arg: Vec<u8>) -> T {
    let bytes = pic
        .update_call(id, from, method, arg)
        .unwrap_or_else(|e| panic!("{method} rejected: {e:?}"));
    decode_one(&bytes).unwrap()
}

fn query<T: CandidType + DeserializeOwned>(pic: &PocketIc, id: Principal, from: Principal, method: &str, arg: Vec<u8>) -> T {
    let bytes = pic
        .query_call(id, from, method, arg)
        .unwrap_or_else(|e| panic!("{method} rejected: {e:?}"));
    decode_one(&bytes).unwrap()
}

fn send(pic: &PocketIc, id: Principal, from: Principal, request_id: &str, base: u64, text: &str) -> SendResult {
    update(pic, id, from, "send_message", encode_args((request_id, base, text)).unwrap())
}

fn submit(pic: &PocketIc, id: Principal, from: Principal, request_id: &str, base: u64, text: &str) -> RawMessageId {
    pic.submit_call(id, from, "send_message", encode_args((request_id, base, text)).unwrap())
        .unwrap()
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

/// The wasm of the version running on mainnet (v0.6.0), built by `make test-canister` from its commit.
fn old_wasm() -> Vec<u8> {
    let path = std::env::var("ELIZA_OLD_WASM")
        .expect("ELIZA_OLD_WASM must point at a build of the deployed version (make test-canister sets it)");
    read(&path)
}

#[test]
#[ignore]
fn upgrade_from_the_deployed_version_keeps_every_conversation() {
    let (pic, id) = setup(old_wasm());
    set_provider(&pic, id, LlmProvider::Fallback);
    let first = send(&pic, id, alice(), "a-1", 0, "hello").unwrap();
    let second = send(&pic, id, alice(), "a-2", 1, "tell me about icp").unwrap();
    let legacy: Result<String, String> = update(&pic, id, bob(), "chat", encode_one("who are you").unwrap());
    legacy.unwrap();
    let carol = Principal::self_authenticating(b"carol");
    send(&pic, id, carol, "c-1", 0, "hello").unwrap();
    let cleared: u64 = update(&pic, id, carol, "clear_conversation", encode_args(()).unwrap());
    assert_eq!(cleared, 2);
    let (alice_before, bob_before) = (chat_messages(&pic, id, alice()), chat_messages(&pic, id, bob()));
    assert_eq!((alice_before.len(), bob_before.len()), (4, 2));

    upgrade(&pic, id, read(NEW_WASM));

    assert_eq!(chat_messages(&pic, id, alice()), alice_before);
    assert_eq!(chat_messages(&pic, id, bob()), bob_before);
    assert!(chat_messages(&pic, id, carol).is_empty());
    assert_eq!(count(&pic, id), 2);
    assert_eq!(conversation(&pic, id, alice()).revision, 2);
    assert_eq!(conversation(&pic, id, bob()).revision, 1);
    assert_eq!(conversation(&pic, id, carol).revision, 2);
    // Requests saved before the upgrade still replay instead of running again, and stale ones stay refused.
    assert_eq!(status(&pic, id, alice(), "a-2"), MessageStatus::Done(second.clone()));
    assert_eq!(send(&pic, id, alice(), "a-1", 0, "hello"), Ok(Sent { reply: first.reply.clone(), revision: 2 }));
    assert_eq!(send(&pic, id, carol, "c-1", 0, "hello"), Err(ChatError::Conflict));
    assert_eq!(llm_seen(&pic).len(), 0, "nothing was run again");

    // And the upgraded canister keeps working, through another upgrade as well.
    set_provider(&pic, id, LlmProvider::Fallback);
    let third = send(&pic, id, alice(), "a-3", 2, "hi").unwrap();
    assert_eq!(third.revision, 3);
    upgrade(&pic, id, read(NEW_WASM));
    assert_eq!(status(&pic, id, alice(), "a-3"), MessageStatus::Done(third.clone()));
    assert_eq!(conversation(&pic, id, alice()).revision, 3);
    assert_eq!(chat_messages(&pic, id, alice()).len(), 6);
}

#[test]
#[ignore]
fn an_upgrade_without_saved_state_is_refused() {
    // An empty module has no pre_upgrade, so nothing reaches stable memory.
    let (pic, id) = setup(b"\0asm\x01\0\0\0".to_vec());
    pic.stop_canister(id, Some(admin())).unwrap();
    let result = pic.upgrade_canister(id, read(NEW_WASM), encode_args(()).unwrap(), Some(admin()));
    assert!(result.is_err(), "post_upgrade must trap instead of starting empty");
}

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

    let cleared: u64 = update(&pic, id, alice(), "clear_conversation", encode_args(()).unwrap());
    assert_eq!(cleared, 1);
    llm_release(&pic);
    assert_eq!(outcome(&pic, first), Err(ChatError::Cleared));
    let cleared = conversation(&pic, id, alice());
    assert_eq!((cleared.messages.len(), cleared.revision), (1, 1), "only the system prompt, next revision");
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
fn a_slow_model_call_that_fails_saves_nothing_and_cannot_run_later() {
    let (pic, id) = setup(read(NEW_WASM));
    llm_mode(&pic, Mode::HoldThenReject);

    let slow = submit(&pic, id, alice(), "x", 0, "first try");
    llm_pending(&pic, 1);
    pic.advance_time(Duration::from_secs(5 * 60 + 1));
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

#[test]
#[ignore]
fn a_chat_stuck_for_five_minutes_is_taken_over_and_its_late_reply_is_not_saved() {
    let (pic, id) = setup(read(NEW_WASM));
    llm_mode(&pic, Mode::Hold("late".into()));

    let stuck = submit(&pic, id, alice(), "x", 0, "first try");
    llm_pending(&pic, 1);
    assert_eq!(send(&pic, id, alice(), "y", 0, "second try"), Err(ChatError::Busy));
    pic.advance_time(Duration::from_secs(5 * 60 + 1));
    pic.tick(); // queries see the time of the latest round
    assert_eq!(status(&pic, id, alice(), "x"), MessageStatus::Unknown);

    // The new request takes the stuck one's place...
    let fresh = submit(&pic, id, alice(), "y", 0, "second try");
    llm_pending(&pic, 2);
    assert_eq!(status(&pic, id, alice(), "y"), MessageStatus::Pending);
    llm_release(&pic);
    // ...and only the new one is saved, whichever answer arrives first.
    assert_eq!(outcome(&pic, fresh), sent("late", 1));
    assert!(outcome(&pic, stuck).is_err());
    assert_eq!(
        chat_messages(&pic, id, alice()),
        [("user".to_string(), "second try".to_string()), ("assistant".to_string(), "late".to_string())]
    );
    assert_eq!(send(&pic, id, alice(), "x", 0, "first try"), Err(ChatError::Conflict));
}

#[test]
#[ignore]
fn clearing_bumps_the_revision_and_keeps_the_count_meaningful() {
    let (pic, id) = setup(read(NEW_WASM));
    set_provider(&pic, id, LlmProvider::Fallback);
    send(&pic, id, alice(), "a1", 0, "hello").unwrap();
    send(&pic, id, bob(), "b1", 0, "hello").unwrap();
    assert_eq!(count(&pic, id), 2);

    let cleared: u64 = update(&pic, id, alice(), "clear_conversation", encode_args(()).unwrap());
    assert_eq!(cleared, 2);
    assert_eq!(conversation(&pic, id, alice()).revision, 2);
    assert!(chat_messages(&pic, id, alice()).is_empty());
    let history: Vec<Message> = query(&pic, id, alice(), "get_conversation_history", encode_args(()).unwrap());
    assert!(history.iter().all(|m| m.role == "system"));
    assert_eq!(count(&pic, id), 1);

    // Clearing a conversation that never existed still moves the revision on, so a first message that was
    // delayed in transit cannot arrive after the clear and be saved.
    let carol = Principal::self_authenticating(b"carol");
    let cleared: u64 = update(&pic, id, carol, "clear_conversation", encode_args(()).unwrap());
    assert_eq!(cleared, 1);
    assert_eq!(conversation(&pic, id, carol).revision, 1);
    assert!(chat_messages(&pic, id, carol).is_empty());
    assert_eq!(send(&pic, id, carol, "late", 0, "sent before the clear"), Err(ChatError::Conflict));
    assert_eq!(count(&pic, id), 1);
    assert_eq!(send(&pic, id, alice(), "a2", 2, "again").map(|s| s.revision), Ok(3));
    // Anonymous callers have no conversation to clear.
    let _: u64 = update(&pic, id, Principal::anonymous(), "clear_conversation", encode_args(()).unwrap());
    assert_eq!(conversation(&pic, id, Principal::anonymous()).revision, 0);
}

#[test]
#[ignore]
fn llm_failure_is_an_error_not_a_trap() {
    // No LLM canister at all (as on a local replica): the call is rejected.
    let (pic, id) = setup_without_llm(read(NEW_WASM));
    assert!(matches!(send(&pic, id, alice(), "r-1", 0, "hello"), Err(ChatError::Failed(_))));
    assert!(chat_messages(&pic, id, alice()).is_empty());
    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Unknown);
    // Not left locked, and nothing changed the revision.
    assert!(matches!(send(&pic, id, alice(), "r-2", 0, "again"), Err(ChatError::Failed(_))));

    // The LLM canister rejects the call, or answers without content: an error too, and nothing is saved.
    let (pic, id) = setup(read(NEW_WASM));
    for mode in [Mode::Reject, Mode::Empty] {
        llm_mode(&pic, mode);
        assert!(matches!(send(&pic, id, alice(), "r-3", 0, "hello"), Err(ChatError::Failed(_))));
        assert!(chat_messages(&pic, id, alice()).is_empty());
        assert_eq!(conversation(&pic, id, alice()).revision, 0);
    }
    // The same request succeeds once the model answers.
    llm_mode(&pic, Mode::Reply("Hi there!".into()));
    assert_eq!(send(&pic, id, alice(), "r-3", 0, "hello"), sent("Hi there!", 1));
}

#[test]
#[ignore]
fn a_trap_in_the_reply_callback_rolls_back_and_releases_the_lock() {
    let (pic, id) = setup(read(TRAP_WASM));
    let call = submit(&pic, id, alice(), "r-1", 0, "trap after reply");
    assert!(pic.await_call(call).is_err(), "the call is rejected");
    assert_eq!(llm_seen(&pic).len(), 1, "the model was asked before the trap");
    assert!(chat_messages(&pic, id, alice()).is_empty());
    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Unknown);
    // The in-flight entry was released by the cleanup callback: not Busy.
    assert_eq!(send(&pic, id, alice(), "r-2", 0, "next"), sent("Hi there!", 1));
}

#[test]
#[ignore]
fn the_on_chain_request_is_what_the_llm_canister_expects() {
    let (pic, id) = setup(read(NEW_WASM));
    assert_eq!(send(&pic, id, alice(), "r-1", 0, "hello"), sent("Hi there!", 1));
    let asked = &llm_seen(&pic)[0];
    assert_eq!(asked.model, "llama3.1:8b");
    assert_eq!(asked.messages.len(), 2);
    assert_eq!(asked.messages[0].0, "system");
    assert_eq!(asked.messages[1], ("user".to_string(), "hello".to_string()));
}

#[test]
#[ignore]
fn rejects_anonymous_callers_and_bad_request_ids() {
    let (pic, id) = setup(read(NEW_WASM));
    set_provider(&pic, id, LlmProvider::Fallback);
    assert!(matches!(send(&pic, id, Principal::anonymous(), "r-1", 0, "hi"), Err(ChatError::Invalid(_))));
    let old: Result<String, String> = update(&pic, id, Principal::anonymous(), "chat", encode_one("hi").unwrap());
    assert!(old.is_err());
    for bad in ["", "has space", "ü", &"x".repeat(65)] {
        assert!(matches!(send(&pic, id, alice(), bad, 0, "hi"), Err(ChatError::Invalid(_))), "{bad:?}");
    }
    assert!(chat_messages(&pic, id, alice()).is_empty());

    // The old endpoint still works and moves the revision, so a client that did not see it gets a Conflict.
    let old: Result<String, String> = update(&pic, id, alice(), "chat", encode_one("hello").unwrap());
    assert!(old.is_ok());
    assert_eq!(send(&pic, id, alice(), &"x".repeat(64), 0, "hi"), Err(ChatError::Conflict));
    assert!(send(&pic, id, alice(), &"x".repeat(64), 1, "hi").is_ok());
    assert_eq!(chat_messages(&pic, id, alice()).len(), 4);
}
