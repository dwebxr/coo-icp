//! The types of the canister's public interface.

use candid::{CandidType, Deserialize};
use serde::Serialize;

/// One turn of a conversation, as returned by `get_conversation`.
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// "user" or "assistant".
    pub role: String,
    pub content: String,
    /// The request id a user message was sent with.
    pub request_id: Option<String>,
}

/// The caller's conversation and its revision.
#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub messages: Vec<Message>,
    /// Changes with every saved exchange and every clear, and never repeats for a caller.
    pub revision: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Sent {
    pub reply: String,
    /// The conversation's revision now.
    pub revision: u64,
}

/// Why `send_message` saved nothing.
#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ChatError {
    /// Try again later, with the same request id: another message from this caller is being answered, or the
    /// canister is at one of its limits (chats in progress, model calls per minute, storage).
    Busy,
    /// This request id is already being answered; poll `get_message_status`.
    InProgress,
    /// The conversation changed since the client's revision. Nothing was run, and this request never will be;
    /// reload and send again.
    Conflict,
    /// The conversation was cleared while the reply was generated.
    Cleared,
    /// The request itself is refused, whenever it is sent: anonymous caller, malformed request id, a request id
    /// already used for another message, or an empty or too long message.
    Invalid(String),
    /// The model did not answer, or not in time.
    Failed(String),
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum MessageStatus {
    /// Not being answered and not in the conversation (never received, failed, cleared, or trimmed away).
    Unknown,
    Pending,
    Done(Sent),
}

/// Who Coo is. All of it goes into the system message of every model request.
#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
pub struct Character {
    pub name: String,
    pub system_prompt: String,
    pub bio: Vec<String>,
    pub style: Vec<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum LlmProvider {
    /// The Internet Computer's LLM canister (mainnet only).
    OnChain,
    /// A canned echo, for local replicas where the LLM canister does not exist.
    Fallback,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub llm_provider: LlmProvider,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Metrics {
    pub version: String,
    /// Conversations with at least one saved exchange.
    pub conversations: u64,
    /// All entries, including cleared conversations that only keep their revision.
    pub entries: u64,
    /// Bytes counted against the storage budget.
    pub stored_bytes: u64,
    /// The revision of every caller without an entry; raised whenever an entry is removed to make room.
    pub revision_floor: u64,
    pub chats_in_flight: u64,
    /// Pages of stable memory allocated (64 KiB each). Stable memory is only written during upgrades.
    pub stable_memory_pages: u64,
    /// Size of the Wasm memory, which grows but never shrinks; not the bytes currently in use.
    pub wasm_memory_bytes: u64,
}
