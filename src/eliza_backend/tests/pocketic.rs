//! Integration tests against a local PocketIC server.
//!
//! Ignored by default. Run with:
//!   cargo build --target wasm32-unknown-unknown --release -p eliza_backend
//!   cargo build --target wasm32-unknown-unknown --release -p eliza_backend --features trap-after-reply \
//!     --target-dir target/trap-after-reply
//!   POCKET_IC_BIN=~/.cache/dfinity/versions/0.30.2/pocket-ic \
//!   ELIZA_OLD_WASM=<v0.5.0 wasm, for the upgrade test; skipped without it> \
//!   cargo test -p eliza_backend --test pocketic -- --ignored --test-threads=1
//! or `make test-canister`.

use candid::{decode_one, encode_args, encode_one, CandidType, Principal};
use eliza_backend::{ChatError, Config, Conversation, LlmProvider, Message, MessageStatus, Sent};
use pocket_ic::common::rest::{
    CanisterHttpReply, CanisterHttpRequest, CanisterHttpResponse, MockCanisterHttpResponse, RawMessageId,
};
use pocket_ic::PocketIc;
use serde::de::DeserializeOwned;
use std::time::Duration;

const NEW_WASM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/wasm32-unknown-unknown/release/eliza_backend.wasm"
);
const TRAP_WASM: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../target/trap-after-reply/wasm32-unknown-unknown/release/eliza_backend.wasm"
);

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

fn setup(wasm: Vec<u8>) -> (PocketIc, Principal) {
    let pic = PocketIc::new();
    let id = pic.create_canister_with_settings(Some(admin()), None);
    pic.add_cycles(id, 20_000_000_000_000);
    pic.install_canister(id, wasm, encode_args(()).unwrap(), Some(admin()));
    (pic, id)
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

fn openai(pic: &PocketIc, id: Principal) {
    let r: Result<(), String> = update(pic, id, admin(), "store_encrypted_api_key", encode_one(b"sk-test".to_vec()).unwrap());
    r.unwrap();
    set_provider(pic, id, LlmProvider::OpenAI);
}

/// Ticks until an HTTPS outcall whose request body contains `marker` is waiting for a response.
fn pending_http(pic: &PocketIc, marker: &str) -> CanisterHttpRequest {
    for _ in 0..20 {
        pic.tick();
        if let Some(req) = pic
            .get_canister_http()
            .into_iter()
            .find(|r| String::from_utf8_lossy(&r.body).contains(marker))
        {
            return req;
        }
    }
    panic!("no pending HTTPS outcall for {marker:?}");
}

fn reply_http(pic: &PocketIc, req: &CanisterHttpRequest, body: &str) {
    pic.mock_canister_http_response(MockCanisterHttpResponse {
        subnet_id: req.subnet_id,
        request_id: req.request_id,
        response: CanisterHttpResponse::CanisterHttpReply(CanisterHttpReply {
            status: 200,
            headers: vec![],
            body: body.as_bytes().to_vec(),
        }),
        additional_responses: vec![],
    });
}

const OPENAI_REPLY: &str = r#"{"choices":[{"message":{"content":"Hi there!"}}]}"#;

fn sent(reply: &str, revision: u64) -> SendResult {
    Ok(Sent { reply: reply.to_string(), revision })
}

#[test]
#[ignore]
fn upgrade_from_v050_keeps_every_conversation() {
    let Ok(old) = std::env::var("ELIZA_OLD_WASM") else {
        eprintln!("SKIPPED upgrade test: set ELIZA_OLD_WASM to a build of v0.5.0");
        return;
    };
    let (pic, id) = setup(read(&old));
    set_provider(&pic, id, LlmProvider::Fallback);
    for (who, text) in [(alice(), "hello"), (bob(), "who are you"), (alice(), "tell me about icp")] {
        let r: Result<String, String> = update(&pic, id, who, "chat", encode_one(text).unwrap());
        r.unwrap();
    }
    let history = |who| -> Vec<Message> { query(&pic, id, who, "get_conversation_history", encode_args(()).unwrap()) };
    let plain = |messages: Vec<Message>| -> Vec<(String, String)> { messages.into_iter().map(|m| (m.role, m.content)).collect() };
    let (alice_before, bob_before) = (plain(history(alice())), plain(history(bob())));
    assert_eq!(alice_before.len(), 5);

    upgrade(&pic, id, read(NEW_WASM));

    assert_eq!(plain(history(alice())), alice_before);
    assert_eq!(plain(history(bob())), bob_before);
    assert!(history(alice()).iter().all(|m| m.request_id.is_none()));
    assert_eq!(count(&pic, id), 2);
    let config: Option<Config> = query(&pic, id, admin(), "get_config", encode_args(()).unwrap());
    assert!(matches!(config.unwrap().llm_provider, LlmProvider::OnChain));
    // Restored conversations start at revision 0 and know no request ids.
    assert_eq!(conversation(&pic, id, alice()).revision, 0);
    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Unknown);

    set_provider(&pic, id, LlmProvider::Fallback);
    let first = send(&pic, id, alice(), "r-1", 0, "hi").unwrap();
    assert_eq!(first.revision, 1);
    assert_eq!(chat_messages(&pic, id, alice()).len(), 6);
    // An old client that expects clear_conversation to return nothing still decodes the reply.
    let reply = pic.update_call(id, bob(), "clear_conversation", encode_args(()).unwrap()).unwrap();
    let () = candid::decode_args(&reply).unwrap();

    // Request ids and revisions survive the next upgrade.
    upgrade(&pic, id, read(NEW_WASM));
    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Done(first.clone()));
    assert_eq!(conversation(&pic, id, alice()).revision, 1);
    set_provider(&pic, id, LlmProvider::Fallback);
    assert_eq!(send(&pic, id, alice(), "r-1", 0, "hi"), Ok(first));
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
    openai(&pic, id);

    let first = submit(&pic, id, alice(), "r-1", 0, "hello");
    let req = pending_http(&pic, "hello");

    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Pending);
    assert_eq!(send(&pic, id, alice(), "r-2", 0, "other"), Err(ChatError::Busy));
    assert_eq!(send(&pic, id, alice(), "r-1", 0, "hello"), Err(ChatError::InProgress));
    // Another user is not blocked by alice's request.
    assert_eq!(status(&pic, id, bob(), "r-1"), MessageStatus::Unknown);

    let cleared: u64 = update(&pic, id, alice(), "clear_conversation", encode_args(()).unwrap());
    assert_eq!(cleared, 1);
    reply_http(&pic, &req, OPENAI_REPLY);
    assert_eq!(outcome(&pic, first), Err(ChatError::Cleared));
    let cleared = conversation(&pic, id, alice());
    assert_eq!((cleared.messages.len(), cleared.revision), (1, 1), "only the system prompt, next revision");
    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Unknown);
    // The request from before the clear can never run.
    assert_eq!(send(&pic, id, alice(), "r-1", 0, "hello"), Err(ChatError::Conflict));

    // The lock is released: a new request goes through, and repeating it replays the saved reply.
    let second = submit(&pic, id, alice(), "r-2", 1, "other");
    let req = pending_http(&pic, "other");
    reply_http(&pic, &req, OPENAI_REPLY);
    assert_eq!(outcome(&pic, second), sent("Hi there!", 2));
    assert_eq!(status(&pic, id, alice(), "r-2"), MessageStatus::Done(Sent { reply: "Hi there!".into(), revision: 2 }));
    assert_eq!(send(&pic, id, alice(), "r-2", 1, "other"), sent("Hi there!", 2));
    assert_eq!(chat_messages(&pic, id, alice()).len(), 2);
    assert!(pic.get_canister_http().is_empty(), "replay must not call the LLM");
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

