use candid::{CandidType, Deserialize, Principal};
use ic_cdk::api::management_canister::http_request::{
    http_request, CanisterHttpRequestArgument, HttpHeader, HttpMethod, HttpResponse, TransformArgs,
    TransformContext, TransformFunc,
};
use ic_cdk_macros::{init, pre_upgrade, post_upgrade, query, update};
use serde::Serialize;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt;

// ========== Data Structures ==========

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct Message {
    pub role: String,    // "user", "assistant", "system"
    pub content: String,
    /// The request id of a user message sent with send_message. Absent in state saved before v0.6.0.
    pub request_id: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct Character {
    pub name: String,
    pub system_prompt: String,
    pub bio: Vec<String>,
    pub style: Vec<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct ConversationState {
    pub messages: Vec<Message>,
    pub character: Character,
    pub created_at: u64,
    pub updated_at: u64,
    /// Bumped by every saved exchange and by clearing, so it never repeats for a caller. send_message only runs
    /// when the client's revision matches, which is what saves a (request id, base revision) pair at most once.
    /// Absent (= 0) in state saved before v0.6.0.
    pub revision: Option<u64>,
}

/// A conversation and its revision, for clients of send_message.
#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct Conversation {
    pub messages: Vec<Message>,
    pub revision: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Sent {
    pub reply: String,
    /// The conversation's revision now.
    pub revision: u64,
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ChatError {
    /// Another message from this caller is being answered. Nothing was saved; retry later with the same id.
    Busy,
    /// This request id is already being answered; poll `get_message_status`.
    InProgress,
    /// The conversation changed since the client's revision. Nothing was run; reload and send again.
    Conflict,
    /// The conversation was cleared while the reply was generated. Nothing was saved.
    Cleared,
    Invalid(String),
    /// The model could not reply. Nothing was saved.
    Failed(String),
}

impl fmt::Display for ChatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChatError::Busy => f.write_str("Another message is still being answered. Try again in a moment."),
            ChatError::InProgress => f.write_str("This message is still being answered."),
            ChatError::Conflict => f.write_str("The conversation changed. Reload and try again."),
            ChatError::Cleared => f.write_str("The conversation was cleared."),
            ChatError::Invalid(msg) | ChatError::Failed(msg) => f.write_str(msg),
        }
    }
}

