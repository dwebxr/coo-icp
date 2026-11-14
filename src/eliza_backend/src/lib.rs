//! Coo: a chat agent in one canister. Signed-in callers each have one conversation with a model; the canister
//! keeps the conversations, enforces the limits, and guarantees that a request is saved at most once.
//!
//! - `api` — the types of the public interface
//! - `limits` — every limit, and the checks on what callers send
//! - `store` — the conversations, their byte budget, eviction
//! - `flight` — the chats in progress
//! - `llm` — building the model request and asking the model
//! - `chat` — sending, status, clearing
//! - `stable` — what is kept across upgrades
//! - `state` — the canister's one state cell

mod api;
mod chat;
mod flight;
mod limits;
mod llm;
mod stable;
mod state;
mod store;

pub use api::{Character, ChatError, Config, Conversation, LlmProvider, Message, MessageStatus, Metrics, Sent};

use candid::Principal;
use ic_cdk::api::{msg_caller, time};
use ic_cdk::{init, inspect_message, post_upgrade, pre_upgrade, query, update};
use stable::LENGTH_PREFIX_BYTES;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const WASM_PAGE_BYTES: u64 = 65536;

// ========== Lifecycle ==========

#[init]
fn init() {
    // The default state is the initial one: no conversations, the default character, the on-chain model.
}

// Any failure here traps, which aborts the upgrade and keeps the running version and its state.
#[pre_upgrade]
fn pre_upgrade() {
    let payload =
        state::read(stable::encode).unwrap_or_else(|e| ic_cdk::trap(format!("pre_upgrade: cannot save state: {e}")));

    let needed_pages = ((LENGTH_PREFIX_BYTES + payload.len()) as u64).div_ceil(WASM_PAGE_BYTES);
    let current_pages = ic_cdk::stable::stable_size();
    if current_pages < needed_pages && ic_cdk::stable::stable_grow(needed_pages - current_pages).is_err() {
        ic_cdk::trap(format!("pre_upgrade: cannot grow stable memory ({current_pages} -> {needed_pages} pages)"));
    }
    ic_cdk::stable::stable_write(0, &(payload.len() as u64).to_le_bytes());
    ic_cdk::stable::stable_write(LENGTH_PREFIX_BYTES as u64, &payload);
    // The same count as post_upgrade logs, so the two can be compared.
    ic_cdk::println!("pre_upgrade: {} bytes serialized to stable memory", payload.len());
}

// A saved state that cannot be read traps instead of starting empty: the upgrade is rolled back and no
// conversation is lost.
#[post_upgrade]
fn post_upgrade() {
    let pages = ic_cdk::stable::stable_size();
    // pre_upgrade always writes the state, so empty stable memory means it was not saved.
    if pages == 0 {
        ic_cdk::trap("post_upgrade: no saved state in stable memory");
    }
    let mut prefix = [0u8; LENGTH_PREFIX_BYTES];
    ic_cdk::stable::stable_read(0, &mut prefix);
    let len = stable::payload_len(prefix).unwrap_or_else(|e| ic_cdk::trap(format!("post_upgrade: {e}")));
    if (LENGTH_PREFIX_BYTES + len) as u64 > pages * WASM_PAGE_BYTES {
        ic_cdk::trap("post_upgrade: saved state is truncated");
    }
    let mut payload = vec![0u8; len];
    ic_cdk::stable::stable_read(LENGTH_PREFIX_BYTES as u64, &mut payload);
    let loaded = stable::decode(&payload)
        .unwrap_or_else(|e| ic_cdk::trap(format!("post_upgrade: cannot read saved state: {e}")));
    drop(payload);

    ic_cdk::println!("Restoring state (schema version {})", loaded.version);
    let (entries, bytes, provider) = (loaded.state.store.entries(), loaded.state.store.bytes(), loaded.state.provider);
    state::replace(loaded.state);
    ic_cdk::println!(
        "State restored from stable memory successfully ({len} bytes; {entries} entries, {bytes} bytes counted, provider {provider:?})"
    );
}

