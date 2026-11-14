//! Stand-in for the LLM canister (`w36hm-eqaaa-aaaal-qr76a-cai`) in PocketIC tests. It is installed at that id, so
//! the backend's real `v1_chat` call is exercised, and it records what it was asked.
//!
//! The types below are written from the LLM canister's interface (as published in the `ic-llm` crate), separately
//! from the backend's own copy, so a mistake in either shows up as a decoding failure in the tests.

use candid::{CandidType, Deserialize, Principal, Reserved};
use std::cell::{Cell, RefCell};

#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum ChatMessage {
    #[serde(rename = "user")]
    User { content: String },
    #[serde(rename = "system")]
    System { content: String },
    #[serde(rename = "assistant")]
    Assistant(AssistantMessage),
    #[serde(rename = "tool")]
    Tool { content: String, tool_call_id: String },
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct AssistantMessage {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct ToolCall {
    pub id: String,
    pub function: FunctionCall,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct FunctionCall {
    pub name: String,
    pub arguments: Vec<ToolCallArgument>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct ToolCallArgument {
    pub name: String,
    pub value: String,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct Request {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    /// The backend sends no tools; whatever arrives is accepted and ignored.
    pub tools: Option<Reserved>,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct Response {
    pub message: AssistantMessage,
}

/// How `v1_chat` answers.
#[derive(CandidType, Deserialize, Clone, Debug)]
pub enum Mode {
    /// Answer at once with this text.
    Reply(String),
    /// Answer with this text, but only after `mock_release` was called.
    Hold(String),
    /// Like `Hold`, then trap (the caller sees a rejected call).
    HoldThenReject,
    /// Trap at once.
    Reject,
    /// Answer with no content.
    Empty,
}

/// One recorded `v1_chat` request: (role, content) of each message.
#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Seen {
    pub model: String,
    pub messages: Vec<(String, String)>,
}

thread_local! {
    static MODE: RefCell<Mode> = RefCell::new(Mode::Reply("Hi there!".to_string()));
    static SEEN: RefCell<Vec<Seen>> = const { RefCell::new(Vec::new()) };
    // Calls held by `Hold` wait for the release count to pass the value they started with.
    static RELEASES: Cell<u64> = const { Cell::new(0) };
}

fn seen(request: &Request) -> Seen {
    let messages = request
        .messages
        .iter()
        .map(|m| match m {
            ChatMessage::User { content } => ("user".to_string(), content.clone()),
            ChatMessage::System { content } => ("system".to_string(), content.clone()),
            ChatMessage::Assistant(a) => ("assistant".to_string(), a.content.clone().unwrap_or_default()),
            ChatMessage::Tool { content, .. } => ("tool".to_string(), content.clone()),
        })
        .collect();
    Seen { model: request.model.clone(), messages }
}

fn answer(text: Option<String>) -> Response {
    Response { message: AssistantMessage { content: text, tool_calls: vec![] } }
}

/// Keeps this call open, one self-call per round, until `mock_release` is called.
async fn hold() {
    let started = RELEASES.with(Cell::get);
    while RELEASES.with(Cell::get) == started {
        let _: Result<(), _> = ic_cdk::call(ic_cdk::id(), "mock_noop", ()).await;
    }
}

#[ic_cdk::update]
async fn v1_chat(request: Request) -> Response {
    SEEN.with(|s| s.borrow_mut().push(seen(&request)));
    match MODE.with(|m| m.borrow().clone()) {
        Mode::Reply(text) => answer(Some(text)),
        Mode::Empty => answer(None),
        Mode::Reject => ic_cdk::trap("mock: rejected"),
        Mode::Hold(text) => {
            hold().await;
            answer(Some(text))
        }
        Mode::HoldThenReject => {
            hold().await;
            ic_cdk::trap("mock: rejected after holding")
        }
    }
}

#[ic_cdk::update]
fn mock_noop() {}

#[ic_cdk::update]
fn mock_set_mode(mode: Mode) {
    MODE.with(|m| *m.borrow_mut() = mode);
}

/// Lets every held `v1_chat` call finish.
#[ic_cdk::update]
fn mock_release() {
    RELEASES.with(|r| r.set(r.get() + 1));
}

/// Every request received so far, oldest first. (A request that ends in a trap right away is not recorded: the
/// trap rolls the recording back.)
#[ic_cdk::query]
fn mock_seen() -> Vec<Seen> {
    SEEN.with(|s| s.borrow().clone())
}

/// Calls `method` on `canister` from this canister, so a test can reach the backend with a canister as the caller
/// (inter-canister calls do not pass through `canister_inspect_message`).
#[ic_cdk::update]
async fn mock_proxy(canister: Principal, method: String, arg: Vec<u8>) -> Result<Vec<u8>, String> {
    ic_cdk::api::call::call_raw(canister, &method, arg, 0)
        .await
        .map_err(|(code, message)| format!("{code:?}: {message}"))
}