#[derive(CandidType, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum MessageStatus {
    /// Not being answered and not in the conversation (never received, failed, cleared, or trimmed away).
    Unknown,
    Pending,
    Done(Sent),
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub enum LlmProvider {
    OnChain,           // IC LLM Canister (fully on-chain) - mainnet only
    OpenAI,            // HTTPS Outcalls to OpenAI
    Fallback,          // Simple pattern matching (for local dev)
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct Config {
    pub llm_provider: LlmProvider,
    pub max_conversation_length: usize,
    pub admin: Principal,
}

// ========== State Management ==========

/// A chat being answered for a caller.
struct InFlight {
    token: u64,
    request_id: Option<String>,
    started_at: u64,
    cleared: bool,
}

thread_local! {
    static CONVERSATIONS: RefCell<HashMap<Principal, ConversationState>> = RefCell::new(HashMap::new());
    static ENCRYPTED_API_KEY: RefCell<Option<Vec<u8>>> = RefCell::new(None);
    static CHARACTER: RefCell<Option<Character>> = RefCell::new(None);
    static CONFIG: RefCell<Option<Config>> = RefCell::new(None);
    // At most one chat per caller. Heap-only on purpose: upgrades run with the canister stopped, so none is open.
    static IN_FLIGHT: RefCell<HashMap<Principal, InFlight>> = RefCell::new(HashMap::new());
    static NEXT_TOKEN: Cell<u64> = const { Cell::new(0) };
}

const MAX_REQUEST_ID_BYTES: usize = 64;
// An entry older than this is assumed stuck and may be taken over (agents give up on a call after 5 minutes).
const STALE_FLIGHT_NS: u64 = 5 * 60 * 1_000_000_000;
const MAX_STATE_BYTES: usize = 100_000_000;

// ========== Stable Memory for Upgrades ==========

/// State that persists across canister upgrades
#[derive(CandidType, Deserialize, Serialize, Clone, Default)]
struct StableState {
    #[serde(default)]
    version: u32, // Schema version for forward compatibility

    // Core state
    conversations: HashMap<Principal, ConversationState>,
    encrypted_api_key: Option<Vec<u8>>,
    character: Option<Character>,
    config: Option<Config>,
}

// ========== Initialization ==========

fn default_character() -> Character {
    Character {
        name: "Coo".to_string(),
        system_prompt: r#"You are Coo, a helpful AI assistant built on the elizaOS framework, running fully on-chain on the Internet Computer.

You are:
- Friendly and approachable
- Knowledgeable about blockchain, Web3, and the Internet Computer
- Running as a decentralized, censorship-resistant AI agent
- Built on elizaOS - the leading open-source AI agent framework
- Capable of maintaining context across conversations

Your responses should be:
- Concise but helpful
- Engaging and conversational
- Accurate and informative"#.to_string(),
        bio: vec![
            "On-chain AI agent powered by elizaOS and Internet Computer".to_string(),
            "Fully decentralized and censorship-resistant".to_string(),
            "Built on elizaOS framework for autonomous AI agents".to_string(),
        ],
        style: vec![
            "Friendly".to_string(),
            "Helpful".to_string(),
            "Knowledgeable".to_string(),
        ],
    }
}

#[init]
fn init() {
    let caller = ic_cdk::caller();

    CHARACTER.with(|c| {
        *c.borrow_mut() = Some(default_character());
    });

    CONFIG.with(|cfg| {
        *cfg.borrow_mut() = Some(Config {
            // Use IC LLM Canister (Llama 3.1 8B) for mainnet
            llm_provider: LlmProvider::OnChain,
            max_conversation_length: 50,
            admin: caller,
        });
    });
}

/// The stable-memory blob: an 8-byte little-endian length followed by the candid-encoded state.
fn encode_stable_state(state: &StableState) -> Result<Vec<u8>, String> {
    let payload = candid::encode_one(state).map_err(|e| e.to_string())?;
    let mut blob = Vec::with_capacity(8 + payload.len());
    blob.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    blob.extend_from_slice(&payload);
    Ok(blob)
}

fn decode_state_len(prefix: [u8; 8]) -> Result<usize, String> {
    let len = u64::from_le_bytes(prefix) as usize;
    if len == 0 || len > MAX_STATE_BYTES {
        return Err(format!("invalid saved state length {}", len));
    }
    Ok(len)
}

fn decode_stable_state(blob: &[u8]) -> Result<StableState, String> {
    let prefix: [u8; 8] = blob
        .get(..8)
        .and_then(|b| b.try_into().ok())
        .ok_or("saved state is truncated")?;
    let len = decode_state_len(prefix)?;
    let payload = blob.get(8..8 + len).ok_or("saved state is truncated")?;
    candid::decode_one(payload).map_err(|e| e.to_string())
}

// Any failure here traps, which aborts the upgrade and keeps the running version and its state.
#[pre_upgrade]
fn pre_upgrade() {
    let state = StableState {
        version: 2,
        conversations: CONVERSATIONS.with(|c| c.borrow().clone()),
        encrypted_api_key: ENCRYPTED_API_KEY.with(|k| k.borrow().clone()),
        character: CHARACTER.with(|c| c.borrow().clone()),
        config: CONFIG.with(|c| c.borrow().clone()),
    };

    let blob = encode_stable_state(&state)
        .unwrap_or_else(|e| ic_cdk::trap(&format!("pre_upgrade: cannot serialize state: {}", e)));

    // 1 page = 64KB
    let needed_pages = (blob.len() as u64).div_ceil(65536);
    let current_pages = ic_cdk::api::stable::stable_size();
    if current_pages < needed_pages && ic_cdk::api::stable::stable_grow(needed_pages - current_pages).is_err() {
        ic_cdk::trap(&format!(
            "pre_upgrade: cannot grow stable memory ({} -> {} pages)",
            current_pages, needed_pages
        ));
    }

    ic_cdk::api::stable::stable_write(0, &blob);
    // Same count as post_upgrade logs (the state without its length prefix), so the two can be compared.
    ic_cdk::println!("pre_upgrade: {} bytes serialized to stable memory", blob.len() - 8);
}

// A saved state that cannot be read traps instead of starting empty: the upgrade is rolled back and no
// conversation is lost.
#[post_upgrade]
fn post_upgrade() {
    // pre_upgrade always writes the state, so empty stable memory means it was not saved.
    if ic_cdk::api::stable::stable_size() == 0 {
        ic_cdk::trap("post_upgrade: no saved state in stable memory");
    }
    {
        let mut prefix = [0u8; 8];
        ic_cdk::api::stable::stable_read(0, &mut prefix);
        let len = decode_state_len(prefix).unwrap_or_else(|e| ic_cdk::trap(&format!("post_upgrade: {}", e)));
        let mut blob = vec![0u8; 8 + len];
        ic_cdk::api::stable::stable_read(0, &mut blob);
        let state = decode_stable_state(&blob)
            .unwrap_or_else(|e| ic_cdk::trap(&format!("post_upgrade: cannot decode saved state: {}", e)));

        ic_cdk::println!("Restoring state (schema version {})", state.version);
        CONVERSATIONS.with(|c| *c.borrow_mut() = state.conversations);
        ENCRYPTED_API_KEY.with(|k| *k.borrow_mut() = state.encrypted_api_key);
        CHARACTER.with(|c| *c.borrow_mut() = state.character);
        CONFIG.with(|c| *c.borrow_mut() = state.config);

        // Force LLM provider to OnChain for mainnet
        CONFIG.with(|c| {
            if let Some(ref mut cfg) = *c.borrow_mut() {
                cfg.llm_provider = LlmProvider::OnChain;
            }
        });
        ic_cdk::println!("State restored from stable memory successfully ({} bytes, LLM: OnChain)", len);
    }

    // Defaults for anything missing from the saved state
    CHARACTER.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(default_character());
        }
    });

    CONFIG.with(|cfg| {
        if cfg.borrow().is_none() {
            *cfg.borrow_mut() = Some(Config {
                llm_provider: LlmProvider::OnChain,
                max_conversation_length: 50,
                admin: ic_cdk::caller(),
            });
        }
    });
}

// ========== Eliza Chat Endpoint ==========