/// Runs on one replica before an ingress update call is accepted, and costs the canister nothing when it refuses.
/// A coarse filter only — who may call what, and how big the argument may be — so that junk is not paid for.
/// It is not what protects the methods: calls from other canisters never pass through here, and a single replica
/// decides. Every method checks its caller and its input again.
#[inspect_message]
fn inspect_message() {
    use limits::{MAX_ADMIN_ARG_BYTES, MAX_SEND_ARG_BYTES, MAX_SMALL_ARG_BYTES};

    let caller = msg_caller();
    let signed_in = caller != Principal::anonymous();
    let arg_bytes = ic_cdk::api::msg_arg_data().len();
    let accept = match ic_cdk::api::msg_method_name().as_str() {
        "send_message" => signed_in && arg_bytes <= MAX_SEND_ARG_BYTES,
        // Also the queries a client may want answered by the whole subnet rather than one replica.
        "clear_conversation" | "get_conversation" | "get_message_status" => {
            signed_in && arg_bytes <= MAX_SMALL_ARG_BYTES
        }
        "update_character" | "set_llm_provider" => {
            ic_cdk::api::is_controller(&caller) && arg_bytes <= MAX_ADMIN_ARG_BYTES
        }
        #[cfg(feature = "test-hooks")]
        "test_fill" => ic_cdk::api::is_controller(&caller),
        // Everything else is a query; call it as one.
        _ => false,
    };
    if accept {
        ic_cdk::api::accept_message();
    } else {
        ic_cdk::trap("not accepted: sign in, call queries as queries, and keep arguments small");
    }
}

// ========== Chat ==========

/// Sends a message as the next exchange after `base_revision`. A (request id, base revision) pair is saved at most
/// once: repeating it returns the saved reply, and a stale revision is a Conflict.
#[update]
async fn send_message(request_id: String, base_revision: u64, content: String) -> Result<Sent, ChatError> {
    chat::send_message(request_id, base_revision, content).await
}

/// The caller's conversation: user and assistant turns, oldest first.
#[query]
fn get_conversation() -> Conversation {
    state::read(|s| chat::conversation(s, &msg_caller()))
}

#[query]
fn get_message_status(request_id: String) -> MessageStatus {
    state::read(|s| chat::status(s, &msg_caller(), &request_id, time()))
}

/// Empties the caller's conversation and cancels saving a reply that is still being generated. The conversation
/// gets the next revision (even if there was none yet), so no request sent before the clear can run afterwards.
/// Returns the new revision.
#[update]
fn clear_conversation() -> u64 {
    let caller = msg_caller();
    if caller == Principal::anonymous() {
        ic_cdk::trap("Sign in first.");
    }
    state::mutate(|s| chat::clear(s, caller, time())).unwrap_or_else(|e| ic_cdk::trap(e))
}

// ========== Character and configuration ==========

#[query]
fn get_character() -> Character {
    state::read(|s| s.character.clone())
}

/// Controllers only. The new character applies to every conversation from its next message on.
#[update]
fn update_character(character: Character) -> Result<(), String> {
    require_controller()?;
    limits::validate_character(&character)?;
    state::mutate(|s| s.character = character);
    Ok(())
}

#[query]
fn get_config() -> Config {
    state::read(|s| Config { llm_provider: s.provider })
}

/// Controllers only.
#[update]
fn set_llm_provider(provider: LlmProvider) -> Result<(), String> {
    require_controller()?;
    state::mutate(|s| s.provider = provider);
    Ok(())
}

/// Administration is for the canister's controllers — those who can replace its code anyway.
fn require_controller() -> Result<(), String> {
    if ic_cdk::api::is_controller(&msg_caller()) {
        Ok(())
    } else {
        Err("Only a controller of the canister can do this".to_string())
    }
}

// ========== Status ==========

