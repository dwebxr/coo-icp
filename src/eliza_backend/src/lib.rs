use candid::{CandidType, Deserialize, Principal};
use ic_cdk::api::management_canister::http_request::{
    http_request, CanisterHttpRequestArgument, HttpHeader, HttpMethod, HttpResponse, TransformArgs,
    TransformContext, TransformFunc,
};
use ic_cdk_macros::{init, post_upgrade, query, update};
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
            // Default to Fallback for local dev; change to OnChain for mainnet
            llm_provider: LlmProvider::Fallback,
            max_conversation_length: 50,
            admin: caller,
        });
    });
}

#[post_upgrade]
fn post_upgrade() {
    // Restore default character if not set
    CHARACTER.with(|c| {
        if c.borrow().is_none() {
            *c.borrow_mut() = Some(default_character());
        }
    });

    // Restore config if not set (preserves admin from init if this is first upgrade)
    CONFIG.with(|cfg| {
        if cfg.borrow().is_none() {
            // Note: After upgrade, we can't recover the original admin
            // Consider using stable memory for production
            *cfg.borrow_mut() = Some(Config {
                llm_provider: LlmProvider::Fallback,
                max_conversation_length: 50,
                admin: ic_cdk::caller(), // Will be the upgrade caller
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
    "Eliza is running on-chain!".to_string()
}

#[query]
fn version() -> String {
    "0.2.0-llm".to_string()
}

// Candid export
ic_cdk::export_candid!();