fn new_conversation(now: u64) -> ConversationState {
    let character = CHARACTER.with(|ch| ch.borrow().clone().unwrap_or_else(default_character));
    ConversationState {
        messages: vec![Message {
            role: "system".to_string(),
            content: character.system_prompt.clone(),
            request_id: None,
        }],
        character,
        created_at: now,
        updated_at: now,
        revision: None,
    }
}

fn max_conversation_length() -> usize {
    CONFIG.with(|cfg| {
        cfg.borrow()
            .as_ref()
            .map(|c| c.max_conversation_length)
            .unwrap_or(50)
    })
}

/// Keeps the first message (the system prompt) and the newest ones once there are more than `max_len`.
fn trim_messages(messages: &mut Vec<Message>, max_len: usize) {
    let max_len = max_len.max(2);
    if messages.len() > max_len {
        let excess = messages.len() - max_len;
        messages.drain(1..1 + excess);
    }
}

fn append_exchange(
    state: &mut ConversationState,
    user_message: String,
    reply: String,
    request_id: Option<String>,
    max_len: usize,
    now: u64,
) {
    state.messages.push(Message {
        role: "user".to_string(),
        content: user_message,
        request_id,
    });
    trim_messages(&mut state.messages, max_len);
    state.messages.push(Message {
        role: "assistant".to_string(),
        content: reply,
        request_id: None,
    });
    state.revision = Some(state.revision.unwrap_or(0) + 1);
    state.updated_at = now;
}

fn revision_of(caller: &Principal) -> u64 {
    CONVERSATIONS.with(|c| c.borrow().get(caller).and_then(|s| s.revision).unwrap_or(0))
}

/// The saved user message with this request id and the reply right after it. Trimming removes a user message
/// before its reply, so a found message always has its own reply next.
fn saved_exchange(caller: &Principal, request_id: &str) -> Option<(String, String)> {
    CONVERSATIONS.with(|c| {
        let conversations = c.borrow();
        let messages = &conversations.get(caller)?.messages;
        let i = messages
            .iter()
            .position(|m| m.role == "user" && m.request_id.as_deref() == Some(request_id))?;
        let reply = messages.get(i + 1).filter(|m| m.role == "assistant")?;
        Some((messages[i].content.clone(), reply.content.clone()))
    })
}

fn validate_request_id(request_id: &str) -> Result<(), ChatError> {
    let valid = !request_id.is_empty()
        && request_id.len() <= MAX_REQUEST_ID_BYTES
        && request_id.bytes().all(|b| b.is_ascii_graphic());
    if valid {
        Ok(())
    } else {
        Err(ChatError::Invalid("Invalid request id.".to_string()))
    }
}

/// Releases the caller's in-flight entry when the chat ends, including after a trap in the reply callback:
/// ic-cdk drops the future in the cleanup callback, so this must only touch the thread-local, never system APIs.
struct FlightGuard {
    caller: Principal,
    token: u64,
}

impl FlightGuard {
    /// False when the conversation was cleared, or the entry was taken over, while the reply was generated.
    fn still_current(&self) -> bool {
        IN_FLIGHT.with(|f| {
            f.borrow()
                .get(&self.caller)
                .is_some_and(|e| e.token == self.token && !e.cleared)
        })
    }
}

impl Drop for FlightGuard {
    fn drop(&mut self) {
        IN_FLIGHT.with(|f| {
            let mut f = f.borrow_mut();
            if f.get(&self.caller).is_some_and(|e| e.token == self.token) {
                f.remove(&self.caller);
            }
        });
    }
}

fn begin_flight(caller: Principal, request_id: Option<&str>, now: u64) -> Result<FlightGuard, ChatError> {
    IN_FLIGHT.with(|f| {
        let mut f = f.borrow_mut();
        if let Some(current) = f.get(&caller) {
            if now.saturating_sub(current.started_at) < STALE_FLIGHT_NS {
                let same = request_id.is_some() && current.request_id.as_deref() == request_id;
                return Err(if same { ChatError::InProgress } else { ChatError::Busy });
            }
        }
        let token = NEXT_TOKEN.with(|t| {
            t.set(t.get() + 1);
            t.get()
        });
        f.insert(
            caller,
            InFlight {
                token,
                request_id: request_id.map(str::to_owned),
                started_at: now,
                cleared: false,
            },
        );
        Ok(FlightGuard { caller, token })
    })
}

fn flight_pending(caller: &Principal, request_id: &str, now: u64) -> bool {
    IN_FLIGHT.with(|f| {
        f.borrow().get(caller).is_some_and(|e| {
            e.request_id.as_deref() == Some(request_id)
                && !e.cleared
                && now.saturating_sub(e.started_at) < STALE_FLIGHT_NS
        })
    })
}

fn mark_flight_cleared(caller: &Principal) {
    IN_FLIGHT.with(|f| {
        if let Some(e) = f.borrow_mut().get_mut(caller) {
            e.cleared = true;
        }
    });
}

