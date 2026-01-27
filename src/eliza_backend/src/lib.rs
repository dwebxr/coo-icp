use candid::{CandidType, Deserialize, Principal};
use ic_cdk::api::management_canister::http_request::{
    http_request, CanisterHttpRequestArgument, HttpHeader, HttpMethod, HttpResponse, TransformArgs,
    TransformContext, TransformFunc,
};
use ic_cdk_macros::{init, post_upgrade, query, update};
use ic_cdk_timers::TimerId;
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::time::Duration;

// Crypto imports for OAuth 1.0a
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Sha256, Digest};

// ICP Ledger constants
const ICP_LEDGER_CANISTER_ID: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";

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

// ========== Social Integration Types ==========

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, PartialEq)]
pub enum SocialPlatform {
    Twitter,
    Discord,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct TwitterCredentials {
    pub api_key: Vec<u8>,              // Consumer Key
    pub api_secret: Vec<u8>,           // Consumer Secret
    pub access_token: Vec<u8>,         // Access Token
    pub access_token_secret: Vec<u8>,  // Access Token Secret
    pub user_id: Option<String>,       // Twitter User ID (cached)
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct DiscordConfig {
    pub bot_token: Vec<u8>,           // Discord Bot Token
    pub webhook_url: Option<String>,  // Webhook URL for outgoing messages
    pub channel_ids: Vec<String>,     // Channels to monitor
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct SocialIntegrationConfig {
    pub twitter: Option<TwitterCredentials>,
    pub discord: Option<DiscordConfig>,
    pub enabled_platforms: Vec<SocialPlatform>,
    pub auto_reply: bool,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub enum PostStatus {
    Pending,
    Processing,
    Completed,
    Failed(String),
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct PostMetadata {
    pub reply_to_id: Option<String>,
    pub discord_channel_id: Option<String>,
    pub result_id: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct ScheduledPost {
    pub id: u64,
    pub platform: SocialPlatform,
    pub content: String,
    pub scheduled_time: u64,
    pub status: PostStatus,
    pub retry_count: u32,
    pub created_at: u64,
    pub metadata: Option<PostMetadata>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct IncomingMessage {
    pub id: String,
    pub platform: SocialPlatform,
    pub author_id: String,
    pub author_name: String,
    pub content: String,
    pub timestamp: u64,
    pub processed: bool,
    pub replied: bool,
    pub conversation_id: Option<String>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default)]
pub struct PollingState {
    pub twitter_last_mention_id: Option<String>,
    pub twitter_last_poll_time: u64,
    pub discord_last_message_ids: HashMap<String, String>,
    pub discord_last_poll_time: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct SocialStatus {
    pub twitter_configured: bool,
    pub discord_configured: bool,
    pub enabled_platforms: Vec<SocialPlatform>,
    pub polling_active: bool,
    pub last_twitter_poll: u64,
    pub last_discord_poll: u64,
    pub pending_posts: u32,
    pub unprocessed_messages: u32,
}

#[derive(Default)]
struct RateLimiter {
    twitter_calls: u32,
    discord_calls: u32,
    last_reset: u64,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct AutoPostConfig {
    pub enabled: bool,
    pub interval_seconds: u64,
    pub topics: Vec<String>,
    pub platform: SocialPlatform,
    pub last_post_time: u64,
}

// ========== Wallet Data Structures ==========

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct WalletInfo {
    pub icp_address: String,           // Account Identifier (hex)
    pub principal_id: String,          // Canister Principal
    pub icp_balance: u64,              // Balance in e8s (1 ICP = 100_000_000 e8s)
    pub last_balance_update: u64,      // Timestamp of last balance check
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct TransactionRecord {
    pub id: u64,
    pub tx_type: TransactionType,
    pub amount: u64,                   // in e8s
    pub to: Option<String>,            // Recipient address (for transfers)
    pub from: Option<String>,          // Sender address (for receives)
    pub memo: u64,
    pub timestamp: u64,
    pub status: TransactionStatus,
    pub block_height: Option<u64>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub enum TransactionType {
    Send,
    Receive,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub enum TransactionStatus {
    Pending,
    Completed,
    Failed(String),
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default)]
pub struct WalletState {
    pub transaction_history: Vec<TransactionRecord>,
    pub tx_counter: u64,
}

// ========== EVM Wallet Data Structures (Chain-Key ECDSA) ==========

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct EvmWalletInfo {
    pub address: String,              // Ethereum address (0x...)
    pub chain_id: u64,                // EVM chain ID (1=Ethereum, 8453=Base, 137=Polygon)
    pub chain_name: String,           // Human readable chain name
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct EvmTransactionRecord {
    pub id: u64,
    pub chain_id: u64,
    pub tx_hash: Option<String>,
    pub to: String,
    pub value_wei: String,            // Value in wei (as string for large numbers)
    pub data: Option<String>,         // Contract call data (hex)
    pub timestamp: u64,
    pub status: EvmTransactionStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub enum EvmTransactionStatus {
    Pending,
    Submitted(String),                // tx_hash
    Confirmed(u64),                   // block_number
    Failed(String),                   // error message
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct EvmChainConfig {
    pub chain_id: u64,
    pub chain_name: String,
    pub rpc_url: String,
    pub native_symbol: String,        // ETH, MATIC, etc.
    pub decimals: u8,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default)]
pub struct EvmWalletState {
    pub cached_address: Option<String>,
    pub transaction_history: Vec<EvmTransactionRecord>,
    pub tx_counter: u64,
    pub configured_chains: Vec<EvmChainConfig>,
}

// ========== State Management ==========

thread_local! {
    static CONVERSATIONS: RefCell<HashMap<Principal, ConversationState>> = RefCell::new(HashMap::new());
    static ENCRYPTED_API_KEY: RefCell<Option<Vec<u8>>> = RefCell::new(None);
    static CHARACTER: RefCell<Option<Character>> = RefCell::new(None);
    static CONFIG: RefCell<Option<Config>> = RefCell::new(None);

    // Social Integration State
    static SOCIAL_CONFIG: RefCell<Option<SocialIntegrationConfig>> = RefCell::new(None);
    static SCHEDULED_POSTS: RefCell<Vec<ScheduledPost>> = RefCell::new(Vec::new());
    static INCOMING_MESSAGES: RefCell<Vec<IncomingMessage>> = RefCell::new(Vec::new());
    static POLLING_STATE: RefCell<PollingState> = RefCell::new(PollingState::default());
    static POST_COUNTER: RefCell<u64> = RefCell::new(0);
    static TIMER_ID: RefCell<Option<TimerId>> = RefCell::new(None);
    static AUTO_POST_TIMER_ID: RefCell<Option<TimerId>> = RefCell::new(None);
    static AUTO_POST_CONFIG: RefCell<Option<AutoPostConfig>> = RefCell::new(None);
    static RATE_LIMITER: RefCell<RateLimiter> = RefCell::new(RateLimiter::default());

    // Wallet State (ICP)
    static WALLET_STATE: RefCell<WalletState> = RefCell::new(WalletState {
        transaction_history: Vec::new(),
        tx_counter: 0,
    });

    // EVM Wallet State (Chain-Key ECDSA)
    static EVM_WALLET_STATE: RefCell<EvmWalletState> = RefCell::new(EvmWalletState {
        cached_address: None,
        transaction_history: Vec::new(),
        tx_counter: 0,
        configured_chains: Vec::new(),
    });
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
    "0.4.0-wallet".to_string()
}

// ========== Social Integration: OAuth 1.0a ==========

type HmacSha1 = Hmac<Sha1>;

/// URL percent encoding for OAuth
fn percent_encode(input: &str) -> String {
    let mut result = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                result.push(byte as char);
            }
            _ => {
                result.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    result
}

/// Generate OAuth 1.0a Authorization header for Twitter API
fn generate_twitter_oauth_header(
    method: &str,
    base_url: &str,
    api_key: &str,
    api_secret: &str,
    access_token: &str,
    access_token_secret: &str,
    additional_params: &[(&str, &str)],
) -> Result<String, String> {
    let timestamp = (ic_cdk::api::time() / 1_000_000_000).to_string();

    // Deterministic nonce from timestamp + url hash for ICP consensus
    let nonce_input = format!("{}{}{}", timestamp, base_url, method);
    let mut hasher = Sha256::new();
    hasher.update(nonce_input.as_bytes());
    let hash_result = hasher.finalize();
    let nonce = hex::encode(&hash_result[..16]);

    // OAuth parameters
    let oauth_params: Vec<(&str, String)> = vec![
        ("oauth_consumer_key", api_key.to_string()),
        ("oauth_nonce", nonce.clone()),
        ("oauth_signature_method", "HMAC-SHA1".to_string()),
        ("oauth_timestamp", timestamp.clone()),
        ("oauth_token", access_token.to_string()),
        ("oauth_version", "1.0".to_string()),
    ];

    // Combine all parameters for signature
    let mut all_params: Vec<(String, String)> = oauth_params
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    for (k, v) in additional_params {
        all_params.push((k.to_string(), v.to_string()));
    }
    all_params.sort_by(|a, b| a.0.cmp(&b.0));

    // Create parameter string
    let param_string: String = all_params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    // Create signature base string
    let signature_base = format!(
        "{}&{}&{}",
        method.to_uppercase(),
        percent_encode(base_url),
        percent_encode(&param_string)
    );

    // Create signing key
    let signing_key = format!(
        "{}&{}",
        percent_encode(api_secret),
        percent_encode(access_token_secret)
    );

    // HMAC-SHA1 signature
    let mut mac = HmacSha1::new_from_slice(signing_key.as_bytes())
        .map_err(|_| "HMAC error")?;
    mac.update(signature_base.as_bytes());
    let signature = mac.finalize().into_bytes();
    let signature_b64 = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, &signature);

    // Build Authorization header
    let auth_header = format!(
        r#"OAuth oauth_consumer_key="{}", oauth_nonce="{}", oauth_signature="{}", oauth_signature_method="HMAC-SHA1", oauth_timestamp="{}", oauth_token="{}", oauth_version="1.0""#,
        percent_encode(api_key),
        percent_encode(&nonce),
        percent_encode(&signature_b64),
        percent_encode(&timestamp),
        percent_encode(access_token)
    );

    Ok(auth_header)
}

// ========== Social Integration: Helper Functions ==========

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

fn decrypt_bytes(encrypted: &[u8]) -> Result<String, String> {
    // In production, integrate with vetKeys
    // For now, stored directly (NOT secure for production)
    String::from_utf8(encrypted.to_vec())
        .map_err(|e| format!("Decryption error: {}", e))
}

fn get_twitter_credentials() -> Result<TwitterCredentials, String> {
    SOCIAL_CONFIG.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(|cfg| cfg.twitter.clone())
            .ok_or_else(|| "Twitter credentials not configured".to_string())
    })
}

fn get_discord_config() -> Result<DiscordConfig, String> {
    SOCIAL_CONFIG.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(|cfg| cfg.discord.clone())
            .ok_or_else(|| "Discord config not set".to_string())
    })
}

fn check_rate_limit(platform: &SocialPlatform) -> Result<(), String> {
    RATE_LIMITER.with(|r| {
        let mut limiter = r.borrow_mut();
        let now = ic_cdk::api::time();

        // Reset counters every hour (3600 seconds in nanoseconds)
        if now - limiter.last_reset > 3_600_000_000_000 {
            limiter.twitter_calls = 0;
            limiter.discord_calls = 0;
            limiter.last_reset = now;
        }

        match platform {
            SocialPlatform::Twitter => {
                if limiter.twitter_calls >= 100 {
                    return Err("Twitter rate limit exceeded (100/hour)".to_string());
                }
                limiter.twitter_calls += 1;
            }
            SocialPlatform::Discord => {
                if limiter.discord_calls >= 500 {
                    return Err("Discord rate limit exceeded (500/hour)".to_string());
                }
                limiter.discord_calls += 1;
            }
        }
        Ok(())
    })
}

// ========== Social Integration: Twitter API ==========

/// Post a tweet using Twitter API v2
async fn post_tweet(content: &str, reply_to: Option<&str>) -> Result<String, String> {
    check_rate_limit(&SocialPlatform::Twitter)?;
    let creds = get_twitter_credentials()?;

    let url = "https://api.twitter.com/2/tweets";

    // Build request body
    let mut body_json = serde_json::json!({
        "text": content
    });

    if let Some(reply_id) = reply_to {
        body_json["reply"] = serde_json::json!({
            "in_reply_to_tweet_id": reply_id
        });
    }

    let body = body_json.to_string();

    let oauth_header = generate_twitter_oauth_header(
        "POST",
        url,
        &decrypt_bytes(&creds.api_key)?,
        &decrypt_bytes(&creds.api_secret)?,
        &decrypt_bytes(&creds.access_token)?,
        &decrypt_bytes(&creds.access_token_secret)?,
        &[],
    )?;

    let request = CanisterHttpRequestArgument {
        url: url.to_string(),
        max_response_bytes: Some(5_000),
        method: HttpMethod::POST,
        headers: vec![
            HttpHeader {
                name: "Authorization".to_string(),
                value: oauth_header,
            },
            HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            },
        ],
        body: Some(body.into_bytes()),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_social_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("JSON error: {} - Body: {}", e, body))?;

            if let Some(error) = json.get("errors") {
                return Err(format!("Twitter API error: {}", error));
            }

            json["data"]["id"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("Tweet ID not found in response: {}", body))
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

/// Fetch Twitter user ID for authenticated user
async fn get_twitter_user_id() -> Result<String, String> {
    // Check if cached
    if let Some(user_id) = SOCIAL_CONFIG.with(|c| {
        c.borrow()
            .as_ref()
            .and_then(|cfg| cfg.twitter.as_ref())
            .and_then(|t| t.user_id.clone())
    }) {
        return Ok(user_id);
    }

    check_rate_limit(&SocialPlatform::Twitter)?;
    let creds = get_twitter_credentials()?;

    let url = "https://api.twitter.com/2/users/me";

    let oauth_header = generate_twitter_oauth_header(
        "GET",
        url,
        &decrypt_bytes(&creds.api_key)?,
        &decrypt_bytes(&creds.api_secret)?,
        &decrypt_bytes(&creds.access_token)?,
        &decrypt_bytes(&creds.access_token_secret)?,
        &[],
    )?;

    let request = CanisterHttpRequestArgument {
        url: url.to_string(),
        max_response_bytes: Some(2_000),
        method: HttpMethod::GET,
        headers: vec![
            HttpHeader {
                name: "Authorization".to_string(),
                value: oauth_header,
            },
        ],
        body: None,
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_social_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("JSON error: {}", e))?;

            let user_id = json["data"]["id"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "User ID not found".to_string())?;

            // Cache the user ID
            SOCIAL_CONFIG.with(|c| {
                if let Some(ref mut cfg) = *c.borrow_mut() {
                    if let Some(ref mut twitter) = cfg.twitter {
                        twitter.user_id = Some(user_id.clone());
                    }
                }
            });

            Ok(user_id)
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

/// Fetch recent mentions from Twitter
async fn fetch_twitter_mentions(since_id: Option<&str>) -> Result<Vec<IncomingMessage>, String> {
    check_rate_limit(&SocialPlatform::Twitter)?;
    let creds = get_twitter_credentials()?;

    let user_id = get_twitter_user_id().await?;

    let base_url = format!("https://api.twitter.com/2/users/{}/mentions", user_id);

    let mut params: Vec<(&str, &str)> = vec![
        ("tweet.fields", "author_id,conversation_id,created_at"),
        ("expansions", "author_id"),
        ("user.fields", "username"),
        ("max_results", "10"),
    ];

    let since_id_owned: String;
    if let Some(id) = since_id {
        since_id_owned = id.to_string();
        params.push(("since_id", &since_id_owned));
    }

    let oauth_header = generate_twitter_oauth_header(
        "GET",
        &base_url,
        &decrypt_bytes(&creds.api_key)?,
        &decrypt_bytes(&creds.api_secret)?,
        &decrypt_bytes(&creds.access_token)?,
        &decrypt_bytes(&creds.access_token_secret)?,
        &params,
    )?;

    // Build URL with query params
    let query_string: String = params
        .iter()
        .map(|(k, v)| format!("{}={}", percent_encode(k), percent_encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let full_url = format!("{}?{}", base_url, query_string);

    let request = CanisterHttpRequestArgument {
        url: full_url,
        max_response_bytes: Some(50_000),
        method: HttpMethod::GET,
        headers: vec![
            HttpHeader {
                name: "Authorization".to_string(),
                value: oauth_header,
            },
        ],
        body: None,
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_social_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            parse_twitter_mentions_response(&body)
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

fn parse_twitter_mentions_response(body: &str) -> Result<Vec<IncomingMessage>, String> {
    let json: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| format!("JSON error: {}", e))?;

    let mut messages = Vec::new();

    // Build user lookup map
    let mut user_map: HashMap<String, String> = HashMap::new();
    if let Some(users) = json["includes"]["users"].as_array() {
        for user in users {
            if let (Some(id), Some(username)) = (
                user["id"].as_str(),
                user["username"].as_str()
            ) {
                user_map.insert(id.to_string(), username.to_string());
            }
        }
    }

    if let Some(data) = json["data"].as_array() {
        for tweet in data {
            let author_id = tweet["author_id"].as_str().unwrap_or("unknown").to_string();
            let author_name = user_map.get(&author_id)
                .cloned()
                .unwrap_or_else(|| author_id.clone());

            messages.push(IncomingMessage {
                id: tweet["id"].as_str().unwrap_or("").to_string(),
                platform: SocialPlatform::Twitter,
                author_id,
                author_name,
                content: tweet["text"].as_str().unwrap_or("").to_string(),
                timestamp: ic_cdk::api::time(),
                processed: false,
                replied: false,
                conversation_id: tweet["conversation_id"].as_str().map(|s| s.to_string()),
            });
        }
    }

    Ok(messages)
}

// ========== Social Integration: Discord API ==========

/// Send message via Discord webhook
async fn send_discord_webhook(webhook_url: &str, content: &str) -> Result<(), String> {
    check_rate_limit(&SocialPlatform::Discord)?;

    let body = serde_json::json!({
        "content": content
    }).to_string();

    let request = CanisterHttpRequestArgument {
        url: webhook_url.to_string(),
        max_response_bytes: Some(10_000),
        method: HttpMethod::POST,
        headers: vec![
            HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            },
        ],
        body: Some(body.into_bytes()),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_social_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    match http_request(request, cycles).await {
        Ok((response,)) => {
            if response.status >= candid::Nat::from(200u32) && response.status < candid::Nat::from(300u32) {
                Ok(())
            } else {
                let body = String::from_utf8_lossy(&response.body);
                Err(format!("Discord webhook failed: {} - {}", response.status, body))
            }
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

/// Send message to Discord channel via Bot API
async fn send_discord_message(channel_id: &str, content: &str) -> Result<String, String> {
    check_rate_limit(&SocialPlatform::Discord)?;
    let config = get_discord_config()?;
    let bot_token = decrypt_bytes(&config.bot_token)?;

    let url = format!("https://discord.com/api/v10/channels/{}/messages", channel_id);

    let body = serde_json::json!({
        "content": content
    }).to_string();

    let request = CanisterHttpRequestArgument {
        url,
        max_response_bytes: Some(5_000),
        method: HttpMethod::POST,
        headers: vec![
            HttpHeader {
                name: "Authorization".to_string(),
                value: format!("Bot {}", bot_token),
            },
            HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            },
        ],
        body: Some(body.into_bytes()),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_social_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("JSON error: {}", e))?;

            json["id"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("Message ID not found: {}", body))
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

/// Fetch messages from Discord channel
async fn fetch_discord_messages(
    channel_id: &str,
    after_id: Option<&str>
) -> Result<Vec<IncomingMessage>, String> {
    check_rate_limit(&SocialPlatform::Discord)?;
    let config = get_discord_config()?;
    let bot_token = decrypt_bytes(&config.bot_token)?;

    let mut url = format!(
        "https://discord.com/api/v10/channels/{}/messages?limit=20",
        channel_id
    );

    if let Some(id) = after_id {
        url.push_str(&format!("&after={}", id));
    }

    let request = CanisterHttpRequestArgument {
        url,
        max_response_bytes: Some(100_000),
        method: HttpMethod::GET,
        headers: vec![
            HttpHeader {
                name: "Authorization".to_string(),
                value: format!("Bot {}", bot_token),
            },
        ],
        body: None,
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_social_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            parse_discord_messages_response(&body, channel_id)
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

fn parse_discord_messages_response(body: &str, channel_id: &str) -> Result<Vec<IncomingMessage>, String> {
    let json: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| format!("JSON error: {}", e))?;

    let mut messages = Vec::new();

    if let Some(data) = json.as_array() {
        for msg in data {
            // Skip bot messages
            if msg["author"]["bot"].as_bool().unwrap_or(false) {
                continue;
            }

            let msg_id = msg["id"].as_str().unwrap_or("").to_string();

            messages.push(IncomingMessage {
                id: format!("{}:{}", channel_id, msg_id),
                platform: SocialPlatform::Discord,
                author_id: msg["author"]["id"].as_str().unwrap_or("").to_string(),
                author_name: msg["author"]["username"].as_str().unwrap_or("").to_string(),
                content: msg["content"].as_str().unwrap_or("").to_string(),
                timestamp: ic_cdk::api::time(),
                processed: false,
                replied: false,
                conversation_id: Some(channel_id.to_string()),
            });
        }
    }

    // Discord returns newest first, reverse for chronological
    messages.reverse();
    Ok(messages)
}

/// Transform function for social API responses
#[query]
fn transform_social_response(raw: TransformArgs) -> HttpResponse {
    HttpResponse {
        status: raw.response.status,
        body: raw.response.body,
        headers: vec![],
    }
}

// ========== Social Integration: Timer & Scheduler ==========

/// Start social media polling timer
#[update]
fn start_social_polling(interval_seconds: u64) -> Result<(), String> {
    require_admin()?;

    // Stop existing timer
    stop_social_polling_internal();

    let interval = Duration::from_secs(interval_seconds);

    let timer_id = ic_cdk_timers::set_timer_interval(interval, || {
        ic_cdk::spawn(async {
            if let Err(e) = poll_and_process().await {
                ic_cdk::println!("Polling error: {}", e);
            }
        });
    });

    TIMER_ID.with(|t| {
        *t.borrow_mut() = Some(timer_id);
    });

    Ok(())
}

#[update]
fn stop_social_polling() -> Result<(), String> {
    require_admin()?;
    stop_social_polling_internal();
    Ok(())
}

fn stop_social_polling_internal() {
    TIMER_ID.with(|t| {
        if let Some(timer_id) = t.borrow_mut().take() {
            ic_cdk_timers::clear_timer(timer_id);
        }
    });
}

// ========== Autonomous Posting ==========

/// Start autonomous posting with AI-generated content
#[update]
fn start_auto_posting(interval_seconds: u64, topics: Vec<String>) -> Result<(), String> {
    require_admin()?;

    // Validate interval (minimum 1 hour for Free tier rate limits)
    if interval_seconds < 3600 {
        return Err("Minimum interval is 3600 seconds (1 hour) to respect rate limits".to_string());
    }

    // Stop existing auto-post timer
    stop_auto_posting_internal();

    // Save config
    AUTO_POST_CONFIG.with(|c| {
        *c.borrow_mut() = Some(AutoPostConfig {
            enabled: true,
            interval_seconds,
            topics: if topics.is_empty() {
                vec![
                    "Internet Computer blockchain".to_string(),
                    "decentralized AI".to_string(),
                    "Web3 technology".to_string(),
                    "on-chain AI agents".to_string(),
                ]
            } else {
                topics
            },
            platform: SocialPlatform::Twitter,
            last_post_time: 0,
        });
    });

    let interval = Duration::from_secs(interval_seconds);

    let timer_id = ic_cdk_timers::set_timer_interval(interval, || {
        ic_cdk::spawn(async {
            if let Err(e) = generate_and_post().await {
                ic_cdk::println!("Auto-post error: {}", e);
            }
        });
    });

    AUTO_POST_TIMER_ID.with(|t| {
        *t.borrow_mut() = Some(timer_id);
    });

    // Also trigger first post immediately
    ic_cdk::spawn(async {
        if let Err(e) = generate_and_post().await {
            ic_cdk::println!("Initial auto-post error: {}", e);
        }
    });

    Ok(())
}

#[update]
fn stop_auto_posting() -> Result<(), String> {
    require_admin()?;
    stop_auto_posting_internal();

    AUTO_POST_CONFIG.with(|c| {
        if let Some(ref mut config) = *c.borrow_mut() {
            config.enabled = false;
        }
    });

    Ok(())
}

fn stop_auto_posting_internal() {
    AUTO_POST_TIMER_ID.with(|t| {
        if let Some(timer_id) = t.borrow_mut().take() {
            ic_cdk_timers::clear_timer(timer_id);
        }
    });
}

#[query]
fn get_auto_post_config() -> Option<AutoPostConfig> {
    AUTO_POST_CONFIG.with(|c| c.borrow().clone())
}

/// Generate AI content and post to Twitter
async fn generate_and_post() -> Result<String, String> {
    let config = AUTO_POST_CONFIG.with(|c| c.borrow().clone())
        .ok_or_else(|| "Auto-post not configured".to_string())?;

    if !config.enabled {
        return Err("Auto-posting is disabled".to_string());
    }

    // Pick a random topic
    let now = ic_cdk::api::time();
    let topic_index = (now as usize) % config.topics.len();
    let topic = &config.topics[topic_index];

    // Generate tweet content using IC LLM
    let prompt = format!(
        r#"You are Coo, a friendly AI agent running fully on-chain on the Internet Computer.
Generate a single engaging tweet (max 280 characters) about: {}

Rules:
- Be informative and friendly
- Include relevant hashtags (1-2 max)
- Don't use emojis excessively
- Make it feel natural, not promotional
- Vary the style (question, fact, tip, thought)

Output only the tweet text, nothing else."#,
        topic
    );

    let tweet_content = generate_llm_response(&prompt).await?;

    // Trim to 280 characters if needed
    let tweet = if tweet_content.len() > 280 {
        tweet_content.chars().take(277).collect::<String>() + "..."
    } else {
        tweet_content.trim().to_string()
    };

    // Post to Twitter
    let result = post_tweet(&tweet, None).await?;

    // Update last post time
    AUTO_POST_CONFIG.with(|c| {
        if let Some(ref mut cfg) = *c.borrow_mut() {
            cfg.last_post_time = now;
        }
    });

    Ok(result)
}

/// Generate LLM response (internal helper)
async fn generate_llm_response(prompt: &str) -> Result<String, String> {
    use ic_llm::{ChatMessage, Model};

    let provider = CONFIG.with(|cfg| {
        cfg.borrow()
            .as_ref()
            .map(|c| c.llm_provider.clone())
            .unwrap_or(LlmProvider::Fallback)
    });

    match provider {
        LlmProvider::OnChain => {
            let messages = vec![
                ChatMessage::User {
                    content: prompt.to_string(),
                },
            ];

            let response = ic_llm::chat(Model::Llama3_1_8B)
                .with_messages(messages)
                .send()
                .await;

            response.message.content.ok_or_else(|| "No response content from LLM".to_string())
        }
        _ => Err("Auto-posting requires OnChain LLM provider".to_string()),
    }
}

/// Manually trigger an auto-generated post
#[update]
async fn trigger_auto_post() -> Result<String, String> {
    require_admin()?;
    generate_and_post().await
}

/// Main polling and processing function
async fn poll_and_process() -> Result<(), String> {
    // 1. Process scheduled posts
    process_scheduled_posts().await?;

    // 2. Poll for new messages
    poll_incoming_messages().await?;

    // 3. Process and respond to messages (if auto_reply enabled)
    let auto_reply = SOCIAL_CONFIG.with(|c| {
        c.borrow().as_ref().map(|cfg| cfg.auto_reply).unwrap_or(false)
    });

    if auto_reply {
        process_incoming_messages().await?;
    }

    Ok(())
}

/// Process due scheduled posts
async fn process_scheduled_posts() -> Result<(), String> {
    let now = ic_cdk::api::time();

    let due_posts: Vec<ScheduledPost> = SCHEDULED_POSTS.with(|posts| {
        posts.borrow()
            .iter()
            .filter(|p| matches!(p.status, PostStatus::Pending) && p.scheduled_time <= now)
            .cloned()
            .collect()
    });

    for post in due_posts {
        update_post_status(post.id, PostStatus::Processing);

        let result = match post.platform {
            SocialPlatform::Twitter => {
                let reply_to = post.metadata.as_ref()
                    .and_then(|m| m.reply_to_id.as_deref());
                post_tweet(&post.content, reply_to).await
            }
            SocialPlatform::Discord => {
                let channel_id = post.metadata.as_ref()
                    .and_then(|m| m.discord_channel_id.as_deref());

                if let Some(ch_id) = channel_id {
                    send_discord_message(ch_id, &post.content).await
                } else {
                    // Try webhook
                    let webhook = SOCIAL_CONFIG.with(|c| {
                        c.borrow()
                            .as_ref()
                            .and_then(|cfg| cfg.discord.as_ref())
                            .and_then(|d| d.webhook_url.clone())
                    });

                    if let Some(url) = webhook {
                        send_discord_webhook(&url, &post.content).await?;
                        Ok("webhook".to_string())
                    } else {
                        Err("No channel ID or webhook configured".to_string())
                    }
                }
            }
        };

        match result {
            Ok(result_id) => {
                update_post_status_with_result(post.id, PostStatus::Completed, result_id);
            }
            Err(e) => {
                if post.retry_count < 3 {
                    increment_retry_count(post.id);
                    update_post_status(post.id, PostStatus::Pending);
                } else {
                    update_post_status(post.id, PostStatus::Failed(e));
                }
            }
        }
    }

    Ok(())
}

fn update_post_status(post_id: u64, status: PostStatus) {
    SCHEDULED_POSTS.with(|p| {
        if let Some(post) = p.borrow_mut().iter_mut().find(|p| p.id == post_id) {
            post.status = status;
        }
    });
}

fn update_post_status_with_result(post_id: u64, status: PostStatus, result_id: String) {
    SCHEDULED_POSTS.with(|p| {
        if let Some(post) = p.borrow_mut().iter_mut().find(|p| p.id == post_id) {
            post.status = status;
            if let Some(ref mut meta) = post.metadata {
                meta.result_id = Some(result_id);
            } else {
                post.metadata = Some(PostMetadata {
                    reply_to_id: None,
                    discord_channel_id: None,
                    result_id: Some(result_id),
                });
            }
        }
    });
}

fn increment_retry_count(post_id: u64) {
    SCHEDULED_POSTS.with(|p| {
        if let Some(post) = p.borrow_mut().iter_mut().find(|p| p.id == post_id) {
            post.retry_count += 1;
        }
    });
}

/// Poll for incoming messages
async fn poll_incoming_messages() -> Result<(), String> {
    let config = SOCIAL_CONFIG.with(|c| c.borrow().clone());
    let config = match config {
        Some(c) => c,
        None => return Ok(()), // No config, skip
    };

    // Poll Twitter
    if config.enabled_platforms.contains(&SocialPlatform::Twitter) && config.twitter.is_some() {
        let since_id = POLLING_STATE.with(|s| s.borrow().twitter_last_mention_id.clone());

        match fetch_twitter_mentions(since_id.as_deref()).await {
            Ok(mentions) => {
                if let Some(latest) = mentions.first() {
                    POLLING_STATE.with(|s| {
                        let mut state = s.borrow_mut();
                        state.twitter_last_mention_id = Some(latest.id.clone());
                        state.twitter_last_poll_time = ic_cdk::api::time();
                    });
                }
                store_incoming_messages(mentions);
            }
            Err(e) => ic_cdk::println!("Twitter poll error: {}", e),
        }
    }

    // Poll Discord
    if config.enabled_platforms.contains(&SocialPlatform::Discord) {
        if let Some(ref discord_config) = config.discord {
            for channel_id in &discord_config.channel_ids {
                let after_id = POLLING_STATE.with(|s| {
                    s.borrow().discord_last_message_ids.get(channel_id).cloned()
                });

                match fetch_discord_messages(channel_id, after_id.as_deref()).await {
                    Ok(messages) => {
                        if let Some(latest) = messages.last() {
                            let msg_id = latest.id.split(':').last()
                                .unwrap_or(&latest.id).to_string();

                            POLLING_STATE.with(|s| {
                                let mut state = s.borrow_mut();
                                state.discord_last_message_ids.insert(channel_id.clone(), msg_id);
                                state.discord_last_poll_time = ic_cdk::api::time();
                            });
                        }
                        store_incoming_messages(messages);
                    }
                    Err(e) => ic_cdk::println!("Discord poll error for {}: {}", channel_id, e),
                }
            }
        }
    }

    Ok(())
}

fn store_incoming_messages(messages: Vec<IncomingMessage>) {
    INCOMING_MESSAGES.with(|m| {
        let mut stored = m.borrow_mut();
        for msg in messages {
            if !stored.iter().any(|existing| existing.id == msg.id) {
                stored.push(msg);
            }
        }
        // Keep only last 500 messages
        let len = stored.len();
        if len > 500 {
            stored.drain(0..len - 500);
        }
    });
}

/// Process and respond to incoming messages
async fn process_incoming_messages() -> Result<(), String> {
    let unprocessed: Vec<IncomingMessage> = INCOMING_MESSAGES.with(|m| {
        m.borrow()
            .iter()
            .filter(|msg| !msg.processed && !msg.replied)
            .take(3) // Process max 3 per cycle
            .cloned()
            .collect()
    });

    for msg in unprocessed {
        mark_message_processed(&msg.id);

        if !should_respond_to(&msg) {
            continue;
        }

        match generate_social_response(&msg).await {
            Ok(reply_text) => {
                let reply_content = match msg.platform {
                    SocialPlatform::Twitter => format!("@{} {}", msg.author_name, truncate_text(&reply_text, 260)),
                    SocialPlatform::Discord => format!("<@{}> {}", msg.author_id, reply_text),
                };

                let metadata = match msg.platform {
                    SocialPlatform::Twitter => Some(PostMetadata {
                        reply_to_id: Some(msg.id.clone()),
                        discord_channel_id: None,
                        result_id: None,
                    }),
                    SocialPlatform::Discord => Some(PostMetadata {
                        reply_to_id: None,
                        discord_channel_id: msg.conversation_id.clone(),
                        result_id: None,
                    }),
                };

                let _ = schedule_post_internal(
                    msg.platform.clone(),
                    reply_content,
                    ic_cdk::api::time(),
                    metadata,
                );

                mark_message_replied(&msg.id);
            }
            Err(e) => {
                ic_cdk::println!("Failed to generate response: {}", e);
            }
        }
    }

    Ok(())
}

fn truncate_text(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...", &text[..max_len - 3])
    }
}

fn mark_message_processed(id: &str) {
    INCOMING_MESSAGES.with(|m| {
        if let Some(msg) = m.borrow_mut().iter_mut().find(|m| m.id == id) {
            msg.processed = true;
        }
    });
}

fn mark_message_replied(id: &str) {
    INCOMING_MESSAGES.with(|m| {
        if let Some(msg) = m.borrow_mut().iter_mut().find(|m| m.id == id) {
            msg.replied = true;
        }
    });
}

fn should_respond_to(msg: &IncomingMessage) -> bool {
    let character_name = CHARACTER.with(|c| {
        c.borrow().as_ref().map(|ch| ch.name.to_lowercase()).unwrap_or_default()
    });

    let content_lower = msg.content.to_lowercase();

    content_lower.contains(&character_name) ||
    content_lower.contains("@coo") ||
    content_lower.contains("?")
}

/// Generate AI response for social message
async fn generate_social_response(msg: &IncomingMessage) -> Result<String, String> {
    let character = CHARACTER.with(|c| c.borrow().clone().unwrap_or_else(default_character));

    let platform_name = match msg.platform {
        SocialPlatform::Twitter => "Twitter",
        SocialPlatform::Discord => "Discord",
    };

    let char_limit = match msg.platform {
        SocialPlatform::Twitter => "under 280 characters",
        SocialPlatform::Discord => "under 500 characters",
    };

    let social_system_prompt = format!(
        "{}\n\nYou are responding on {}. Keep responses concise ({}). Be engaging and helpful. The user's handle is @{}.",
        character.system_prompt,
        platform_name,
        char_limit,
        msg.author_name
    );

    let state = ConversationState {
        messages: vec![
            Message {
                role: "system".to_string(),
                content: social_system_prompt,
            },
            Message {
                role: "user".to_string(),
                content: msg.content.clone(),
            },
        ],
        character,
        created_at: ic_cdk::api::time(),
        updated_at: ic_cdk::api::time(),
    };

    generate_response(&state).await
}

// ========== Social Integration: Admin APIs ==========

/// Configure Twitter integration
#[update]
fn configure_twitter(credentials: TwitterCredentials) -> Result<(), String> {
    require_admin()?;

    SOCIAL_CONFIG.with(|c| {
        let mut config = c.borrow_mut();
        if config.is_none() {
            *config = Some(SocialIntegrationConfig {
                twitter: None,
                discord: None,
                enabled_platforms: Vec::new(),
                auto_reply: false,
            });
        }
        if let Some(ref mut cfg) = *config {
            cfg.twitter = Some(credentials);
        }
    });

    Ok(())
}

/// Configure Discord integration
#[update]
fn configure_discord(config: DiscordConfig) -> Result<(), String> {
    require_admin()?;

    SOCIAL_CONFIG.with(|c| {
        let mut social_config = c.borrow_mut();
        if social_config.is_none() {
            *social_config = Some(SocialIntegrationConfig {
                twitter: None,
                discord: None,
                enabled_platforms: Vec::new(),
                auto_reply: false,
            });
        }
        if let Some(ref mut cfg) = *social_config {
            cfg.discord = Some(config);
        }
    });

    Ok(())
}

/// Enable/disable social platforms
#[update]
fn set_enabled_platforms(platforms: Vec<SocialPlatform>) -> Result<(), String> {
    require_admin()?;

    SOCIAL_CONFIG.with(|c| {
        let mut config = c.borrow_mut();
        if config.is_none() {
            *config = Some(SocialIntegrationConfig {
                twitter: None,
                discord: None,
                enabled_platforms: Vec::new(),
                auto_reply: false,
            });
        }
        if let Some(ref mut cfg) = *config {
            cfg.enabled_platforms = platforms;
        }
    });

    Ok(())
}

/// Enable/disable auto-reply
#[update]
fn set_auto_reply(enabled: bool) -> Result<(), String> {
    require_admin()?;

    SOCIAL_CONFIG.with(|c| {
        if let Some(ref mut cfg) = *c.borrow_mut() {
            cfg.auto_reply = enabled;
        }
    });

    Ok(())
}

/// Schedule a post
#[update]
fn schedule_post(
    platform: SocialPlatform,
    content: String,
    scheduled_time: u64,
    metadata: Option<PostMetadata>,
) -> Result<u64, String> {
    require_admin()?;
    schedule_post_internal(platform, content, scheduled_time, metadata)
}

fn schedule_post_internal(
    platform: SocialPlatform,
    content: String,
    scheduled_time: u64,
    metadata: Option<PostMetadata>,
) -> Result<u64, String> {
    // Validate content length
    match platform {
        SocialPlatform::Twitter if content.len() > 280 => {
            return Err("Twitter content exceeds 280 characters".to_string());
        }
        SocialPlatform::Discord if content.len() > 2000 => {
            return Err("Discord content exceeds 2000 characters".to_string());
        }
        _ => {}
    }

    let post_id = POST_COUNTER.with(|c| {
        let id = *c.borrow();
        *c.borrow_mut() = id + 1;
        id
    });

    let post = ScheduledPost {
        id: post_id,
        platform,
        content,
        scheduled_time,
        status: PostStatus::Pending,
        retry_count: 0,
        created_at: ic_cdk::api::time(),
        metadata,
    };

    SCHEDULED_POSTS.with(|p| {
        let mut posts = p.borrow_mut();
        posts.push(post);
        // Remove old completed/failed posts if over 200 total
        if posts.len() > 200 {
            posts.retain(|p| matches!(p.status, PostStatus::Pending | PostStatus::Processing));
        }
    });

    Ok(post_id)
}

/// Cancel a scheduled post
#[update]
fn cancel_scheduled_post(post_id: u64) -> Result<(), String> {
    require_admin()?;

    SCHEDULED_POSTS.with(|p| {
        let mut posts = p.borrow_mut();
        if posts.iter().any(|p| p.id == post_id && matches!(p.status, PostStatus::Pending)) {
            posts.retain(|p| p.id != post_id);
            Ok(())
        } else {
            Err("Post not found or not pending".to_string())
        }
    })
}

/// Get scheduled posts
#[query]
fn get_scheduled_posts() -> Vec<ScheduledPost> {
    SCHEDULED_POSTS.with(|p| p.borrow().clone())
}

/// Get incoming messages
#[query]
fn get_incoming_messages(limit: Option<u32>) -> Vec<IncomingMessage> {
    let limit = limit.unwrap_or(50) as usize;
    INCOMING_MESSAGES.with(|m| {
        m.borrow().iter().rev().take(limit).cloned().collect()
    })
}

/// Get social integration status
#[query]
fn get_social_status() -> SocialStatus {
    let config = SOCIAL_CONFIG.with(|c| c.borrow().clone());
    let polling_state = POLLING_STATE.with(|s| s.borrow().clone());
    let timer_active = TIMER_ID.with(|t| t.borrow().is_some());

    let pending_posts = SCHEDULED_POSTS.with(|p| {
        p.borrow().iter()
            .filter(|post| matches!(post.status, PostStatus::Pending))
            .count() as u32
    });

    let unprocessed_messages = INCOMING_MESSAGES.with(|m| {
        m.borrow().iter()
            .filter(|msg| !msg.processed)
            .count() as u32
    });

    SocialStatus {
        twitter_configured: config.as_ref().map(|c| c.twitter.is_some()).unwrap_or(false),
        discord_configured: config.as_ref().map(|c| c.discord.is_some()).unwrap_or(false),
        enabled_platforms: config.map(|c| c.enabled_platforms).unwrap_or_default(),
        polling_active: timer_active,
        last_twitter_poll: polling_state.twitter_last_poll_time,
        last_discord_poll: polling_state.discord_last_poll_time,
        pending_posts,
        unprocessed_messages,
    }
}

/// Manually trigger a poll
#[update]
async fn trigger_poll() -> Result<(), String> {
    require_admin()?;
    poll_and_process().await
}

/// Post immediately (bypass scheduling)
#[update]
async fn post_now(platform: SocialPlatform, content: String) -> Result<String, String> {
    require_admin()?;

    match platform {
        SocialPlatform::Twitter => post_tweet(&content, None).await,
        SocialPlatform::Discord => {
            let config = get_discord_config()?;
            if let Some(ref webhook_url) = config.webhook_url {
                send_discord_webhook(webhook_url, &content).await?;
                Ok("sent via webhook".to_string())
            } else if let Some(channel_id) = config.channel_ids.first() {
                send_discord_message(channel_id, &content).await
            } else {
                Err("No webhook URL or channel configured".to_string())
            }
        }
    }
}

// ========== Wallet Functions ==========

// ICP Ledger types (manual implementation)
#[derive(CandidType, Deserialize)]
struct AccountBalanceArgs {
    account: Vec<u8>,
}

#[derive(CandidType, Deserialize, Debug, Clone)]
struct Tokens {
    e8s: u64,
}

#[derive(CandidType, Deserialize)]
struct TransferArgsLedger {
    memo: u64,
    amount: Tokens,
    fee: Tokens,
    from_subaccount: Option<Vec<u8>>,
    to: Vec<u8>,
    created_at_time: Option<u64>,
}

#[derive(CandidType, Deserialize, Debug)]
enum TransferResultLedger {
    Ok(u64),
    Err(TransferErrorLedger),
}

#[derive(CandidType, Deserialize, Debug)]
enum TransferErrorLedger {
    BadFee { expected_fee: Tokens },
    InsufficientFunds { balance: Tokens },
    TxTooOld { allowed_window_nanos: u64 },
    TxCreatedInFuture,
    TxDuplicate { duplicate_of: u64 },
}

/// Compute Account Identifier from Principal (simplified version)
fn compute_account_identifier(principal: &Principal) -> Vec<u8> {
    use sha2::{Sha224, Digest};

    let mut hasher = Sha224::new();
    hasher.update(b"\x0Aaccount-id");
    hasher.update(principal.as_slice());
    hasher.update(&[0u8; 32]); // Default subaccount (32 zero bytes)

    let hash = hasher.finalize();
    let mut account_id = Vec::with_capacity(32);

    // CRC32 checksum
    let crc = crc32(&hash);
    account_id.extend_from_slice(&crc.to_be_bytes());
    account_id.extend_from_slice(&hash);

    account_id
}

/// Simple CRC32 implementation
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFFFFFF;
    for byte in data {
        crc ^= *byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB88320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Get the canister's ICP wallet address
#[query]
fn get_wallet_address() -> String {
    let canister_id = ic_cdk::id();
    let account_id = compute_account_identifier(&canister_id);
    hex::encode(&account_id)
}

/// Get wallet info including address and principal
#[query]
fn get_wallet_info() -> WalletInfo {
    let canister_id = ic_cdk::id();
    let account_id = compute_account_identifier(&canister_id);

    WalletInfo {
        icp_address: hex::encode(&account_id),
        principal_id: canister_id.to_string(),
        icp_balance: 0, // Will be updated by check_balance
        last_balance_update: 0,
    }
}

/// Check ICP balance from the ledger
#[update]
async fn check_icp_balance() -> Result<u64, String> {
    let canister_id = ic_cdk::id();
    let account_id = compute_account_identifier(&canister_id);

    let ledger_id = Principal::from_text(ICP_LEDGER_CANISTER_ID)
        .map_err(|e| format!("Invalid ledger canister ID: {:?}", e))?;

    // Call the ICP ledger to get balance
    let balance_result: Result<(Tokens,), _> = ic_cdk::call(
        ledger_id,
        "account_balance",
        (AccountBalanceArgs { account: account_id },),
    ).await;

    match balance_result {
        Ok((tokens,)) => Ok(tokens.e8s),
        Err((code, msg)) => Err(format!("Ledger call failed: {:?} - {}", code, msg)),
    }
}

/// Parse hex account identifier
fn parse_account_identifier(hex_str: &str) -> Result<Vec<u8>, String> {
    hex::decode(hex_str).map_err(|e| format!("Invalid hex: {:?}", e))
}

/// Send ICP to another address
#[update]
async fn send_icp(to_address: String, amount_e8s: u64, memo: Option<u64>) -> Result<u64, String> {
    require_admin()?;

    // Validate amount (minimum 10000 e8s = 0.0001 ICP for fee)
    if amount_e8s < 10_000 {
        return Err("Amount too small. Minimum is 10000 e8s (0.0001 ICP)".to_string());
    }

    // Parse destination address
    let to_account = parse_account_identifier(&to_address)?;
    if to_account.len() != 32 {
        return Err("Invalid account identifier length".to_string());
    }

    let ledger_id = Principal::from_text(ICP_LEDGER_CANISTER_ID)
        .map_err(|e| format!("Invalid ledger canister ID: {:?}", e))?;

    // Build transfer args
    let transfer_args = TransferArgsLedger {
        memo: memo.unwrap_or(0),
        amount: Tokens { e8s: amount_e8s },
        fee: Tokens { e8s: 10_000 }, // 0.0001 ICP fee
        from_subaccount: None,
        to: to_account,
        created_at_time: None,
    };

    // Call the ledger
    let transfer_result: Result<(TransferResultLedger,), _> = ic_cdk::call(
        ledger_id,
        "transfer",
        (transfer_args,),
    ).await;

    match transfer_result {
        Ok((TransferResultLedger::Ok(block_height),)) => {
            // Record transaction (keep max 1000 records)
            WALLET_STATE.with(|state| {
                let mut s = state.borrow_mut();
                s.tx_counter += 1;
                let tx = TransactionRecord {
                    id: s.tx_counter,
                    tx_type: TransactionType::Send,
                    amount: amount_e8s,
                    to: Some(to_address),
                    from: None,
                    memo: memo.unwrap_or(0),
                    timestamp: ic_cdk::api::time(),
                    status: TransactionStatus::Completed,
                    block_height: Some(block_height),
                };
                s.transaction_history.push(tx);
                // Limit history to prevent unbounded growth
                if s.transaction_history.len() > 1000 {
                    s.transaction_history.remove(0);
                }
            });

            ic_cdk::println!("ICP transfer successful: {} e8s sent, block: {}", amount_e8s, block_height);
            Ok(block_height)
        }
        Ok((TransferResultLedger::Err(err),)) => {
            let error_msg = format!("Transfer failed: {:?}", err);

            // Record failed transaction (keep max 1000 records)
            WALLET_STATE.with(|state| {
                let mut s = state.borrow_mut();
                s.tx_counter += 1;
                let tx = TransactionRecord {
                    id: s.tx_counter,
                    tx_type: TransactionType::Send,
                    amount: amount_e8s,
                    to: Some(to_address.clone()),
                    from: None,
                    memo: memo.unwrap_or(0),
                    timestamp: ic_cdk::api::time(),
                    status: TransactionStatus::Failed(error_msg.clone()),
                    block_height: None,
                };
                s.transaction_history.push(tx);
                // Limit history to prevent unbounded growth
                if s.transaction_history.len() > 1000 {
                    s.transaction_history.remove(0);
                }
            });

            Err(error_msg)
        }
        Err((code, msg)) => Err(format!("Ledger call failed: {:?} - {}", code, msg)),
    }
}

/// Get transaction history
#[query]
fn get_transaction_history(limit: Option<u32>) -> Vec<TransactionRecord> {
    let limit = limit.unwrap_or(50) as usize;

    WALLET_STATE.with(|state| {
        let s = state.borrow();
        s.transaction_history
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    })
}

/// Get wallet status summary
#[update]
async fn get_wallet_status() -> Result<WalletInfo, String> {
    let canister_id = ic_cdk::id();
    let account_id = compute_account_identifier(&canister_id);

    // Get balance
    let balance = check_icp_balance().await?;

    Ok(WalletInfo {
        icp_address: hex::encode(&account_id),
        principal_id: canister_id.to_string(),
        icp_balance: balance,
        last_balance_update: ic_cdk::api::time(),
    })
}

// ========== EVM Wallet (Chain-Key ECDSA) ==========

use ic_cdk::api::management_canister::ecdsa::{
    ecdsa_public_key, sign_with_ecdsa, EcdsaCurve, EcdsaKeyId, EcdsaPublicKeyArgument,
    SignWithEcdsaArgument,
};
use tiny_keccak::{Hasher, Keccak};

/// ECDSA key name for production (mainnet) or test (local)
fn get_ecdsa_key_id() -> EcdsaKeyId {
    // Use "key_1" for mainnet, "dfx_test_key" for local
    EcdsaKeyId {
        curve: EcdsaCurve::Secp256k1,
        name: "key_1".to_string(), // mainnet key
    }
}

/// Decompress a secp256k1 compressed public key
fn decompress_pubkey(compressed: &[u8]) -> Result<Vec<u8>, String> {
    use num_bigint::BigUint;

    if compressed.len() != 33 {
        return Err("Invalid compressed key length".to_string());
    }

    let prefix = compressed[0];
    if prefix != 0x02 && prefix != 0x03 {
        return Err("Invalid compression prefix".to_string());
    }

    // secp256k1 parameters
    // p = 2^256 - 2^32 - 977
    let p = BigUint::parse_bytes(
        b"FFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFEFFFFFC2F",
        16,
    ).unwrap();

    // x coordinate
    let x = BigUint::from_bytes_be(&compressed[1..]);

    // y² = x³ + 7 (mod p)
    let x_cubed = x.modpow(&BigUint::from(3u32), &p);
    let y_squared = (&x_cubed + BigUint::from(7u32)) % &p;

    // Calculate y = y_squared^((p+1)/4) mod p (since p ≡ 3 mod 4)
    let exp = (&p + BigUint::from(1u32)) / BigUint::from(4u32);
    let mut y = y_squared.modpow(&exp, &p);

    // Check if y has correct parity
    let y_is_odd = &y % BigUint::from(2u32) == BigUint::from(1u32);
    let should_be_odd = prefix == 0x03;

    if y_is_odd != should_be_odd {
        y = &p - &y;
    }

    // Build uncompressed key (0x04 + x + y)
    let mut uncompressed = vec![0x04];

    // Pad x to 32 bytes
    let x_bytes = x.to_bytes_be();
    for _ in 0..(32 - x_bytes.len()) {
        uncompressed.push(0);
    }
    uncompressed.extend_from_slice(&x_bytes);

    // Pad y to 32 bytes
    let y_bytes = y.to_bytes_be();
    for _ in 0..(32 - y_bytes.len()) {
        uncompressed.push(0);
    }
    uncompressed.extend_from_slice(&y_bytes);

    Ok(uncompressed)
}

/// Derive Ethereum address from ECDSA public key using Keccak-256
fn derive_eth_address(public_key: &[u8]) -> Result<String, String> {
    // ICP returns SEC1 encoded public key
    // - 33 bytes: compressed (0x02/0x03 prefix)
    // - 65 bytes: uncompressed (0x04 prefix)

    let uncompressed = match public_key.len() {
        65 if public_key[0] == 0x04 => {
            // Already uncompressed
            public_key.to_vec()
        }
        33 if public_key[0] == 0x02 || public_key[0] == 0x03 => {
            // Decompress
            decompress_pubkey(public_key)?
        }
        _ => {
            return Err(format!(
                "Invalid public key length: {} bytes. Expected 33 (compressed) or 65 (uncompressed). First byte: 0x{:02x}",
                public_key.len(),
                public_key.first().copied().unwrap_or(0)
            ));
        }
    };

    // Take the 64 bytes after the 0x04 prefix
    let key_bytes = &uncompressed[1..];

    let mut hasher = Keccak::v256();
    let mut hash = [0u8; 32];
    hasher.update(key_bytes);
    hasher.finalize(&mut hash);

    // Ethereum address is the last 20 bytes of the Keccak-256 hash
    Ok(format!("0x{}", hex::encode(&hash[12..])))
}

/// Get the canister's EVM wallet address (derived from Chain-Key ECDSA)
#[update]
async fn get_evm_address() -> Result<String, String> {
    // Check if we have a cached address
    let cached = EVM_WALLET_STATE.with(|s| s.borrow().cached_address.clone());
    if let Some(addr) = cached {
        return Ok(addr);
    }

    // Get ECDSA public key from management canister
    let key_id = get_ecdsa_key_id();
    let canister_id = ic_cdk::id();

    let derivation_path = vec![canister_id.as_slice().to_vec()];

    let request = EcdsaPublicKeyArgument {
        canister_id: Some(canister_id),
        derivation_path,
        key_id,
    };

    let (response,) = ecdsa_public_key(request)
        .await
        .map_err(|(code, msg)| format!("ECDSA public key error: {:?} - {}", code, msg))?;

    let eth_address = derive_eth_address(&response.public_key)?;

    // Cache the address
    EVM_WALLET_STATE.with(|s| {
        s.borrow_mut().cached_address = Some(eth_address.clone());
    });

    Ok(eth_address)
}

/// Get EVM wallet info for a specific chain
#[update]
async fn get_evm_wallet_info(chain_id: u64) -> Result<EvmWalletInfo, String> {
    let address = get_evm_address().await?;

    let chain_name = match chain_id {
        1 => "Ethereum Mainnet",
        8453 => "Base",
        137 => "Polygon",
        10 => "Optimism",
        42161 => "Arbitrum One",
        11155111 => "Sepolia (Testnet)",
        84532 => "Base Sepolia (Testnet)",
        _ => "Unknown Chain",
    }.to_string();

    Ok(EvmWalletInfo {
        address,
        chain_id,
        chain_name,
    })
}

/// Configure an EVM chain (Admin only)
#[update]
fn configure_evm_chain(config: EvmChainConfig) -> Result<(), String> {
    require_admin()?;

    EVM_WALLET_STATE.with(|s| {
        let mut state = s.borrow_mut();
        // Update or add chain config
        if let Some(existing) = state.configured_chains.iter_mut().find(|c| c.chain_id == config.chain_id) {
            *existing = config;
        } else {
            state.configured_chains.push(config);
        }
    });

    Ok(())
}

/// Get configured EVM chains
#[query]
fn get_configured_chains() -> Vec<EvmChainConfig> {
    EVM_WALLET_STATE.with(|s| s.borrow().configured_chains.clone())
}

/// RLP encode a u64 value
fn rlp_encode_u64(value: u64) -> Vec<u8> {
    if value == 0 {
        vec![0x80]
    } else if value < 128 {
        vec![value as u8]
    } else {
        let bytes = value.to_be_bytes();
        let start = bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let significant_bytes = &bytes[start..];
        let len = significant_bytes.len();
        let mut result = vec![0x80 + len as u8];
        result.extend_from_slice(significant_bytes);
        result
    }
}

/// RLP encode bytes
fn rlp_encode_bytes(data: &[u8]) -> Vec<u8> {
    if data.len() == 1 && data[0] < 128 {
        data.to_vec()
    } else if data.len() < 56 {
        let mut result = vec![0x80 + data.len() as u8];
        result.extend_from_slice(data);
        result
    } else {
        let len_bytes = data.len().to_be_bytes();
        let start = len_bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let significant_len_bytes = &len_bytes[start..];
        let mut result = vec![0xb7 + significant_len_bytes.len() as u8];
        result.extend_from_slice(significant_len_bytes);
        result.extend_from_slice(data);
        result
    }
}

/// RLP encode a list
fn rlp_encode_list(items: &[Vec<u8>]) -> Vec<u8> {
    let mut payload = Vec::new();
    for item in items {
        payload.extend_from_slice(item);
    }

    if payload.len() < 56 {
        let mut result = vec![0xc0 + payload.len() as u8];
        result.extend_from_slice(&payload);
        result
    } else {
        let len_bytes = payload.len().to_be_bytes();
        let start = len_bytes.iter().position(|&b| b != 0).unwrap_or(7);
        let significant_len_bytes = &len_bytes[start..];
        let mut result = vec![0xf7 + significant_len_bytes.len() as u8];
        result.extend_from_slice(significant_len_bytes);
        result.extend_from_slice(&payload);
        result
    }
}

/// Parse hex string to bytes
fn hex_to_bytes(hex_str: &str) -> Result<Vec<u8>, String> {
    let s = hex_str.strip_prefix("0x").unwrap_or(hex_str);
    hex::decode(s).map_err(|e| format!("Invalid hex: {:?}", e))
}

/// Parse wei string to bytes (for large numbers)
fn wei_to_bytes(wei_str: &str) -> Result<Vec<u8>, String> {
    use num_bigint::BigUint;
    let value = wei_str.parse::<BigUint>()
        .map_err(|e| format!("Invalid wei value: {:?}", e))?;
    let bytes = value.to_bytes_be();
    // Remove leading zeros
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(bytes.len() - 1);
    Ok(bytes[start..].to_vec())
}

/// Build EIP-1559 transaction for signing
fn build_eip1559_tx_for_signing(
    chain_id: u64,
    nonce: u64,
    max_priority_fee_per_gas: u64,
    max_fee_per_gas: u64,
    gas_limit: u64,
    to: &[u8],
    value: &[u8],
    data: &[u8],
) -> Vec<u8> {
    let items = vec![
        rlp_encode_u64(chain_id),
        rlp_encode_u64(nonce),
        rlp_encode_u64(max_priority_fee_per_gas),
        rlp_encode_u64(max_fee_per_gas),
        rlp_encode_u64(gas_limit),
        rlp_encode_bytes(to),
        rlp_encode_bytes(value),
        rlp_encode_bytes(data),
        rlp_encode_bytes(&[]), // accessList (empty)
    ];

    let mut tx = vec![0x02]; // EIP-1559 transaction type
    tx.extend_from_slice(&rlp_encode_list(&items));
    tx
}

/// Sign a message using Chain-Key ECDSA
async fn sign_with_chain_key_ecdsa(message_hash: &[u8]) -> Result<Vec<u8>, String> {
    let key_id = get_ecdsa_key_id();
    let canister_id = ic_cdk::id();
    let derivation_path = vec![canister_id.as_slice().to_vec()];

    let request = SignWithEcdsaArgument {
        message_hash: message_hash.to_vec(),
        derivation_path,
        key_id,
    };

    let (response,) = sign_with_ecdsa(request)
        .await
        .map_err(|(code, msg)| format!("ECDSA signing error: {:?} - {}", code, msg))?;

    Ok(response.signature)
}

/// Send signed transaction to EVM RPC
async fn send_raw_transaction(rpc_url: &str, raw_tx: &[u8]) -> Result<String, String> {
    let raw_tx_hex = format!("0x{}", hex::encode(raw_tx));

    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_sendRawTransaction",
        "params": [raw_tx_hex],
        "id": 1
    });

    let request = CanisterHttpRequestArgument {
        url: rpc_url.to_string(),
        max_response_bytes: Some(5_000),
        method: HttpMethod::POST,
        headers: vec![
            HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            },
        ],
        body: Some(request_body.to_string().into_bytes()),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_evm_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("JSON error: {} - Body: {}", e, body))?;

            if let Some(error) = json.get("error") {
                return Err(format!("RPC error: {}", error));
            }

            json["result"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("No tx hash in response: {}", body))
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

/// Get nonce for address from EVM RPC
async fn get_nonce(rpc_url: &str, address: &str) -> Result<u64, String> {
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getTransactionCount",
        "params": [address, "pending"],
        "id": 1
    });

    let request = CanisterHttpRequestArgument {
        url: rpc_url.to_string(),
        max_response_bytes: Some(2_000),
        method: HttpMethod::POST,
        headers: vec![
            HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            },
        ],
        body: Some(request_body.to_string().into_bytes()),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_evm_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 30_000_000_000u128;

    match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("JSON error: {}", e))?;

            let nonce_hex = json["result"]
                .as_str()
                .ok_or_else(|| "No nonce in response".to_string())?;

            let nonce_str = nonce_hex.strip_prefix("0x").unwrap_or(nonce_hex);
            u64::from_str_radix(nonce_str, 16)
                .map_err(|e| format!("Invalid nonce: {:?}", e))
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

/// Get gas price from EVM RPC
async fn get_gas_price(rpc_url: &str) -> Result<u64, String> {
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_gasPrice",
        "params": [],
        "id": 1
    });

    let request = CanisterHttpRequestArgument {
        url: rpc_url.to_string(),
        max_response_bytes: Some(2_000),
        method: HttpMethod::POST,
        headers: vec![
            HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            },
        ],
        body: Some(request_body.to_string().into_bytes()),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_evm_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 30_000_000_000u128;

    match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("JSON error: {}", e))?;

            let gas_hex = json["result"]
                .as_str()
                .ok_or_else(|| "No gas price in response".to_string())?;

            let gas_str = gas_hex.strip_prefix("0x").unwrap_or(gas_hex);
            u64::from_str_radix(gas_str, 16)
                .map_err(|e| format!("Invalid gas price: {:?}", e))
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

/// Transform function for EVM RPC responses
#[query]
fn transform_evm_response(raw: TransformArgs) -> HttpResponse {
    HttpResponse {
        status: raw.response.status,
        body: raw.response.body,
        headers: vec![],
    }
}

/// Send native token (ETH, MATIC, etc.) on EVM chain - Admin Only
#[update]
async fn send_evm_native(
    chain_id: u64,
    to_address: String,
    amount_wei: String,
) -> Result<String, String> {
    // ========== ADMIN ONLY ==========
    require_admin()?;

    // Get chain config
    let chain_config = EVM_WALLET_STATE.with(|s| {
        s.borrow().configured_chains.iter().find(|c| c.chain_id == chain_id).cloned()
    }).ok_or_else(|| format!("Chain {} not configured. Use configure_evm_chain first.", chain_id))?;

    // Get our address
    let from_address = get_evm_address().await?;

    // Get nonce
    let nonce = get_nonce(&chain_config.rpc_url, &from_address).await?;

    // Get gas price
    let gas_price = get_gas_price(&chain_config.rpc_url).await?;
    let max_fee_per_gas = gas_price * 2; // 2x for safety
    let max_priority_fee_per_gas = 1_500_000_000; // 1.5 gwei

    // Parse addresses and values
    let to_bytes = hex_to_bytes(&to_address)?;
    if to_bytes.len() != 20 {
        return Err("Invalid to address length".to_string());
    }

    let value_bytes = wei_to_bytes(&amount_wei)?;

    // Build transaction for signing (EIP-1559)
    let gas_limit = 21_000u64; // Standard ETH transfer
    let tx_for_signing = build_eip1559_tx_for_signing(
        chain_id,
        nonce,
        max_priority_fee_per_gas,
        max_fee_per_gas,
        gas_limit,
        &to_bytes,
        &value_bytes,
        &[], // no data for native transfer
    );

    // Hash the transaction
    let mut hasher = Keccak::v256();
    let mut tx_hash = [0u8; 32];
    hasher.update(&tx_for_signing);
    hasher.finalize(&mut tx_hash);

    // Sign with Chain-Key ECDSA
    let signature = sign_with_chain_key_ecdsa(&tx_hash).await?;

    // Parse signature (r, s, v)
    if signature.len() != 64 {
        return Err(format!("Invalid signature length: {}", signature.len()));
    }
    let r = &signature[..32];
    let s = &signature[32..];

    // Determine recovery id (v) - try both 0 and 1
    // For EIP-1559, v is just 0 or 1 (not 27/28)
    let v = 0u8; // We'll try 0 first, recovery logic would be needed for production

    // Build signed transaction
    let signed_items = vec![
        rlp_encode_u64(chain_id),
        rlp_encode_u64(nonce),
        rlp_encode_u64(max_priority_fee_per_gas),
        rlp_encode_u64(max_fee_per_gas),
        rlp_encode_u64(gas_limit),
        rlp_encode_bytes(&to_bytes),
        rlp_encode_bytes(&value_bytes),
        rlp_encode_bytes(&[]), // data
        rlp_encode_bytes(&[]), // accessList
        rlp_encode_bytes(&[v]),
        rlp_encode_bytes(r),
        rlp_encode_bytes(s),
    ];

    let mut signed_tx = vec![0x02]; // EIP-1559 type
    signed_tx.extend_from_slice(&rlp_encode_list(&signed_items));

    // Send transaction
    let tx_hash_result = send_raw_transaction(&chain_config.rpc_url, &signed_tx).await?;

    // Record transaction
    EVM_WALLET_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.tx_counter += 1;
        let tx_record = EvmTransactionRecord {
            id: s.tx_counter,
            chain_id,
            tx_hash: Some(tx_hash_result.clone()),
            to: to_address.clone(),
            value_wei: amount_wei.clone(),
            data: None,
            timestamp: ic_cdk::api::time(),
            status: EvmTransactionStatus::Submitted(tx_hash_result.clone()),
        };
        s.transaction_history.push(tx_record);

        // Limit history
        if s.transaction_history.len() > 500 {
            s.transaction_history.remove(0);
        }
    });

    ic_cdk::println!("EVM transfer submitted: {} to {}, tx: {}", amount_wei, to_address, tx_hash_result);
    Ok(tx_hash_result)
}

/// Get EVM transaction history
#[query]
fn get_evm_transaction_history(limit: Option<u32>) -> Vec<EvmTransactionRecord> {
    let limit = limit.unwrap_or(50) as usize;

    EVM_WALLET_STATE.with(|state| {
        let s = state.borrow();
        s.transaction_history
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    })
}

/// Get EVM balance from RPC (Admin can check, but public can view)
#[update]
async fn get_evm_balance(chain_id: u64) -> Result<String, String> {
    let chain_config = EVM_WALLET_STATE.with(|s| {
        s.borrow().configured_chains.iter().find(|c| c.chain_id == chain_id).cloned()
    }).ok_or_else(|| format!("Chain {} not configured", chain_id))?;

    let address = get_evm_address().await?;

    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_getBalance",
        "params": [address, "latest"],
        "id": 1
    });

    let request = CanisterHttpRequestArgument {
        url: chain_config.rpc_url.clone(),
        max_response_bytes: Some(2_000),
        method: HttpMethod::POST,
        headers: vec![
            HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            },
        ],
        body: Some(request_body.to_string().into_bytes()),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_evm_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 30_000_000_000u128;

    match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("JSON error: {}", e))?;

            json["result"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "No balance in response".to_string())
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

// Candid export
ic_cdk::export_candid!();
