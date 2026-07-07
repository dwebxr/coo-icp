use candid::{CandidType, Deserialize, Principal};
use ic_cdk::api::management_canister::http_request::{
    http_request, CanisterHttpRequestArgument, HttpHeader, HttpMethod, HttpResponse, TransformArgs,
    TransformContext, TransformFunc,
};
use ic_cdk_macros::{init, pre_upgrade, post_upgrade, query, update};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;

// ========== Data Structures ==========

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct Message {
    pub role: String,    // "user", "assistant", "system"
    pub content: String,
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

thread_local! {
    static CONVERSATIONS: RefCell<HashMap<Principal, ConversationState>> = RefCell::new(HashMap::new());
    static ENCRYPTED_API_KEY: RefCell<Option<Vec<u8>>> = RefCell::new(None);
    static CHARACTER: RefCell<Option<Character>> = RefCell::new(None);
    static CONFIG: RefCell<Option<Config>> = RefCell::new(None);
}

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

#[pre_upgrade]
fn pre_upgrade() {
    let state = StableState {
        version: 1,
        conversations: CONVERSATIONS.with(|c| c.borrow().clone()),
        encrypted_api_key: ENCRYPTED_API_KEY.with(|k| k.borrow().clone()),
        character: CHARACTER.with(|c| c.borrow().clone()),
        config: CONFIG.with(|c| c.borrow().clone()),
    };

    let serialized = match candid::encode_one(&state) {
        Ok(data) => data,
        Err(e) => {
            ic_cdk::println!("CRITICAL: Failed to serialize state during pre_upgrade: {}", e);
            if ic_cdk::api::stable::stable_size() > 0 {
                ic_cdk::api::stable::stable_write(0, &0u64.to_le_bytes());
            }
            return;
        }
    };

    let len = serialized.len() as u64;
    let len_bytes = len.to_le_bytes();

    // 1 page = 64KB
    let needed_pages = ((8 + serialized.len()) as u64 + 65535) / 65536;
    let current_pages = ic_cdk::api::stable::stable_size();
    if current_pages < needed_pages {
        match ic_cdk::api::stable::stable_grow(needed_pages - current_pages) {
            Ok(_) => {}
            Err(_) => {
                ic_cdk::println!(
                    "CRITICAL: Failed to grow stable memory ({} -> {} pages). State may be lost.",
                    current_pages, needed_pages
                );
                return;
            }
        }
    }

    ic_cdk::api::stable::stable_write(0, &len_bytes);
    ic_cdk::api::stable::stable_write(8, &serialized);
    ic_cdk::println!("pre_upgrade: {} bytes serialized to stable memory", serialized.len());
}

#[post_upgrade]
fn post_upgrade() {
    // Try to restore from stable memory
    let stable_size = ic_cdk::api::stable::stable_size();

    if stable_size > 0 {
        // Read length prefix
        let mut len_bytes = [0u8; 8];
        ic_cdk::api::stable::stable_read(0, &mut len_bytes);
        let len = u64::from_le_bytes(len_bytes) as usize;

        if len > 0 && len < 100_000_000 {
            let mut serialized = vec![0u8; len];
            ic_cdk::api::stable::stable_read(8, &mut serialized);

            match candid::decode_one::<StableState>(&serialized) {
            Ok(state) => {
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
                return;
            }
            Err(e) => {
                ic_cdk::println!(
                    "CRITICAL: Failed to deserialize state ({} bytes): {}. Falling back to defaults.",
                    len, e
                );
            }
            }
        } else {
            ic_cdk::println!(
                "WARNING: Invalid stable memory length: {} bytes. Falling back to defaults.",
                len
            );
        }
    }

    // Fallback: initialize defaults if restoration failed
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

#[update]
async fn chat(user_message: String) -> Result<String, String> {
    let caller = ic_cdk::caller();
    let now = ic_cdk::api::time();

    // Get or create conversation state
    let mut state = CONVERSATIONS.with(|c| {
        c.borrow()
            .get(&caller)
            .cloned()
            .unwrap_or_else(|| {
                let character = CHARACTER.with(|ch| ch.borrow().clone().unwrap_or_else(default_character));
                ConversationState {
                    messages: vec![Message {
                        role: "system".to_string(),
                        content: character.system_prompt.clone(),
                    }],
                    character,
                    created_at: now,
                    updated_at: now,
                }
            })
    });

    // Add user message
    state.messages.push(Message {
        role: "user".to_string(),
        content: user_message,
    });

    // Trim conversation if too long
    let max_len = CONFIG.with(|cfg| {
        cfg.borrow()
            .as_ref()
            .map(|c| c.max_conversation_length)
            .unwrap_or(50)
    });

    if state.messages.len() > max_len {
        // Keep system message and recent messages
        let system_msg = state.messages[0].clone();
        let recent: Vec<Message> = state.messages.iter().skip(state.messages.len() - max_len + 1).cloned().collect();
        state.messages = vec![system_msg];
        state.messages.extend(recent);
    }

    // Generate response
    let response = generate_response(&state).await?;

    // Add assistant response
    state.messages.push(Message {
        role: "assistant".to_string(),
        content: response.clone(),
    });

    state.updated_at = now;

    // Save conversation state
    CONVERSATIONS.with(|c| {
        c.borrow_mut().insert(caller, state);
    });

    Ok(response)
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
    use ic_llm::{ChatMessage, Model, AssistantMessage};

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

    // Call IC LLM Canister with Llama 3.1 8B
    let response = ic_llm::chat(Model::Llama3_1_8B)
        .with_messages(messages)
        .send()
        .await;

    // Extract text from response
    response.message.content.ok_or_else(|| "No response content from LLM".to_string())
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

#[update]
fn clear_conversation() {
    let caller = ic_cdk::caller();
    CONVERSATIONS.with(|c| {
        c.borrow_mut().remove(&caller);
    });
}

#[query]
fn get_conversation_count() -> u64 {
    CONVERSATIONS.with(|c| c.borrow().len() as u64)
}

// ========== Health Check ==========

#[query]
fn health() -> String {
    let has_config = CONFIG.with(|c| c.borrow().is_some());
    let has_character = CHARACTER.with(|c| c.borrow().is_some());
    let conversation_count = CONVERSATIONS.with(|c| c.borrow().len());
    if has_config && has_character {
        format!("ok | v{} | {} conversations", VERSION, conversation_count)
    } else {
        format!("degraded | config={} character={}", has_config, has_character)
    }
}

const VERSION: &str = "0.5.0";

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
        conversations: CONVERSATIONS.with(|c| c.borrow().len() as u64),
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
}

// Candid export
ic_cdk::export_candid!();