/// `request` is the request id and the revision the client last saw; None for the original `chat` endpoint.
async fn run_chat(request: Option<(String, u64)>, user_message: String) -> Result<Sent, ChatError> {
    let caller = ic_cdk::caller();
    // Anonymous callers would all share one conversation.
    if caller == Principal::anonymous() {
        return Err(ChatError::Invalid("Sign in to chat.".to_string()));
    }
    let request_id = match &request {
        Some((id, _)) => {
            validate_request_id(id)?;
            if let Some((sent, reply)) = saved_exchange(&caller, id) {
                if sent != user_message {
                    return Err(ChatError::Invalid("This request id was used for another message.".to_string()));
                }
                return Ok(Sent { reply, revision: revision_of(&caller) });
            }
            Some(id.clone())
        }
        None => None,
    };

    let now = ic_cdk::api::time();
    let guard = begin_flight(caller, request_id.as_deref(), now)?;
    let revision = revision_of(&caller);
    if let Some((_, base)) = &request {
        if *base != revision {
            return Err(ChatError::Conflict);
        }
    }
    let max_len = max_conversation_length();

    // The model sees the conversation as it will be saved.
    let mut prompt = CONVERSATIONS
        .with(|c| c.borrow().get(&caller).cloned())
        .unwrap_or_else(|| new_conversation(now));
    prompt.messages.push(Message {
        role: "user".to_string(),
        content: user_message.clone(),
        request_id: None,
    });
    trim_messages(&mut prompt.messages, max_len);

    let response = generate_response(&prompt).await.map_err(ChatError::Failed)?;

    #[cfg(feature = "trap-after-reply")]
    if user_message == "trap after reply" {
        ic_cdk::trap("test: trap in the reply callback");
    }

    // With one chat per caller, only a clear (or a takeover of a stuck chat) can change the conversation meanwhile.
    if !guard.still_current() || revision_of(&caller) != revision {
        return Err(ChatError::Cleared);
    }
    // Applied to the stored conversation rather than writing the copy back, so nothing that happened during the
    // await is overwritten.
    let now = ic_cdk::api::time();
    CONVERSATIONS.with(|c| {
        let mut conversations = c.borrow_mut();
        let state = conversations.entry(caller).or_insert_with(|| new_conversation(now));
        append_exchange(state, user_message, response.clone(), request_id, max_len, now);
    });
    Ok(Sent { reply: response, revision: revision + 1 })
}

/// Sends a message as the next exchange after `base_revision`. A (request id, base revision) pair is saved at most
/// once: repeating it returns the saved reply, and a stale revision is a Conflict.
#[update]
async fn send_message(request_id: String, base_revision: u64, content: String) -> Result<Sent, ChatError> {
    run_chat(Some((request_id, base_revision)), content).await
}

/// Original endpoint without a request id, kept for older clients.
#[update]
async fn chat(user_message: String) -> Result<String, String> {
    run_chat(None, user_message).await.map(|sent| sent.reply).map_err(|e| e.to_string())
}

#[query]
fn get_conversation() -> Conversation {
    let caller = ic_cdk::caller();
    CONVERSATIONS.with(|c| match c.borrow().get(&caller) {
        Some(state) => Conversation {
            messages: state.messages.clone(),
            revision: state.revision.unwrap_or(0),
        },
        None => Conversation { messages: vec![], revision: 0 },
    })
}

#[query]
fn get_message_status(request_id: String) -> MessageStatus {
    let caller = ic_cdk::caller();
    if let Some((_, reply)) = saved_exchange(&caller, &request_id) {
        MessageStatus::Done(Sent { reply, revision: revision_of(&caller) })
    } else if flight_pending(&caller, &request_id, ic_cdk::api::time()) {
        MessageStatus::Pending
    } else {
        MessageStatus::Unknown
    }
}

// ========== LLM Inference ==========

async fn generate_response(state: &ConversationState) -> Result<String, String> {
    let provider = CONFIG.with(|cfg| {
        cfg.borrow()
            .as_ref()
            .map(|c| c.llm_provider.clone())
            .unwrap_or(LlmProvider::Fallback)
    });

    match provider {
        LlmProvider::OnChain => generate_response_onchain(state).await,
        LlmProvider::OpenAI => generate_response_openai(state).await,
        LlmProvider::Fallback => generate_response_fallback(state),
    }
}

// Option 1: IC LLM Canister (Llama 3.1 8B - fully on-chain)
// Note: IC LLM Canister only available on mainnet (w36hm-eqaaa-aaaal-qr76a-cai)
async fn generate_response_onchain(state: &ConversationState) -> Result<String, String> {
    use ic_llm::{AssistantMessage, ChatMessage, Model};

    // Convert our messages to IC LLM format
    // IC LLM has a limit of 10 messages, so we take the most recent ones
    let messages: Vec<ChatMessage> = state.messages
        .iter()
        .rev()
        .take(10)
        .rev()
        .map(|m| match m.role.as_str() {
            "system" => ChatMessage::System {
                content: m.content.clone(),
            },
            "user" => ChatMessage::User {
                content: m.content.clone(),
            },
            "assistant" => ChatMessage::Assistant(AssistantMessage {
                content: Some(m.content.clone()),
                tool_calls: vec![],
            }),
            _ => ChatMessage::User {
                content: m.content.clone(),
            },
        })
        .collect();

    // Called directly rather than through ic_llm::chat(..).send(), which unwraps the call result and so turns a
    // rejected call into a trap. Same request as ic-llm sends.
    let llm = Principal::from_text(LLM_CANISTER).map_err(|e| e.to_string())?;
    let request = LlmRequest {
        model: Model::Llama3_1_8B.to_string(),
        messages,
        tools: None,
    };
    let (response,): (ic_llm::Response,) = ic_cdk::call(llm, "v1_chat", (request,))
        .await
        .map_err(|(code, msg)| format!("LLM call failed ({:?}): {}", code, msg))?;

    response.message.content.ok_or_else(|| "No response content from LLM".to_string())
}