/// Conversations with at least one saved exchange.
#[query]
fn get_conversation_count() -> u64 {
    state::read(|s| s.store.conversations() as u64)
}

/// "ok | v<version> | <n> conversations": the canister answers and its state is loaded. It says nothing about
/// whether the model can be reached.
#[query]
fn health() -> String {
    format!("ok | v{} | {} conversations", VERSION, state::read(|s| s.store.conversations()))
}

#[query]
fn version() -> String {
    VERSION.to_string()
}

#[query]
fn get_metrics() -> Metrics {
    state::read(|s| Metrics {
        version: VERSION.to_string(),
        conversations: s.store.conversations() as u64,
        entries: s.store.entries() as u64,
        stored_bytes: s.store.bytes() as u64,
        revision_floor: s.store.revision_floor(),
        chats_in_flight: s.flights.live(time()) as u64,
        stable_memory_pages: ic_cdk::stable::stable_size(),
        wasm_memory_bytes: wasm_memory_bytes(),
    })
}

#[cfg(target_arch = "wasm32")]
fn wasm_memory_bytes() -> u64 {
    core::arch::wasm32::memory_size(0) as u64 * WASM_PAGE_BYTES
}

#[cfg(not(target_arch = "wasm32"))]
fn wasm_memory_bytes() -> u64 {
    0
}

// ========== Test hooks (feature `test-hooks`; never part of a deployed build) ==========

/// Fills the store with `callers` conversations of `exchanges` exchanges of `bytes`-byte texts, past any limit,
/// so tests can upgrade a full canister. Returns the bytes counted afterwards.
#[cfg(feature = "test-hooks")]
#[update]
fn test_fill(callers: u32, exchanges: u32, bytes: u32) -> u64 {
    require_controller().unwrap_or_else(|e| ic_cdk::trap(e));
    let now = time();
    state::mutate(|s| {
        for c in 0..callers {
            let mut id = [0xfeu8; 29];
            id[..4].copy_from_slice(&c.to_be_bytes());
            let caller = Principal::from_slice(&id);
            for e in 0..exchanges {
                let text = "x".repeat(bytes as usize);
                s.store.append_exchange(caller, text.clone(), text, format!("fill-{e}"), now);
            }
        }
        s.store.bytes() as u64
    })
}

ic_cdk::export_candid!();

#[cfg(test)]
mod tests {
    use candid_parser::utils::{service_compatible, service_equal, CandidSource};

    fn did_file() -> String {
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/eliza_backend.did")).unwrap()
    }

    /// `eliza_backend.did` is what clients and dfx use; it must be what this code exports.
    /// Regenerate it with `make candid`.
    #[test]
    fn the_candid_file_matches_the_code() {
        service_equal(CandidSource::Text(&super::__export_service()), CandidSource::Text(&did_file()))
            .expect("eliza_backend.did is out of date: run `make candid`");
    }

    /// The four methods the 0.6.0 frontend calls, with the types it expects. A tab opened before an upgrade keeps
    /// working only as long as the new interface can stand in for this one.
    const USED_BY_V060_CLIENTS: &str = r#"
        type ChatError = variant { Invalid : text; Failed : text; Busy; Cleared; InProgress; Conflict };
        type Message = record { request_id : opt text; content : text; role : text };
        type Conversation = record { messages : vec Message; revision : nat64 };
        type Sent = record { revision : nat64; reply : text };
        type MessageStatus = variant { Done : Sent; Unknown; Pending };
        service : {
          send_message : (text, nat64, text) -> (variant { Ok : Sent; Err : ChatError });
          get_conversation : () -> (Conversation) query;
          get_message_status : (text) -> (MessageStatus) query;
          clear_conversation : () -> (nat64);
        }
    "#;

    #[test]
    fn clients_of_the_previous_version_still_understand_the_interface() {
        service_compatible(CandidSource::Text(&did_file()), CandidSource::Text(USED_BY_V060_CLIENTS))
            .expect("the interface no longer serves 0.6.0 clients");
    }
}