// (Taking over a chat stuck for 5 minutes is covered by unit tests: PocketIC times out the outcall first.)
#[test]
#[ignore]
fn a_timed_out_llm_call_saves_nothing_and_cannot_run_later() {
    let (pic, id) = setup(read(NEW_WASM));
    openai(&pic, id);

    let slow = submit(&pic, id, alice(), "x", 0, "first try");
    pending_http(&pic, "first try");
    pic.advance_time(Duration::from_secs(5 * 60 + 1));
    assert!(matches!(outcome(&pic, slow), Err(ChatError::Failed(_))));
    assert!(chat_messages(&pic, id, alice()).is_empty());
    assert_eq!(status(&pic, id, alice(), "x"), MessageStatus::Unknown);

    // Not left locked; once something else is saved, the failed request can no longer run.
    let next = submit(&pic, id, alice(), "y", 0, "second try");
    let req = pending_http(&pic, "second try");
    reply_http(&pic, &req, OPENAI_REPLY);
    assert_eq!(outcome(&pic, next), sent("Hi there!", 1));
    assert_eq!(send(&pic, id, alice(), "x", 0, "first try"), Err(ChatError::Conflict));
    assert_eq!(chat_messages(&pic, id, alice()).len(), 2);
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
    // Fresh installs use the on-chain LLM canister, which does not exist in PocketIC: the call is rejected.
    let (pic, id) = setup(read(NEW_WASM));
    assert!(matches!(send(&pic, id, alice(), "r-1", 0, "hello"), Err(ChatError::Failed(_))));
    assert!(chat_messages(&pic, id, alice()).is_empty());
    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Unknown);
    // Not left locked, and nothing changed the revision.
    assert!(matches!(send(&pic, id, alice(), "r-2", 0, "again"), Err(ChatError::Failed(_))));

    // An unusable LLM response is an error too, and nothing is saved.
    openai(&pic, id);
    let call = submit(&pic, id, alice(), "r-3", 0, "hello");
    let req = pending_http(&pic, "hello");
    reply_http(&pic, &req, "not json");
    assert!(matches!(outcome(&pic, call), Err(ChatError::Failed(_))));
    assert!(chat_messages(&pic, id, alice()).is_empty());
}

#[test]
#[ignore]
fn a_trap_in_the_reply_callback_rolls_back_and_releases_the_lock() {
    let (pic, id) = setup(read(TRAP_WASM));
    openai(&pic, id);
    let call = submit(&pic, id, alice(), "r-1", 0, "trap after reply");
    let req = pending_http(&pic, "trap after reply");
    reply_http(&pic, &req, OPENAI_REPLY);
    assert!(pic.await_call(call).is_err(), "the call is rejected");
    assert!(chat_messages(&pic, id, alice()).is_empty());
    assert_eq!(status(&pic, id, alice(), "r-1"), MessageStatus::Unknown);
    // The in-flight entry was released by the cleanup callback: not Busy.
    let next = submit(&pic, id, alice(), "r-2", 0, "next");
    let req = pending_http(&pic, "next");
    reply_http(&pic, &req, OPENAI_REPLY);
    assert_eq!(outcome(&pic, next), sent("Hi there!", 1));
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