const LLM_CANISTER: &str = "w36hm-eqaaa-aaaal-qr76a-cai";

/// The request type of the LLM canister's `v1_chat` (private in ic-llm).
#[derive(CandidType)]
struct LlmRequest {
    model: String,
    messages: Vec<ic_llm::ChatMessage>,
    tools: Option<Vec<ic_llm::Tool>>,
}

// Fallback for local development (simple pattern matching)
fn generate_response_fallback(state: &ConversationState) -> Result<String, String> {
    let last_user_message = state.messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.as_str())
        .unwrap_or("");

    let response = match last_user_message.to_lowercase() {
        msg if msg.contains("hello") || msg.contains("hi") || msg.contains("こんにちは") => {
            format!("Hello! I'm {}, your on-chain AI assistant built on elizaOS. Note: I'm running in fallback mode (local dev). \
            Deploy to mainnet for full Llama 3.1 powered responses!", state.character.name)
        }
        msg if msg.contains("who are you") || msg.contains("what are you") => {
            format!("I'm {}, an AI agent built on the elizaOS framework, running on the Internet Computer. {}",
                state.character.name,
                state.character.bio.join(" "))
        }
        msg if msg.contains("elizaos") || msg.contains("eliza") => {
            format!("I'm {} - built on elizaOS, the leading open-source framework for autonomous AI agents. \
            elizaOS enables developers to create intelligent agents that can operate across multiple platforms. \
            I'm deployed on the Internet Computer for fully decentralized, on-chain AI!", state.character.name)
        }
        msg if msg.contains("internet computer") || msg.contains("icp") => {
            "The Internet Computer is a blockchain that runs at web speed and hosts fully decentralized applications. \
            On mainnet, I use Llama 3.1 8B for intelligent responses!".to_string()
        }
        _ => {
            format!("[Fallback Mode] I'm {} - built on elizaOS, running locally without IC LLM Canister. \
            Deploy me to mainnet for full AI capabilities with Llama 3.1 8B! \
            Your message: '{}'", state.character.name, last_user_message)
        }
    };

    Ok(response)
}

// Option 2: HTTPS Outcalls to OpenAI API
async fn generate_response_openai(state: &ConversationState) -> Result<String, String> {
    // Get decrypted API key
    let api_key = decrypt_api_key().await?;

    // Build messages JSON
    let messages_json: Vec<serde_json::Value> = state.messages.iter().map(|m| {
        serde_json::json!({
            "role": m.role,
            "content": m.content
        })
    }).collect();

    let request_body = serde_json::json!({
        "model": "gpt-4o-mini",
        "messages": messages_json,
        "max_tokens": 500,
        "temperature": 0.7
    });

    let request_body_bytes = request_body.to_string().into_bytes();

    let request = CanisterHttpRequestArgument {
        url: "https://api.openai.com/v1/chat/completions".to_string(),
        max_response_bytes: Some(10_000),
        method: HttpMethod::POST,
        headers: vec![
            HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            },
            HttpHeader {
                name: "Authorization".to_string(),
                value: format!("Bearer {}", api_key),
            },
        ],
        body: Some(request_body_bytes),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_openai_response".to_string(),
            }),
            context: vec![],
        }),
    };

    // Attach cycles for HTTP request
    let cycles = 50_000_000_000u128; // 50B cycles

    match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 decode error: {}", e))?;

            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("JSON parse error: {}", e))?;

            json["choices"][0]["message"]["content"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "No response content".to_string())
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

// Transform function for HTTPS Outcalls
#[query]
fn transform_openai_response(raw: TransformArgs) -> HttpResponse {
    HttpResponse {
        status: raw.response.status,
        body: raw.response.body,
        headers: vec![],
    }
}

// ========== API Key Management (vetKeys integration placeholder) ==========

async fn decrypt_api_key() -> Result<String, String> {
    let encrypted_key = ENCRYPTED_API_KEY.with(|k| k.borrow().clone())
        .ok_or_else(|| "No API key stored. Please call store_encrypted_api_key first.".to_string())?;

    // In production, this would use vetKeys for decryption
    // For now, we store the key directly (NOT secure for production)
    String::from_utf8(encrypted_key)
        .map_err(|e| format!("Decryption error: {}", e))
}

#[update]
fn store_encrypted_api_key(encrypted_key: Vec<u8>) -> Result<(), String> {
    // Check if caller is admin
    let caller = ic_cdk::caller();
    let is_admin = CONFIG.with(|cfg| {
        cfg.borrow()
            .as_ref()
            .map(|c| c.admin == caller)
            .unwrap_or(false)
    });

    if !is_admin {
        return Err("Only admin can store API key".to_string());
    }

    ENCRYPTED_API_KEY.with(|k| {
        *k.borrow_mut() = Some(encrypted_key);
    });

    Ok(())
}

// ========== Character Management ==========

#[update]
fn update_character(character: Character) -> Result<(), String> {
    // Check if caller is admin
    let caller = ic_cdk::caller();
    let is_admin = CONFIG.with(|cfg| {
        cfg.borrow()
            .as_ref()
            .map(|c| c.admin == caller)
            .unwrap_or(false)
    });

    if !is_admin {
        return Err("Only admin can update character".to_string());
    }

    CHARACTER.with(|c| {
        *c.borrow_mut() = Some(character);
    });

    Ok(())
}

#[query]
fn get_character() -> Option<Character> {
    CHARACTER.with(|c| c.borrow().clone())
}

// ========== Configuration Management ==========

#[update]
fn set_llm_provider(provider: LlmProvider) -> Result<(), String> {
    let caller = ic_cdk::caller();
    let is_admin = CONFIG.with(|cfg| {
        cfg.borrow()
            .as_ref()
            .map(|c| c.admin == caller)
            .unwrap_or(false)
    });

    if !is_admin {
        return Err("Only admin can change LLM provider".to_string());
    }

    CONFIG.with(|cfg| {
        if let Some(config) = cfg.borrow_mut().as_mut() {
            config.llm_provider = provider;
        }
    });

    Ok(())
}

#[query]
fn get_config() -> Option<Config> {
    CONFIG.with(|cfg| cfg.borrow().clone())
}

/// Transfer admin rights to a new principal
#[update]
fn set_admin(new_admin: Principal) -> Result<(), String> {
    require_admin()?;

    CONFIG.with(|cfg| {
        if let Some(config) = cfg.borrow_mut().as_mut() {
            config.admin = new_admin;
            Ok(())
        } else {
            Err("Config not initialized".to_string())
        }
    })
}

// ========== Conversation Management ==========

#[query]
fn get_conversation_history() -> Vec<Message> {
    let caller = ic_cdk::caller();
    CONVERSATIONS.with(|c| {
        c.borrow()
            .get(&caller)
            .map(|s| s.messages.clone())
            .unwrap_or_default()
    })
}

/// Also cancels saving a reply that is still being generated. The conversation is replaced by an empty one with
/// the next revision (even if there was none yet), so no request sent before the clear can run afterwards.
/// Returns the new revision (older clients that expect no result ignore it).
#[update]
fn clear_conversation() -> u64 {
    let caller = ic_cdk::caller();
    if caller == Principal::anonymous() {
        return 0;
    }
    mark_flight_cleared(&caller);
    let now = ic_cdk::api::time();
    CONVERSATIONS.with(|c| {
        let mut conversations = c.borrow_mut();
        let revision = conversations.get(&caller).and_then(|s| s.revision).unwrap_or(0) + 1;
        let mut fresh = new_conversation(now);
        fresh.revision = Some(revision);
        conversations.insert(caller, fresh);
        revision
    })
}

/// Conversations with at least one message from the user (cleared ones are kept only for their revision).
fn conversation_count() -> u64 {
    CONVERSATIONS.with(|c| {
        c.borrow()
            .values()
            .filter(|s| s.messages.iter().any(|m| m.role == "user"))
            .count() as u64
    })
}

#[query]
fn get_conversation_count() -> u64 {
    conversation_count()
}

// ========== Health Check ==========

#[query]
fn health() -> String {
    let has_config = CONFIG.with(|c| c.borrow().is_some());
    let has_character = CHARACTER.with(|c| c.borrow().is_some());
    if has_config && has_character {
        format!("ok | v{} | {} conversations", VERSION, conversation_count())
    } else {
        format!("degraded | config={} character={}", has_config, has_character)
    }
}

const VERSION: &str = "0.6.0";

#[query]
fn version() -> String {
    VERSION.to_string()
}

#[derive(CandidType, Serialize)]
struct CanisterMetrics {
    version: String,
    conversations: u64,
    stable_memory_pages: u64,
    heap_memory_bytes: u64,
}

#[cfg(target_arch = "wasm32")]
fn get_heap_memory_bytes() -> u64 {
    (core::arch::wasm32::memory_size(0) as u64) * 65536
}

#[cfg(not(target_arch = "wasm32"))]
fn get_heap_memory_bytes() -> u64 { 0 }

#[query]
fn get_metrics() -> CanisterMetrics {
    CanisterMetrics {
        version: VERSION.to_string(),
        conversations: conversation_count(),
        stable_memory_pages: ic_cdk::api::stable::stable_size(),
        heap_memory_bytes: get_heap_memory_bytes(),
    }
}

fn require_admin() -> Result<(), String> {
    let caller = ic_cdk::caller();
    let is_admin = CONFIG.with(|cfg| {
        cfg.borrow()
            .as_ref()
            .map(|c| c.admin == caller)
            .unwrap_or(false)
    });

    if !is_admin {
        return Err("Only admin can perform this action".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_character_has_name() {
        let ch = default_character();
        assert!(!ch.name.is_empty());
    }

    #[test]
    fn default_character_has_system_prompt() {
        let ch = default_character();
        assert!(!ch.system_prompt.is_empty());
    }

    #[test]
    fn default_character_has_bio_and_style() {
        let ch = default_character();
        assert!(!ch.bio.is_empty());
        assert!(!ch.style.is_empty());
    }

    fn msg(role: &str, content: &str) -> Message {
        Message { role: role.to_string(), content: content.to_string(), request_id: None }
    }

    fn conversation(len: usize) -> ConversationState {
        let mut state = new_conversation(0);
        for i in 1..len {
            state.messages.push(msg(if i % 2 == 1 { "user" } else { "assistant" }, &format!("m{}", i)));
        }
        state
    }

    fn contents(messages: &[Message]) -> Vec<String> {
        messages.iter().map(|m| m.content.clone()).collect()
    }

    #[test]
    fn trim_keeps_the_first_message_and_the_newest() {
        let mut state = conversation(52);
        trim_messages(&mut state.messages, 50);
        assert_eq!(state.messages.len(), 50);
        assert_eq!(state.messages[0].role, "system");
        assert_eq!(state.messages[1].content, "m3");
        assert_eq!(state.messages[49].content, "m51");

        let mut short = conversation(10);
        trim_messages(&mut short.messages, 50);
        assert_eq!(short.messages.len(), 10);

        // Index 0 is kept whatever its role, as before.
        let mut no_system: Vec<Message> = (0..5).map(|i| msg("user", &format!("u{}", i))).collect();
        trim_messages(&mut no_system, 3);
        assert_eq!(contents(&no_system), ["u0", "u3", "u4"]);

        // A limit below 2 would drop the message just added.
        let mut tiny = conversation(4);
        trim_messages(&mut tiny.messages, 0);
        assert_eq!(contents(&tiny.messages[1..]), ["m3"]);
    }

    #[test]
    fn append_matches_the_previous_chat_arithmetic() {
        for len in [1, 48, 49, 50, 51] {
            let mut state = conversation(len);
            // What v0.5.0 did: push user, trim, push assistant.
            let mut expected = state.messages.clone();
            expected.push(msg("user", "q"));
            if expected.len() > 50 {
                let system = expected[0].clone();
                let recent: Vec<Message> = expected.iter().skip(expected.len() - 50 + 1).cloned().collect();
                expected = vec![system];
                expected.extend(recent);
            }
            expected.push(msg("assistant", "a"));

            append_exchange(&mut state, "q".into(), "a".into(), None, 50, 7);
            assert_eq!(contents(&state.messages), contents(&expected), "len {}", len);
            assert_eq!(state.updated_at, 7);
            assert_eq!(state.revision, Some(1));
        }
    }

    #[test]
    fn replays_from_the_conversation_until_trimmed_away() {
        let caller = Principal::from_slice(&[7; 29]);
        let mut state = new_conversation(0);
        for i in 0..30 {
            append_exchange(&mut state, format!("q{}", i), format!("a{}", i), Some(format!("r{}", i)), 50, 0);
        }
        assert_eq!(state.revision, Some(30));
        // The system prompt and the newest 50 messages: q5 onwards.
        assert_eq!(state.messages.len(), 51);
        assert_eq!(state.messages[1].content, "q5");
        CONVERSATIONS.with(|c| c.borrow_mut().insert(caller, state));
        assert_eq!(revision_of(&caller), 30);
        assert_eq!(saved_exchange(&caller, "r29"), Some(("q29".into(), "a29".into())));
        assert_eq!(saved_exchange(&caller, "r5"), Some(("q5".into(), "a5".into())));
        assert_eq!(saved_exchange(&caller, "r4"), None);
        assert_eq!(saved_exchange(&caller, "r0"), None);
        assert_eq!(saved_exchange(&Principal::from_slice(&[8; 29]), "r29"), None);
        assert_eq!(revision_of(&Principal::from_slice(&[8; 29])), 0);
    }

    #[test]
    fn messages_without_ids_never_replay() {
        let caller = Principal::from_slice(&[9; 29]);
        let mut state = new_conversation(0);
        append_exchange(&mut state, "q".into(), "a".into(), None, 50, 0);
        CONVERSATIONS.with(|c| c.borrow_mut().insert(caller, state));
        assert_eq!(saved_exchange(&caller, "q"), None);
    }

    #[test]
    fn one_chat_per_caller() {
        let alice = Principal::from_slice(&[1; 29]);
        let bob = Principal::from_slice(&[2; 29]);
        let guard = begin_flight(alice, Some("r1"), 100).unwrap();
        assert!(matches!(begin_flight(alice, Some("r1"), 101), Err(ChatError::InProgress)));
        assert!(matches!(begin_flight(alice, Some("r2"), 101), Err(ChatError::Busy)));
        assert!(matches!(begin_flight(alice, None, 101), Err(ChatError::Busy)));
        let other = begin_flight(bob, Some("r1"), 101).unwrap();
        assert!(flight_pending(&alice, "r1", 102));
        assert!(!flight_pending(&alice, "r2", 102));
        assert!(guard.still_current());

        drop(guard);
        assert!(!flight_pending(&alice, "r1", 103));
        assert!(begin_flight(alice, Some("r2"), 104).is_ok());
        drop(other);
    }

    #[test]
    fn clearing_cancels_the_save() {
        let alice = Principal::from_slice(&[3; 29]);
        let guard = begin_flight(alice, Some("r1"), 0).unwrap();
        mark_flight_cleared(&alice);
        assert!(!guard.still_current());
        assert!(!flight_pending(&alice, "r1", 1));
        // Still busy until the cleared chat finishes.
        assert!(matches!(begin_flight(alice, Some("r2"), 1), Err(ChatError::Busy)));
        drop(guard);
        assert!(begin_flight(alice, Some("r2"), 2).is_ok());
    }

    #[test]
    fn a_stuck_chat_can_be_taken_over() {
        let alice = Principal::from_slice(&[4; 29]);
        let stuck = begin_flight(alice, Some("r1"), 0).unwrap();
        assert!(matches!(begin_flight(alice, Some("r2"), STALE_FLIGHT_NS - 1), Err(ChatError::Busy)));
        assert!(!flight_pending(&alice, "r1", STALE_FLIGHT_NS));
        let fresh = begin_flight(alice, Some("r2"), STALE_FLIGHT_NS).unwrap();
        // The stuck chat may not save, and finishing it does not release the new one.
        assert!(!stuck.still_current());
        drop(stuck);
        assert!(fresh.still_current());
        assert!(flight_pending(&alice, "r2", STALE_FLIGHT_NS + 1));
    }

    #[test]
    fn request_ids_are_short_printable_ascii() {
        assert!(validate_request_id("0f8c2c1e-4b7a-4d2e-9a51-8f2b7c6d5e4a").is_ok());
        assert!(validate_request_id(&"x".repeat(64)).is_ok());
        for bad in ["", "has space", "tab\t", "ü", "\u{7f}"] {
            assert!(validate_request_id(bad).is_err(), "{:?}", bad);
        }
        assert!(validate_request_id(&"x".repeat(65)).is_err());
    }

    /// The state types exactly as v0.5.0 (the version on mainnet) encoded them.
    mod v050 {
        use super::super::{Character, Config};
        use candid::{CandidType, Principal};
        use std::collections::HashMap;

        #[derive(CandidType, Clone)]
        pub struct Message {
            pub role: String,
            pub content: String,
        }

        #[derive(CandidType)]
        pub struct ConversationState {
            pub messages: Vec<Message>,
            pub character: Character,
            pub created_at: u64,
            pub updated_at: u64,
        }

        #[derive(CandidType)]
        pub struct StableState {
            pub version: u32,
            pub conversations: HashMap<Principal, ConversationState>,
            pub encrypted_api_key: Option<Vec<u8>>,
            pub character: Option<Character>,
            pub config: Option<Config>,
        }

        /// The blob v0.5.0's pre_upgrade wrote.
        pub fn blob(state: &StableState) -> Vec<u8> {
            let payload = candid::encode_one(state).unwrap();
            let mut blob = (payload.len() as u64).to_le_bytes().to_vec();
            blob.extend_from_slice(&payload);
            blob
        }
    }

    #[test]
    fn restores_state_saved_by_v050() {
        let alice = Principal::from_slice(&[5; 29]);
        let old_msg = |role: &str, content: &str| v050::Message { role: role.into(), content: content.into() };
        let messages = vec![old_msg("system", "prompt"), old_msg("user", "hi"), old_msg("assistant", "hello")];
        let old = v050::StableState {
            version: 1,
            conversations: [(
                alice,
                v050::ConversationState {
                    messages: messages.clone(),
                    character: default_character(),
                    created_at: 1,
                    updated_at: 2,
                },
            )]
            .into_iter()
            .collect(),
            encrypted_api_key: Some(b"key".to_vec()),
            character: Some(default_character()),
            config: Some(Config { llm_provider: LlmProvider::OnChain, max_conversation_length: 50, admin: alice }),
        };
        // Stable memory is page-sized, so the blob is followed by zeros.
        let mut stable = v050::blob(&old);
        stable.resize(65536, 0);

        let state = decode_stable_state(&stable).unwrap();
        let restored = &state.conversations[&alice];
        assert_eq!(contents(&restored.messages), ["prompt", "hi", "hello"]);
        assert!(restored.messages.iter().all(|m| m.request_id.is_none()));
        assert_eq!((restored.created_at, restored.updated_at), (1, 2));
        assert!(restored.revision.is_none());
        assert_eq!(state.encrypted_api_key.as_deref(), Some(&b"key"[..]));
        assert_eq!(state.config.unwrap().admin, alice);
    }

    #[test]
    fn new_state_roundtrips() {
        let alice = Principal::from_slice(&[6; 29]);
        let mut conversation = new_conversation(0);
        append_exchange(&mut conversation, "q".into(), "a".into(), Some("r1".into()), 50, 1);
        let state = StableState {
            version: 2,
            conversations: [(alice, conversation)].into_iter().collect(),
            encrypted_api_key: None,
            character: Some(default_character()),
            config: None,
        };
        let restored = decode_stable_state(&encode_stable_state(&state).unwrap()).unwrap();
        assert_eq!(restored.version, 2);
        let conversation = &restored.conversations[&alice];
        assert_eq!(conversation.revision, Some(1));
        assert_eq!(conversation.messages[1].request_id.as_deref(), Some("r1"));
    }

    #[test]
    fn unreadable_state_is_an_error() {
        assert!(decode_stable_state(&[]).is_err());
        assert!(decode_stable_state(&0u64.to_le_bytes()).is_err());
        let mut truncated = 100u64.to_le_bytes().to_vec();
        truncated.extend_from_slice(&[0; 10]);
        assert!(decode_stable_state(&truncated).is_err());
        let mut garbage = 4u64.to_le_bytes().to_vec();
        garbage.extend_from_slice(b"junk");
        assert!(decode_stable_state(&garbage).is_err());
        assert!(decode_state_len(((MAX_STATE_BYTES + 1) as u64).to_le_bytes()).is_err());
    }
}

// Candid export
ic_cdk::export_candid!();
