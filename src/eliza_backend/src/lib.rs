use candid::{CandidType, Deserialize, Principal};
use ic_cdk::api::management_canister::http_request::{
    http_request, CanisterHttpRequestArgument, HttpHeader, HttpMethod, HttpResponse, TransformArgs,
    TransformContext, TransformFunc,
};
use ic_cdk_macros::{init, pre_upgrade, post_upgrade, query, update};
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

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
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

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct EvmWalletState {
    pub cached_address: Option<String>,
    pub transaction_history: Vec<EvmTransactionRecord>,
    pub tx_counter: u64,
    pub configured_chains: Vec<EvmChainConfig>,
}

// ========== Solana Wallet Data Structures ==========

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct SolanaWalletInfo {
    pub address: String,              // Base58 encoded public key
    pub network: String,              // "mainnet-beta", "devnet", "testnet"
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct SolanaTransactionRecord {
    pub id: u64,
    pub signature: Option<String>,    // Base58 encoded signature
    pub to: String,
    pub amount_lamports: u64,         // 1 SOL = 1,000,000,000 lamports
    pub timestamp: u64,
    pub status: SolanaTransactionStatus,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub enum SolanaTransactionStatus {
    Pending,
    Submitted(String),                // signature
    Confirmed(u64),                   // slot
    Failed(String),                   // error message
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct SolanaNetworkConfig {
    pub network_name: String,         // "mainnet-beta", "devnet", "testnet"
    pub rpc_url: String,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug, Default)]
pub struct SolanaWalletState {
    pub initialized: bool,
    pub public_key: Option<Vec<u8>>,           // 32 bytes Ed25519 public key
    pub encrypted_secret_key: Option<Vec<u8>>, // 32 bytes Ed25519 secret key (encrypted)
    pub cached_address: Option<String>,
    pub transaction_history: Vec<SolanaTransactionRecord>,
    pub tx_counter: u64,
    pub configured_networks: Vec<SolanaNetworkConfig>,
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

    // Solana Wallet State (Ed25519)
    static SOLANA_WALLET_STATE: RefCell<SolanaWalletState> = RefCell::new(SolanaWalletState {
        initialized: false,
        public_key: None,
        encrypted_secret_key: None,
        cached_address: None,
        transaction_history: Vec::new(),
        tx_counter: 0,
        configured_networks: Vec::new(),
    });
}

// ========== Stable Memory for Upgrades ==========

/// State that persists across canister upgrades
#[derive(CandidType, Deserialize, Serialize, Clone, Default)]
struct StableState {
    // Core state
    conversations: HashMap<Principal, ConversationState>,
    encrypted_api_key: Option<Vec<u8>>,
    character: Option<Character>,
    config: Option<Config>,

    // Social integration
    social_config: Option<SocialIntegrationConfig>,
    scheduled_posts: Vec<ScheduledPost>,
    incoming_messages: Vec<IncomingMessage>,
    polling_state: PollingState,
    post_counter: u64,
    auto_post_config: Option<AutoPostConfig>,

    // Wallet states
    wallet_state: WalletState,
    evm_wallet_state: EvmWalletState,
    solana_wallet_state: SolanaWalletState,
}

impl Default for WalletState {
    fn default() -> Self {
        WalletState {
            transaction_history: Vec::new(),
            tx_counter: 0,
        }
    }
}

impl Default for EvmWalletState {
    fn default() -> Self {
        EvmWalletState {
            cached_address: None,
            transaction_history: Vec::new(),
            tx_counter: 0,
            configured_chains: Vec::new(),
        }
    }
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

#[pre_upgrade]
fn pre_upgrade() {
    // Collect all state into StableState
    let state = StableState {
        conversations: CONVERSATIONS.with(|c| c.borrow().clone()),
        encrypted_api_key: ENCRYPTED_API_KEY.with(|k| k.borrow().clone()),
        character: CHARACTER.with(|c| c.borrow().clone()),
        config: CONFIG.with(|c| c.borrow().clone()),
        social_config: SOCIAL_CONFIG.with(|c| c.borrow().clone()),
        scheduled_posts: SCHEDULED_POSTS.with(|p| p.borrow().clone()),
        incoming_messages: INCOMING_MESSAGES.with(|m| m.borrow().clone()),
        polling_state: POLLING_STATE.with(|p| p.borrow().clone()),
        post_counter: POST_COUNTER.with(|c| *c.borrow()),
        auto_post_config: AUTO_POST_CONFIG.with(|c| c.borrow().clone()),
        wallet_state: WALLET_STATE.with(|w| w.borrow().clone()),
        evm_wallet_state: EVM_WALLET_STATE.with(|w| w.borrow().clone()),
        solana_wallet_state: SOLANA_WALLET_STATE.with(|w| w.borrow().clone()),
    };

    // Serialize to stable memory
    let serialized = candid::encode_one(&state).expect("Failed to serialize state");

    // Write length prefix + data to stable memory
    let len = serialized.len() as u64;
    let len_bytes = len.to_le_bytes();

    // Grow stable memory if needed (1 page = 64KB)
    let needed_pages = ((8 + serialized.len()) as u64 + 65535) / 65536;
    let current_pages = ic_cdk::api::stable::stable_size();
    if current_pages < needed_pages {
        ic_cdk::api::stable::stable_grow(needed_pages - current_pages)
            .expect("Failed to grow stable memory");
    }

    // Write length prefix
    ic_cdk::api::stable::stable_write(0, &len_bytes);
    // Write serialized data
    ic_cdk::api::stable::stable_write(8, &serialized);
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
            // Sanity check: max 100MB
            // Read serialized data
            let mut serialized = vec![0u8; len];
            ic_cdk::api::stable::stable_read(8, &mut serialized);

            // Deserialize state
            if let Ok(state) = candid::decode_one::<StableState>(&serialized) {
                // Restore all state
                CONVERSATIONS.with(|c| *c.borrow_mut() = state.conversations);
                ENCRYPTED_API_KEY.with(|k| *k.borrow_mut() = state.encrypted_api_key);
                CHARACTER.with(|c| *c.borrow_mut() = state.character);
                CONFIG.with(|c| *c.borrow_mut() = state.config);
                SOCIAL_CONFIG.with(|c| *c.borrow_mut() = state.social_config);
                SCHEDULED_POSTS.with(|p| *p.borrow_mut() = state.scheduled_posts);
                INCOMING_MESSAGES.with(|m| *m.borrow_mut() = state.incoming_messages);
                POLLING_STATE.with(|p| *p.borrow_mut() = state.polling_state);
                POST_COUNTER.with(|c| *c.borrow_mut() = state.post_counter);
                AUTO_POST_CONFIG.with(|c| *c.borrow_mut() = state.auto_post_config);
                WALLET_STATE.with(|w| *w.borrow_mut() = state.wallet_state);
                EVM_WALLET_STATE.with(|w| *w.borrow_mut() = state.evm_wallet_state);
                SOLANA_WALLET_STATE.with(|w| *w.borrow_mut() = state.solana_wallet_state);

                ic_cdk::println!("State restored from stable memory successfully");
                return;
            }
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
                llm_provider: LlmProvider::Fallback,
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
    "Coo is running on-chain with stable memory!".to_string()
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
            // Limit to 20 chains max
            if state.configured_chains.len() >= 20 {
                return Err("Maximum 20 chains allowed. Remove a chain first.".to_string());
            }
            state.configured_chains.push(config);
        }
        Ok(())
    })
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

    // Handle zero case
    if value == BigUint::from(0u32) {
        return Ok(vec![]);
    }

    let bytes = value.to_bytes_be();
    // Remove leading zeros
    let start = bytes.iter().position(|&b| b != 0).unwrap_or(0);
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
    // Use saturating multiplication to prevent overflow
    let max_fee_per_gas = gas_price.saturating_mul(2); // 2x for safety
    let max_priority_fee_per_gas = 1_500_000_000u64; // 1.5 gwei

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

    // Parse signature (r, s)
    if signature.len() != 64 {
        return Err(format!("Invalid signature length: {}", signature.len()));
    }
    let r = &signature[..32];
    let s = &signature[32..];

    // Try both recovery IDs (0 and 1) - EIP-1559 uses 0/1, not 27/28
    // We try v=0 first, then v=1 if that fails
    let mut tx_hash_result: Option<String> = None;
    let mut last_error = String::new();

    for v in [0u8, 1u8] {
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

        // Try to send transaction
        match send_raw_transaction(&chain_config.rpc_url, &signed_tx).await {
            Ok(hash) => {
                tx_hash_result = Some(hash);
                break;
            }
            Err(e) => {
                last_error = e;
                // Continue to try next v value
            }
        }
    }

    let tx_hash_result = tx_hash_result.ok_or_else(|| {
        format!("Transaction failed with both recovery IDs. Last error: {}", last_error)
    })?;

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

/// Send ERC-20 tokens (Admin only)
/// Parameters: chain_id, token_contract_address, to_address, amount (in token's smallest unit)
#[update]
async fn send_erc20(
    chain_id: u64,
    token_address: String,
    to_address: String,
    amount: String,
) -> Result<String, String> {
    // ========== ADMIN ONLY ==========
    require_admin()?;

    // Get chain config
    let chain_config = EVM_WALLET_STATE.with(|s| {
        s.borrow().configured_chains.iter().find(|c| c.chain_id == chain_id).cloned()
    }).ok_or_else(|| format!("Chain {} not configured", chain_id))?;

    // Get our address
    let from_address = get_evm_address().await?;

    // Validate addresses
    let token_bytes = hex_to_bytes(&token_address)?;
    if token_bytes.len() != 20 {
        return Err("Invalid token contract address".to_string());
    }

    let to_bytes = hex_to_bytes(&to_address)?;
    if to_bytes.len() != 20 {
        return Err("Invalid recipient address".to_string());
    }

    // Parse amount to bytes (big-endian, 32 bytes)
    let amount_bytes = parse_token_amount(&amount)?;

    // Build ERC-20 transfer data
    // transfer(address,uint256) = 0xa9059cbb
    let mut data = Vec::with_capacity(68);
    data.extend_from_slice(&[0xa9, 0x05, 0x9c, 0xbb]); // function selector
    // Pad address to 32 bytes
    data.extend_from_slice(&[0u8; 12]); // 12 zero bytes
    data.extend_from_slice(&to_bytes);   // 20 bytes address
    // Amount as 32 bytes
    data.extend_from_slice(&amount_bytes);

    // Get nonce
    let nonce = get_nonce(&chain_config.rpc_url, &from_address).await?;

    // Get gas price
    let gas_price = get_gas_price(&chain_config.rpc_url).await?;
    let max_fee_per_gas = gas_price.saturating_mul(2);
    let max_priority_fee_per_gas = 1_500_000_000u64;

    // Gas limit for ERC-20 transfer (higher than native transfer)
    let gas_limit = 100_000u64;

    // Build transaction (value = 0 for ERC-20 transfer)
    let tx_for_signing = build_eip1559_tx_for_signing(
        chain_id,
        nonce,
        max_priority_fee_per_gas,
        max_fee_per_gas,
        gas_limit,
        &token_bytes, // to = token contract
        &[],          // value = 0
        &data,        // ERC-20 transfer call data
    );

    // Hash and sign
    let mut hasher = Keccak::v256();
    let mut tx_hash = [0u8; 32];
    hasher.update(&tx_for_signing);
    hasher.finalize(&mut tx_hash);

    let signature = sign_with_chain_key_ecdsa(&tx_hash).await?;

    if signature.len() != 64 {
        return Err(format!("Invalid signature length: {}", signature.len()));
    }
    let r = &signature[..32];
    let s = &signature[32..];

    // Try both recovery IDs
    let mut tx_hash_result: Option<String> = None;
    let mut last_error = String::new();

    for v in [0u8, 1u8] {
        let signed_items = vec![
            rlp_encode_u64(chain_id),
            rlp_encode_u64(nonce),
            rlp_encode_u64(max_priority_fee_per_gas),
            rlp_encode_u64(max_fee_per_gas),
            rlp_encode_u64(gas_limit),
            rlp_encode_bytes(&token_bytes),
            rlp_encode_bytes(&[]), // value = 0
            rlp_encode_bytes(&data),
            rlp_encode_bytes(&[]), // accessList
            rlp_encode_bytes(&[v]),
            rlp_encode_bytes(r),
            rlp_encode_bytes(s),
        ];

        let signed_rlp = rlp_encode_list(&signed_items);
        let mut raw_tx = vec![0x02u8]; // EIP-1559 type
        raw_tx.extend_from_slice(&signed_rlp);

        match send_raw_transaction(&chain_config.rpc_url, &raw_tx).await {
            Ok(hash) => {
                tx_hash_result = Some(hash);
                break;
            }
            Err(e) => {
                last_error = e;
            }
        }
    }

    let tx_hash_result = tx_hash_result.ok_or(last_error)?;

    // Record transaction
    EVM_WALLET_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.tx_counter += 1;
        let tx_id = s.tx_counter;
        let record = EvmTransactionRecord {
            id: tx_id,
            chain_id,
            tx_hash: Some(tx_hash_result.clone()),
            to: to_address.clone(),
            value_wei: format!("ERC20:{} amount:{}", token_address, amount),
            data: Some(hex::encode(&data)),
            timestamp: ic_cdk::api::time(),
            status: EvmTransactionStatus::Submitted(tx_hash_result.clone()),
        };
        s.transaction_history.push(record);

        if s.transaction_history.len() > 500 {
            s.transaction_history.remove(0);
        }
    });

    ic_cdk::println!("ERC-20 transfer: {} {} to {}", amount, token_address, to_address);
    Ok(tx_hash_result)
}

/// Parse token amount string to 32-byte big-endian representation
fn parse_token_amount(amount_str: &str) -> Result<[u8; 32], String> {
    use num_bigint::BigUint;

    let amount = amount_str
        .parse::<BigUint>()
        .map_err(|e| format!("Invalid amount: {}", e))?;

    let bytes = amount.to_bytes_be();
    if bytes.len() > 32 {
        return Err("Amount too large".to_string());
    }

    let mut result = [0u8; 32];
    result[32 - bytes.len()..].copy_from_slice(&bytes);
    Ok(result)
}

/// Get ERC-20 token balance
#[update]
async fn get_erc20_balance(
    chain_id: u64,
    token_address: String,
    wallet_address: Option<String>,
) -> Result<String, String> {
    let chain_config = EVM_WALLET_STATE.with(|s| {
        s.borrow().configured_chains.iter().find(|c| c.chain_id == chain_id).cloned()
    }).ok_or_else(|| format!("Chain {} not configured", chain_id))?;

    let wallet = match wallet_address {
        Some(addr) => addr,
        None => get_evm_address().await?,
    };

    let wallet_bytes = hex_to_bytes(&wallet)?;
    if wallet_bytes.len() != 20 {
        return Err("Invalid wallet address".to_string());
    }

    // balanceOf(address) = 0x70a08231
    let mut data = Vec::with_capacity(36);
    data.extend_from_slice(&[0x70, 0xa0, 0x82, 0x31]);
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(&wallet_bytes);

    let data_hex = format!("0x{}", hex::encode(&data));

    // eth_call
    let request_body = format!(
        r#"{{"jsonrpc":"2.0","method":"eth_call","params":[{{"to":"{}","data":"{}"}},"latest"],"id":1}}"#,
        token_address, data_hex
    );

    let request = CanisterHttpRequestArgument {
        url: chain_config.rpc_url.clone(),
        max_response_bytes: Some(2000),
        method: HttpMethod::POST,
        headers: vec![HttpHeader {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        }],
        body: Some(request_body.into_bytes()),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_evm_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;
    let (response,): (HttpResponse,) = http_request(request, cycles)
        .await
        .map_err(|(code, msg)| format!("HTTP error: {:?} - {}", code, msg))?;

    let body = String::from_utf8(response.body)
        .map_err(|e| format!("Invalid response: {}", e))?;

    // Parse result
    if let Some(start) = body.find("\"result\":\"") {
        let start = start + 10;
        if let Some(end) = body[start..].find('"') {
            let hex_result = &body[start..start + end];
            // Convert hex to decimal string
            let hex_value = hex_result.trim_start_matches("0x");
            if hex_value.is_empty() || hex_value == "0" {
                return Ok("0".to_string());
            }
            use num_bigint::BigUint;
            let value = BigUint::parse_bytes(hex_value.as_bytes(), 16)
                .ok_or("Failed to parse balance")?;
            return Ok(value.to_string());
        }
    }

    Err(format!("Failed to parse balance response: {}", body))
}

// ========== LiFi Cross-Chain Bridge ==========

/// LiFi API endpoints
const LIFI_QUOTE_API: &str = "https://li.quest/v1/quote";

/// LiFi bridge quote response
#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct LiFiBridgeQuote {
    pub from_chain_id: u64,
    pub to_chain_id: u64,
    pub from_token: String,
    pub to_token: String,
    pub from_amount: String,
    pub to_amount: String,
    pub estimated_gas: String,
    pub tool: String,
}

/// Get LiFi bridge quote
#[update]
async fn get_lifi_quote(
    from_chain_id: u64,
    to_chain_id: u64,
    from_token: String,
    to_token: String,
    from_amount: String,
) -> Result<LiFiBridgeQuote, String> {
    let from_address = get_evm_address().await?;

    let url = format!(
        "{}?fromChain={}&toChain={}&fromToken={}&toToken={}&fromAmount={}&fromAddress={}",
        LIFI_QUOTE_API, from_chain_id, to_chain_id, from_token, to_token, from_amount, from_address
    );

    let request = CanisterHttpRequestArgument {
        url,
        max_response_bytes: Some(50_000),
        method: HttpMethod::GET,
        headers: vec![],
        body: None,
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_evm_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    let (response,): (HttpResponse,) = http_request(request, cycles)
        .await
        .map_err(|(code, msg)| format!("HTTP error: {:?} - {}", code, msg))?;

    let body = String::from_utf8(response.body)
        .map_err(|e| format!("UTF-8 error: {}", e))?;

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("JSON error: {} - Body: {}", e, body))?;

    if let Some(error) = json.get("message") {
        if json.get("code").is_some() {
            return Err(format!("LiFi API error: {}", error));
        }
    }

    let estimate = &json["estimate"];
    let action = &json["action"];
    let tool = json["tool"].as_str().unwrap_or("unknown");

    Ok(LiFiBridgeQuote {
        from_chain_id,
        to_chain_id,
        from_token: action["fromToken"]["address"].as_str().unwrap_or(&from_token).to_string(),
        to_token: action["toToken"]["address"].as_str().unwrap_or(&to_token).to_string(),
        from_amount: from_amount.clone(),
        to_amount: estimate["toAmount"].as_str().unwrap_or("0").to_string(),
        estimated_gas: estimate["gasCosts"][0]["amount"].as_str().unwrap_or("0").to_string(),
        tool: tool.to_string(),
    })
}

/// Execute LiFi bridge (Admin only)
#[update]
async fn execute_lifi_bridge(
    from_chain_id: u64,
    to_chain_id: u64,
    from_token: String,
    to_token: String,
    from_amount: String,
) -> Result<String, String> {
    // ========== ADMIN ONLY ==========
    require_admin()?;

    // Get chain config for source chain
    let chain_config = EVM_WALLET_STATE.with(|s| {
        s.borrow().configured_chains.iter().find(|c| c.chain_id == from_chain_id).cloned()
    }).ok_or_else(|| format!("Source chain {} not configured", from_chain_id))?;

    let from_address = get_evm_address().await?;

    // Get quote with transaction data
    let url = format!(
        "{}?fromChain={}&toChain={}&fromToken={}&toToken={}&fromAmount={}&fromAddress={}",
        LIFI_QUOTE_API, from_chain_id, to_chain_id, from_token, to_token, from_amount, from_address
    );

    let request = CanisterHttpRequestArgument {
        url,
        max_response_bytes: Some(100_000),
        method: HttpMethod::GET,
        headers: vec![],
        body: None,
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_evm_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    let (response,): (HttpResponse,) = http_request(request, cycles)
        .await
        .map_err(|(code, msg)| format!("Quote HTTP error: {:?} - {}", code, msg))?;

    let body = String::from_utf8(response.body)
        .map_err(|e| format!("UTF-8 error: {}", e))?;

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("JSON error: {}", e))?;

    // Extract transaction data
    let tx_request = &json["transactionRequest"];
    let to = tx_request["to"].as_str().ok_or("No 'to' address in transaction")?;
    let value = tx_request["value"].as_str().unwrap_or("0x0");
    let data = tx_request["data"].as_str().ok_or("No 'data' in transaction")?;
    let gas_limit_hex = tx_request["gasLimit"].as_str().unwrap_or("0x100000");

    // Parse values
    let to_bytes = hex_to_bytes(to)?;
    let value_bytes = hex_to_bytes(value)?;
    let data_bytes = hex::decode(data.trim_start_matches("0x"))
        .map_err(|e| format!("Invalid data hex: {}", e))?;
    let gas_limit = u64::from_str_radix(gas_limit_hex.trim_start_matches("0x"), 16)
        .unwrap_or(500_000);

    // Get nonce and gas price
    let nonce = get_nonce(&chain_config.rpc_url, &from_address).await?;
    let gas_price = get_gas_price(&chain_config.rpc_url).await?;
    let max_fee_per_gas = gas_price.saturating_mul(2);
    let max_priority_fee_per_gas = 1_500_000_000u64;

    // Build transaction
    let tx_for_signing = build_eip1559_tx_for_signing(
        from_chain_id,
        nonce,
        max_priority_fee_per_gas,
        max_fee_per_gas,
        gas_limit,
        &to_bytes,
        &value_bytes,
        &data_bytes,
    );

    // Hash and sign
    let mut hasher = Keccak::v256();
    let mut tx_hash = [0u8; 32];
    hasher.update(&tx_for_signing);
    hasher.finalize(&mut tx_hash);

    let signature = sign_with_chain_key_ecdsa(&tx_hash).await?;

    if signature.len() != 64 {
        return Err("Invalid signature length".to_string());
    }
    let r = &signature[..32];
    let s = &signature[32..];

    // Try both recovery IDs
    let mut tx_hash_result: Option<String> = None;
    let mut last_error = String::new();

    for v in [0u8, 1u8] {
        let signed_items = vec![
            rlp_encode_u64(from_chain_id),
            rlp_encode_u64(nonce),
            rlp_encode_u64(max_priority_fee_per_gas),
            rlp_encode_u64(max_fee_per_gas),
            rlp_encode_u64(gas_limit),
            rlp_encode_bytes(&to_bytes),
            rlp_encode_bytes(&value_bytes),
            rlp_encode_bytes(&data_bytes),
            rlp_encode_bytes(&[]), // accessList
            rlp_encode_bytes(&[v]),
            rlp_encode_bytes(r),
            rlp_encode_bytes(s),
        ];

        let signed_rlp = rlp_encode_list(&signed_items);
        let mut raw_tx = vec![0x02u8];
        raw_tx.extend_from_slice(&signed_rlp);

        match send_raw_transaction(&chain_config.rpc_url, &raw_tx).await {
            Ok(hash) => {
                tx_hash_result = Some(hash);
                break;
            }
            Err(e) => last_error = e,
        }
    }

    let tx_hash_result = tx_hash_result.ok_or(last_error)?;

    // Record transaction
    EVM_WALLET_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.tx_counter += 1;
        let tx_id = s.tx_counter;
        let record = EvmTransactionRecord {
            id: tx_id,
            chain_id: from_chain_id,
            tx_hash: Some(tx_hash_result.clone()),
            to: format!("BRIDGE:{}->chain{}", to_token, to_chain_id),
            value_wei: from_amount.clone(),
            data: Some(format!("LiFi bridge to chain {}", to_chain_id)),
            timestamp: ic_cdk::api::time(),
            status: EvmTransactionStatus::Submitted(tx_hash_result.clone()),
        };
        s.transaction_history.push(record);

        if s.transaction_history.len() > 500 {
            s.transaction_history.remove(0);
        }
    });

    ic_cdk::println!("LiFi bridge: {} {} from chain {} to chain {}, tx: {}",
        from_amount, from_token, from_chain_id, to_chain_id, tx_hash_result);

    Ok(tx_hash_result)
}

// ========== Uniswap/DEX Swap ==========

/// Uniswap V3 Quoter2 address (same on most chains)
const UNISWAP_QUOTER_V2: &str = "0x61fFE014bA17989E743c5F6cB21bF9697530B21e";
/// Uniswap V3 SwapRouter02 address
const UNISWAP_ROUTER_V2: &str = "0x68b3465833fb72A70ecDF485E0e4C7bD8665Fc45";

/// DEX swap quote
#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct DexSwapQuote {
    pub chain_id: u64,
    pub token_in: String,
    pub token_out: String,
    pub amount_in: String,
    pub amount_out: String,
    pub price_impact: String,
}

/// Get Uniswap swap quote (via on-chain quoter)
#[update]
async fn get_uniswap_quote(
    chain_id: u64,
    token_in: String,
    token_out: String,
    amount_in: String,
    fee: Option<u32>,
) -> Result<DexSwapQuote, String> {
    let chain_config = EVM_WALLET_STATE.with(|s| {
        s.borrow().configured_chains.iter().find(|c| c.chain_id == chain_id).cloned()
    }).ok_or_else(|| format!("Chain {} not configured", chain_id))?;

    let pool_fee = fee.unwrap_or(3000); // Default 0.3% fee tier
    let amount_bytes = parse_token_amount(&amount_in)?;
    let token_in_bytes = hex_to_bytes(&token_in)?;
    let token_out_bytes = hex_to_bytes(&token_out)?;

    // quoteExactInputSingle((address,address,uint256,uint24,uint160))
    // Selector: 0xc6a5026a
    let mut data = Vec::new();
    data.extend_from_slice(&[0xc6, 0xa5, 0x02, 0x6a]);
    // tokenIn (padded)
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(&token_in_bytes);
    // tokenOut (padded)
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(&token_out_bytes);
    // amountIn
    data.extend_from_slice(&amount_bytes);
    // fee (padded to 32 bytes)
    let mut fee_bytes = [0u8; 32];
    fee_bytes[28..32].copy_from_slice(&pool_fee.to_be_bytes());
    data.extend_from_slice(&fee_bytes);
    // sqrtPriceLimitX96 = 0
    data.extend_from_slice(&[0u8; 32]);

    let data_hex = format!("0x{}", hex::encode(&data));

    let request_body = format!(
        r#"{{"jsonrpc":"2.0","method":"eth_call","params":[{{"to":"{}","data":"{}"}},"latest"],"id":1}}"#,
        UNISWAP_QUOTER_V2, data_hex
    );

    let request = CanisterHttpRequestArgument {
        url: chain_config.rpc_url.clone(),
        max_response_bytes: Some(5000),
        method: HttpMethod::POST,
        headers: vec![HttpHeader {
            name: "Content-Type".to_string(),
            value: "application/json".to_string(),
        }],
        body: Some(request_body.into_bytes()),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_evm_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;
    let (response,): (HttpResponse,) = http_request(request, cycles)
        .await
        .map_err(|(code, msg)| format!("HTTP error: {:?} - {}", code, msg))?;

    let body = String::from_utf8(response.body)
        .map_err(|e| format!("UTF-8 error: {}", e))?;

    // Parse result - returns (amountOut, sqrtPriceX96After, initializedTicksCrossed, gasEstimate)
    if let Some(start) = body.find("\"result\":\"") {
        let start = start + 10;
        if let Some(end) = body[start..].find('"') {
            let hex_result = &body[start..start + end];
            let result_bytes = hex::decode(hex_result.trim_start_matches("0x"))
                .map_err(|e| format!("Hex decode error: {}", e))?;

            if result_bytes.len() >= 32 {
                use num_bigint::BigUint;
                let amount_out = BigUint::from_bytes_be(&result_bytes[0..32]);

                return Ok(DexSwapQuote {
                    chain_id,
                    token_in,
                    token_out,
                    amount_in,
                    amount_out: amount_out.to_string(),
                    price_impact: "N/A".to_string(), // Would need additional calculation
                });
            }
        }
    }

    if body.contains("error") {
        return Err(format!("Quote failed - pool may not exist for this pair: {}", body));
    }

    Err(format!("Failed to parse quote response: {}", body))
}

/// Execute Uniswap swap (Admin only)
#[update]
async fn execute_uniswap_swap(
    chain_id: u64,
    token_in: String,
    token_out: String,
    amount_in: String,
    min_amount_out: String,
    fee: Option<u32>,
) -> Result<String, String> {
    // ========== ADMIN ONLY ==========
    require_admin()?;

    let chain_config = EVM_WALLET_STATE.with(|s| {
        s.borrow().configured_chains.iter().find(|c| c.chain_id == chain_id).cloned()
    }).ok_or_else(|| format!("Chain {} not configured", chain_id))?;

    let from_address = get_evm_address().await?;
    let pool_fee = fee.unwrap_or(3000);

    let amount_in_bytes = parse_token_amount(&amount_in)?;
    let min_out_bytes = parse_token_amount(&min_amount_out)?;
    let token_in_bytes = hex_to_bytes(&token_in)?;
    let token_out_bytes = hex_to_bytes(&token_out)?;
    let recipient_bytes = hex_to_bytes(&from_address)?;

    // Build exactInputSingle call
    // exactInputSingle((address,address,uint24,address,uint256,uint256,uint160))
    // Selector: 0x04e45aaf
    let mut swap_data = Vec::new();
    swap_data.extend_from_slice(&[0x04, 0xe4, 0x5a, 0xaf]);

    // Encode struct parameters (each padded to 32 bytes)
    // tokenIn
    swap_data.extend_from_slice(&[0u8; 12]);
    swap_data.extend_from_slice(&token_in_bytes);
    // tokenOut
    swap_data.extend_from_slice(&[0u8; 12]);
    swap_data.extend_from_slice(&token_out_bytes);
    // fee
    let mut fee_bytes = [0u8; 32];
    fee_bytes[28..32].copy_from_slice(&pool_fee.to_be_bytes());
    swap_data.extend_from_slice(&fee_bytes);
    // recipient
    swap_data.extend_from_slice(&[0u8; 12]);
    swap_data.extend_from_slice(&recipient_bytes);
    // amountIn
    swap_data.extend_from_slice(&amount_in_bytes);
    // amountOutMinimum
    swap_data.extend_from_slice(&min_out_bytes);
    // sqrtPriceLimitX96 = 0
    swap_data.extend_from_slice(&[0u8; 32]);

    // Get nonce and gas price
    let nonce = get_nonce(&chain_config.rpc_url, &from_address).await?;
    let gas_price = get_gas_price(&chain_config.rpc_url).await?;
    let max_fee_per_gas = gas_price.saturating_mul(2);
    let max_priority_fee_per_gas = 2_000_000_000u64;
    let gas_limit = 300_000u64;

    let router_bytes = hex_to_bytes(UNISWAP_ROUTER_V2)?;

    // Build transaction (value = 0 for ERC20 swap)
    let tx_for_signing = build_eip1559_tx_for_signing(
        chain_id,
        nonce,
        max_priority_fee_per_gas,
        max_fee_per_gas,
        gas_limit,
        &router_bytes,
        &[],
        &swap_data,
    );

    // Hash and sign
    let mut hasher = Keccak::v256();
    let mut tx_hash = [0u8; 32];
    hasher.update(&tx_for_signing);
    hasher.finalize(&mut tx_hash);

    let signature = sign_with_chain_key_ecdsa(&tx_hash).await?;

    if signature.len() != 64 {
        return Err("Invalid signature length".to_string());
    }
    let r = &signature[..32];
    let s = &signature[32..];

    // Try both recovery IDs
    let mut tx_hash_result: Option<String> = None;
    let mut last_error = String::new();

    for v in [0u8, 1u8] {
        let signed_items = vec![
            rlp_encode_u64(chain_id),
            rlp_encode_u64(nonce),
            rlp_encode_u64(max_priority_fee_per_gas),
            rlp_encode_u64(max_fee_per_gas),
            rlp_encode_u64(gas_limit),
            rlp_encode_bytes(&router_bytes),
            rlp_encode_bytes(&[]),
            rlp_encode_bytes(&swap_data),
            rlp_encode_bytes(&[]),
            rlp_encode_bytes(&[v]),
            rlp_encode_bytes(r),
            rlp_encode_bytes(s),
        ];

        let signed_rlp = rlp_encode_list(&signed_items);
        let mut raw_tx = vec![0x02u8];
        raw_tx.extend_from_slice(&signed_rlp);

        match send_raw_transaction(&chain_config.rpc_url, &raw_tx).await {
            Ok(hash) => {
                tx_hash_result = Some(hash);
                break;
            }
            Err(e) => last_error = e,
        }
    }

    let tx_hash_result = tx_hash_result.ok_or(last_error)?;

    // Record transaction
    EVM_WALLET_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.tx_counter += 1;
        let tx_id = s.tx_counter;
        let record = EvmTransactionRecord {
            id: tx_id,
            chain_id,
            tx_hash: Some(tx_hash_result.clone()),
            to: format!("SWAP:{}->{}", token_in, token_out),
            value_wei: amount_in.clone(),
            data: Some("Uniswap V3 Swap".to_string()),
            timestamp: ic_cdk::api::time(),
            status: EvmTransactionStatus::Submitted(tx_hash_result.clone()),
        };
        s.transaction_history.push(record);

        if s.transaction_history.len() > 500 {
            s.transaction_history.remove(0);
        }
    });

    ic_cdk::println!("Uniswap swap: {} {} -> {} on chain {}, tx: {}",
        amount_in, token_in, token_out, chain_id, tx_hash_result);

    Ok(tx_hash_result)
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

// ========== Solana Wallet (Ed25519) ==========

use ed25519_dalek::{SigningKey, Signer, Signature};

/// Custom getrandom implementation for IC
/// This is required because getrandom doesn't support wasm32-unknown-unknown by default
#[cfg(target_arch = "wasm32")]
mod ic_random {
    use getrandom::register_custom_getrandom;

    fn ic_getrandom(buf: &mut [u8]) -> Result<(), getrandom::Error> {
        // Use ic_cdk::api::management_canister::main::raw_rand for true randomness
        // For now, use a deterministic seed based on time (NOT secure for production)
        // Production should use async raw_rand call
        let seed = ic_cdk::api::time();
        for (i, byte) in buf.iter_mut().enumerate() {
            *byte = ((seed >> (i % 8 * 8)) & 0xff) as u8 ^ (i as u8);
        }
        Ok(())
    }

    register_custom_getrandom!(ic_getrandom);
}

/// XOR encryption/decryption for secret key (placeholder for vetKeys)
/// In production, replace with vetKeys encryption
fn xor_encrypt_decrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    data.iter()
        .zip(key.iter().cycle())
        .map(|(d, k)| d ^ k)
        .collect()
}

/// Get encryption key derived from canister ID (placeholder for vetKeys)
fn get_encryption_key() -> Vec<u8> {
    let canister_id = ic_cdk::id();
    let mut key = Vec::with_capacity(32);
    let id_bytes = canister_id.as_slice();
    // Extend to 32 bytes
    for i in 0..32 {
        key.push(id_bytes[i % id_bytes.len()] ^ (i as u8));
    }
    key
}

/// Initialize Solana wallet with a new Ed25519 keypair (Admin only)
#[update]
async fn init_solana_wallet() -> Result<String, String> {
    require_admin()?;

    // Check if already initialized
    let already_initialized = SOLANA_WALLET_STATE.with(|s| s.borrow().initialized);
    if already_initialized {
        return Err("Solana wallet already initialized. Use reset_solana_wallet to reinitialize.".to_string());
    }

    // Generate random bytes using IC's raw_rand for true randomness
    let (random_bytes,): (Vec<u8>,) = ic_cdk::api::management_canister::main::raw_rand()
        .await
        .map_err(|(code, msg)| format!("Failed to get random bytes: {:?} - {}", code, msg))?;

    if random_bytes.len() < 32 {
        return Err("Insufficient random bytes".to_string());
    }

    // Create Ed25519 signing key from random bytes
    let secret_key_bytes: [u8; 32] = random_bytes[..32].try_into()
        .map_err(|_| "Failed to convert random bytes")?;

    let signing_key = SigningKey::from_bytes(&secret_key_bytes);
    let verifying_key = signing_key.verifying_key();
    let public_key_bytes = verifying_key.to_bytes();

    // Encrypt secret key for storage
    let encryption_key = get_encryption_key();
    let encrypted_secret = xor_encrypt_decrypt(&secret_key_bytes, &encryption_key);

    // Derive Solana address (Base58 encoded public key)
    let address = bs58::encode(&public_key_bytes).into_string();

    // Store in state
    SOLANA_WALLET_STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.initialized = true;
        state.public_key = Some(public_key_bytes.to_vec());
        state.encrypted_secret_key = Some(encrypted_secret);
        state.cached_address = Some(address.clone());
    });

    ic_cdk::println!("Solana wallet initialized: {}", address);
    Ok(address)
}

/// Get Solana wallet address
#[query]
fn get_solana_address() -> Result<String, String> {
    SOLANA_WALLET_STATE.with(|s| {
        let state = s.borrow();
        state.cached_address.clone()
            .ok_or_else(|| "Solana wallet not initialized. Call init_solana_wallet first.".to_string())
    })
}

/// Get Solana wallet info
#[query]
fn get_solana_wallet_info(network: String) -> Result<SolanaWalletInfo, String> {
    let address = get_solana_address()?;

    Ok(SolanaWalletInfo {
        address,
        network,
    })
}

/// Configure a Solana network (Admin only)
#[update]
fn configure_solana_network(config: SolanaNetworkConfig) -> Result<(), String> {
    require_admin()?;

    SOLANA_WALLET_STATE.with(|s| {
        let mut state = s.borrow_mut();
        // Update or add network config
        if let Some(existing) = state.configured_networks.iter_mut()
            .find(|n| n.network_name == config.network_name) {
            *existing = config;
        } else {
            // Limit to 5 networks max
            if state.configured_networks.len() >= 5 {
                return Err("Maximum 5 networks allowed".to_string());
            }
            state.configured_networks.push(config);
        }
        Ok(())
    })
}

/// Get configured Solana networks
#[query]
fn get_solana_networks() -> Vec<SolanaNetworkConfig> {
    SOLANA_WALLET_STATE.with(|s| s.borrow().configured_networks.clone())
}

/// Transform function for Solana RPC responses
#[query]
fn transform_solana_response(raw: TransformArgs) -> HttpResponse {
    HttpResponse {
        status: raw.response.status,
        body: raw.response.body,
        headers: vec![],
    }
}

/// Get SOL balance from Solana RPC
#[update]
async fn get_solana_balance(network_name: String) -> Result<u64, String> {
    let network_config = SOLANA_WALLET_STATE.with(|s| {
        s.borrow().configured_networks.iter()
            .find(|n| n.network_name == network_name)
            .cloned()
    }).ok_or_else(|| format!("Network '{}' not configured", network_name))?;

    let address = get_solana_address()?;

    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getBalance",
        "params": [address]
    });

    let request = CanisterHttpRequestArgument {
        url: network_config.rpc_url.clone(),
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
                method: "transform_solana_response".to_string(),
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
                .map_err(|e| format!("JSON error: {} - Body: {}", e, body))?;

            if let Some(error) = json.get("error") {
                return Err(format!("Solana RPC error: {}", error));
            }

            json["result"]["value"]
                .as_u64()
                .ok_or_else(|| format!("No balance in response: {}", body))
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

/// Get recent blockhash from Solana RPC
async fn get_recent_blockhash(rpc_url: &str) -> Result<String, String> {
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getLatestBlockhash",
        "params": []
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
                method: "transform_solana_response".to_string(),
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

            json["result"]["value"]["blockhash"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "No blockhash in response".to_string())
        }
        Err((code, msg)) => Err(format!("HTTP error: {:?} - {}", code, msg)),
    }
}

/// Build a Solana transfer transaction (system program transfer)
fn build_solana_transfer_tx(
    from_pubkey: &[u8; 32],
    to_pubkey: &[u8; 32],
    lamports: u64,
    recent_blockhash: &[u8; 32],
) -> Vec<u8> {
    // Solana transaction format (simplified):
    // 1. Number of signatures (1 byte)
    // 2. Signatures (64 bytes each)
    // 3. Message:
    //    - Header (3 bytes: num_required_signatures, num_readonly_signed, num_readonly_unsigned)
    //    - Account addresses (32 bytes each)
    //    - Recent blockhash (32 bytes)
    //    - Instructions

    let system_program_id: [u8; 32] = [0u8; 32]; // System program is all zeros

    // Build compact message (without signature space - we'll add that after signing)
    let mut message = Vec::new();

    // Message header
    message.push(1u8);  // num_required_signatures
    message.push(0u8);  // num_readonly_signed_accounts
    message.push(1u8);  // num_readonly_unsigned_accounts (system program)

    // Number of account keys
    message.push(3u8);  // from, to, system_program

    // Account addresses (in order: from, to, system_program)
    message.extend_from_slice(from_pubkey);
    message.extend_from_slice(to_pubkey);
    message.extend_from_slice(&system_program_id);

    // Recent blockhash
    message.extend_from_slice(recent_blockhash);

    // Number of instructions
    message.push(1u8);

    // Instruction: System Program Transfer
    message.push(2u8);  // program_id_index (system program at index 2)
    message.push(2u8);  // num_accounts
    message.push(0u8);  // from account index (writable, signer)
    message.push(1u8);  // to account index (writable)

    // Instruction data: transfer instruction (4 bytes type + 8 bytes amount)
    let mut instruction_data = Vec::new();
    instruction_data.extend_from_slice(&2u32.to_le_bytes()); // Transfer instruction type
    instruction_data.extend_from_slice(&lamports.to_le_bytes());

    message.push(instruction_data.len() as u8);
    message.extend_from_slice(&instruction_data);

    message
}

/// Sign a message with the Solana Ed25519 key
fn sign_solana_message(message: &[u8]) -> Result<Vec<u8>, String> {
    // Get and decrypt secret key
    let (encrypted_secret, _public_key) = SOLANA_WALLET_STATE.with(|s| {
        let state = s.borrow();
        (
            state.encrypted_secret_key.clone(),
            state.public_key.clone(),
        )
    });

    let encrypted_secret = encrypted_secret
        .ok_or_else(|| "Solana wallet not initialized".to_string())?;

    let encryption_key = get_encryption_key();
    let secret_bytes = xor_encrypt_decrypt(&encrypted_secret, &encryption_key);

    if secret_bytes.len() != 32 {
        return Err("Invalid secret key length".to_string());
    }

    let secret_array: [u8; 32] = secret_bytes.try_into()
        .map_err(|_| "Failed to convert secret key")?;

    let signing_key = SigningKey::from_bytes(&secret_array);
    let signature: Signature = signing_key.sign(message);

    // Clear secret from memory (Rust will drop, but explicit for clarity)
    drop(signing_key);

    Ok(signature.to_bytes().to_vec())
}

/// Send SOL to another address (Admin only)
#[update]
async fn send_solana(
    network_name: String,
    to_address: String,
    amount_lamports: u64,
) -> Result<String, String> {
    // ========== ADMIN ONLY ==========
    require_admin()?;

    // Validate amount
    if amount_lamports < 5000 {
        return Err("Amount too small. Minimum is 5000 lamports (for rent exemption)".to_string());
    }

    // Get network config
    let network_config = SOLANA_WALLET_STATE.with(|s| {
        s.borrow().configured_networks.iter()
            .find(|n| n.network_name == network_name)
            .cloned()
    }).ok_or_else(|| format!("Network '{}' not configured", network_name))?;

    // Get our public key
    let from_pubkey = SOLANA_WALLET_STATE.with(|s| {
        s.borrow().public_key.clone()
    }).ok_or_else(|| "Solana wallet not initialized".to_string())?;

    let from_pubkey_array: [u8; 32] = from_pubkey.try_into()
        .map_err(|_| "Invalid public key")?;

    // Parse destination address
    let to_pubkey_bytes = bs58::decode(&to_address)
        .into_vec()
        .map_err(|e| format!("Invalid destination address: {:?}", e))?;

    if to_pubkey_bytes.len() != 32 {
        return Err("Invalid destination address length".to_string());
    }
    let to_pubkey_array: [u8; 32] = to_pubkey_bytes.try_into()
        .map_err(|_| "Invalid destination address")?;

    // Get recent blockhash
    let blockhash_str = get_recent_blockhash(&network_config.rpc_url).await?;
    let blockhash_bytes = bs58::decode(&blockhash_str)
        .into_vec()
        .map_err(|e| format!("Invalid blockhash: {:?}", e))?;
    let blockhash_array: [u8; 32] = blockhash_bytes.try_into()
        .map_err(|_| "Invalid blockhash length")?;

    // Build transaction message
    let message = build_solana_transfer_tx(
        &from_pubkey_array,
        &to_pubkey_array,
        amount_lamports,
        &blockhash_array,
    );

    // Sign the message
    let signature = sign_solana_message(&message)?;

    // Build full transaction (signatures + message)
    let mut transaction = Vec::new();
    transaction.push(1u8); // Number of signatures
    transaction.extend_from_slice(&signature);
    transaction.extend_from_slice(&message);

    // Encode transaction for RPC
    let tx_base64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &transaction
    );

    // Send transaction
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendTransaction",
        "params": [
            tx_base64,
            {
                "encoding": "base64",
                "skipPreflight": false,
                "preflightCommitment": "confirmed"
            }
        ]
    });

    let request = CanisterHttpRequestArgument {
        url: network_config.rpc_url.clone(),
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
                method: "transform_solana_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    let tx_signature = match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("JSON error: {} - Body: {}", e, body))?;

            if let Some(error) = json.get("error") {
                return Err(format!("Solana RPC error: {}", error));
            }

            json["result"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("No signature in response: {}", body))?
        }
        Err((code, msg)) => return Err(format!("HTTP error: {:?} - {}", code, msg)),
    };

    // Record transaction
    SOLANA_WALLET_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.tx_counter += 1;
        let tx_record = SolanaTransactionRecord {
            id: s.tx_counter,
            signature: Some(tx_signature.clone()),
            to: to_address.clone(),
            amount_lamports,
            timestamp: ic_cdk::api::time(),
            status: SolanaTransactionStatus::Submitted(tx_signature.clone()),
        };
        s.transaction_history.push(tx_record);

        // Limit history to 500
        if s.transaction_history.len() > 500 {
            s.transaction_history.remove(0);
        }
    });

    ic_cdk::println!("Solana transfer submitted: {} lamports to {}, sig: {}",
        amount_lamports, to_address, tx_signature);
    Ok(tx_signature)
}

/// SPL Token Program ID
const SPL_TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";
/// Associated Token Program ID
const SPL_ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

/// Send SPL tokens (Admin only)
/// Parameters: network_name, token_mint_address, to_address, amount (in smallest units)
#[update]
async fn send_spl_token(
    network_name: String,
    token_mint: String,
    to_address: String,
    amount: u64,
) -> Result<String, String> {
    // ========== ADMIN ONLY ==========
    require_admin()?;

    if amount == 0 {
        return Err("Amount must be greater than 0".to_string());
    }

    // Get network config
    let network_config = SOLANA_WALLET_STATE.with(|s| {
        s.borrow().configured_networks.iter()
            .find(|n| n.network_name == network_name)
            .cloned()
    }).ok_or_else(|| format!("Network '{}' not configured", network_name))?;

    // Get our public key
    let from_pubkey = SOLANA_WALLET_STATE.with(|s| {
        s.borrow().public_key.clone()
    }).ok_or_else(|| "Solana wallet not initialized".to_string())?;

    let from_pubkey_array: [u8; 32] = from_pubkey.try_into()
        .map_err(|_| "Invalid public key")?;

    // Parse addresses
    let mint_pubkey = decode_solana_pubkey(&token_mint)?;
    let to_pubkey = decode_solana_pubkey(&to_address)?;
    let token_program_id = decode_solana_pubkey(SPL_TOKEN_PROGRAM_ID)?;

    // Derive Associated Token Accounts
    let from_ata = derive_associated_token_account(&from_pubkey_array, &mint_pubkey)?;
    let to_ata = derive_associated_token_account(&to_pubkey, &mint_pubkey)?;

    // Get recent blockhash
    let blockhash_str = get_recent_blockhash(&network_config.rpc_url).await?;
    let blockhash = decode_solana_pubkey(&blockhash_str)?;

    // Build SPL token transfer message
    let message = build_spl_transfer_message(
        &from_pubkey_array,
        &from_ata,
        &to_ata,
        &token_program_id,
        amount,
        &blockhash,
    );

    // Sign the message
    let signature = sign_solana_message(&message)?;

    // Build full transaction
    let mut transaction = Vec::new();
    transaction.push(1u8); // Number of signatures
    transaction.extend_from_slice(&signature);
    transaction.extend_from_slice(&message);

    // Encode and send
    let tx_base64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &transaction
    );

    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendTransaction",
        "params": [
            tx_base64,
            {
                "encoding": "base64",
                "skipPreflight": false,
                "preflightCommitment": "confirmed"
            }
        ]
    });

    let request = CanisterHttpRequestArgument {
        url: network_config.rpc_url.clone(),
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
                method: "transform_solana_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    let tx_signature = match http_request(request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("JSON error: {} - Body: {}", e, body))?;

            if let Some(error) = json.get("error") {
                return Err(format!("Solana RPC error: {}", error));
            }

            json["result"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("No signature in response: {}", body))?
        }
        Err((code, msg)) => return Err(format!("HTTP error: {:?} - {}", code, msg)),
    };

    // Record transaction (reusing SolanaTransactionRecord with SPL info in signature field)
    SOLANA_WALLET_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.tx_counter += 1;
        let tx_record = SolanaTransactionRecord {
            id: s.tx_counter,
            signature: Some(format!("SPL:{}:{}", token_mint, tx_signature)),
            to: to_address.clone(),
            amount_lamports: amount, // For SPL this is token amount, not lamports
            timestamp: ic_cdk::api::time(),
            status: SolanaTransactionStatus::Submitted(tx_signature.clone()),
        };
        s.transaction_history.push(tx_record);

        if s.transaction_history.len() > 500 {
            s.transaction_history.remove(0);
        }
    });

    ic_cdk::println!("SPL transfer: {} {} to {}, sig: {}", amount, token_mint, to_address, tx_signature);
    Ok(tx_signature)
}

/// Decode a base58-encoded Solana public key
fn decode_solana_pubkey(address: &str) -> Result<[u8; 32], String> {
    let bytes = bs58::decode(address)
        .into_vec()
        .map_err(|e| format!("Invalid address '{}': {:?}", address, e))?;

    if bytes.len() != 32 {
        return Err(format!("Invalid address length: {} (expected 32)", bytes.len()));
    }

    bytes.try_into().map_err(|_| "Address conversion error".to_string())
}

/// Derive Associated Token Account address
fn derive_associated_token_account(wallet: &[u8; 32], mint: &[u8; 32]) -> Result<[u8; 32], String> {
    // ATA = PDA of [wallet, token_program, mint] with associated_token_program
    // Simplified derivation using SHA256 (note: actual Solana uses find_program_address)

    let ata_program = decode_solana_pubkey(SPL_ASSOCIATED_TOKEN_PROGRAM_ID)?;
    let token_program = decode_solana_pubkey(SPL_TOKEN_PROGRAM_ID)?;

    // Seeds: [wallet_address, token_program_id, mint_address]
    let mut hasher = Sha256::new();
    hasher.update(wallet);
    hasher.update(&token_program);
    hasher.update(mint);
    hasher.update(&ata_program);
    hasher.update(b"ProgramDerivedAddress"); // Standard suffix

    let hash = hasher.finalize();
    let mut result = [0u8; 32];
    result.copy_from_slice(&hash[..32]);

    // Note: This is a simplified derivation. For production, use proper PDA derivation
    // with bump seed finding
    Ok(result)
}

/// Build SPL token transfer message
fn build_spl_transfer_message(
    owner: &[u8; 32],
    from_ata: &[u8; 32],
    to_ata: &[u8; 32],
    token_program: &[u8; 32],
    amount: u64,
    recent_blockhash: &[u8; 32],
) -> Vec<u8> {
    let mut message = Vec::new();

    // Message header
    message.push(1); // num_required_signatures
    message.push(0); // num_readonly_signed_accounts
    message.push(1); // num_readonly_unsigned_accounts (token program)

    // Account addresses (4 accounts)
    message.push(4); // Number of accounts
    message.extend_from_slice(owner);       // 0: owner (signer)
    message.extend_from_slice(from_ata);    // 1: source ATA
    message.extend_from_slice(to_ata);      // 2: destination ATA
    message.extend_from_slice(token_program); // 3: token program (readonly)

    // Recent blockhash
    message.extend_from_slice(recent_blockhash);

    // Instructions (1 instruction: SPL Token Transfer)
    message.push(1); // Number of instructions

    // SPL Token Transfer instruction
    message.push(3); // program_id_index (token program)
    message.push(3); // number of accounts for this instruction
    message.push(1); // source ATA index
    message.push(2); // destination ATA index
    message.push(0); // owner index

    // Instruction data: transfer instruction (3 = transfer, then u64 amount)
    message.push(9); // data length
    message.push(3); // Transfer instruction discriminator
    message.extend_from_slice(&amount.to_le_bytes()); // amount as u64 little-endian

    message
}

/// Get SPL token balance
#[update]
async fn get_spl_token_balance(
    network_name: String,
    token_mint: String,
    wallet_address: Option<String>,
) -> Result<String, String> {
    let network_config = SOLANA_WALLET_STATE.with(|s| {
        s.borrow().configured_networks.iter()
            .find(|n| n.network_name == network_name)
            .cloned()
    }).ok_or_else(|| format!("Network '{}' not configured", network_name))?;

    let wallet = match wallet_address {
        Some(addr) => decode_solana_pubkey(&addr)?,
        None => {
            let pubkey = SOLANA_WALLET_STATE.with(|s| s.borrow().public_key.clone())
                .ok_or("Wallet not initialized")?;
            pubkey.try_into().map_err(|_| "Invalid public key")?
        }
    };

    let mint = decode_solana_pubkey(&token_mint)?;
    let ata = derive_associated_token_account(&wallet, &mint)?;
    let ata_address = bs58::encode(&ata).into_string();

    // Query token account balance
    let request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getTokenAccountBalance",
        "params": [ata_address]
    });

    let request = CanisterHttpRequestArgument {
        url: network_config.rpc_url.clone(),
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
                method: "transform_solana_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 30_000_000_000u128;

    let (response,): (HttpResponse,) = http_request(request, cycles)
        .await
        .map_err(|(code, msg)| format!("HTTP error: {:?} - {}", code, msg))?;

    let body = String::from_utf8(response.body)
        .map_err(|e| format!("UTF-8 error: {}", e))?;

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("JSON error: {}", e))?;

    if let Some(error) = json.get("error") {
        // Account might not exist
        if error.to_string().contains("could not find") {
            return Ok("0".to_string());
        }
        return Err(format!("RPC error: {}", error));
    }

    json["result"]["value"]["amount"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| format!("Failed to parse balance: {}", body))
}

// ========== Jupiter Swap Integration ==========

/// Jupiter Quote API endpoint
const JUPITER_QUOTE_API: &str = "https://quote-api.jup.ag/v6/quote";
/// Jupiter Swap API endpoint
const JUPITER_SWAP_API: &str = "https://quote-api.jup.ag/v6/swap";

/// Jupiter swap quote response
#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct JupiterQuote {
    pub input_mint: String,
    pub output_mint: String,
    pub in_amount: String,
    pub out_amount: String,
    pub price_impact_pct: String,
    pub slippage_bps: u64,
}

/// Get Jupiter swap quote
#[update]
async fn get_jupiter_quote(
    input_mint: String,
    output_mint: String,
    amount: u64,
    slippage_bps: Option<u64>,
) -> Result<JupiterQuote, String> {
    let slippage = slippage_bps.unwrap_or(50); // Default 0.5% slippage

    let url = format!(
        "{}?inputMint={}&outputMint={}&amount={}&slippageBps={}",
        JUPITER_QUOTE_API, input_mint, output_mint, amount, slippage
    );

    let request = CanisterHttpRequestArgument {
        url,
        max_response_bytes: Some(10_000),
        method: HttpMethod::GET,
        headers: vec![],
        body: None,
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_solana_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    let (response,): (HttpResponse,) = http_request(request, cycles)
        .await
        .map_err(|(code, msg)| format!("HTTP error: {:?} - {}", code, msg))?;

    let body = String::from_utf8(response.body)
        .map_err(|e| format!("UTF-8 error: {}", e))?;

    let json: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("JSON error: {} - Body: {}", e, body))?;

    if let Some(error) = json.get("error") {
        return Err(format!("Jupiter API error: {}", error));
    }

    let out_amount = json["outAmount"]
        .as_str()
        .unwrap_or("0")
        .to_string();

    let price_impact = json["priceImpactPct"]
        .as_str()
        .unwrap_or("0")
        .to_string();

    Ok(JupiterQuote {
        input_mint,
        output_mint,
        in_amount: amount.to_string(),
        out_amount,
        price_impact_pct: price_impact,
        slippage_bps: slippage,
    })
}

/// Execute Jupiter swap (Admin only)
/// Parameters: network_name, input_mint, output_mint, amount, slippage_bps
#[update]
async fn execute_jupiter_swap(
    network_name: String,
    input_mint: String,
    output_mint: String,
    amount: u64,
    slippage_bps: Option<u64>,
) -> Result<String, String> {
    // ========== ADMIN ONLY ==========
    require_admin()?;

    // Get network config
    let network_config = SOLANA_WALLET_STATE.with(|s| {
        s.borrow().configured_networks.iter()
            .find(|n| n.network_name == network_name)
            .cloned()
    }).ok_or_else(|| format!("Network '{}' not configured", network_name))?;

    // Only allow mainnet for Jupiter
    if network_name != "mainnet" {
        return Err("Jupiter swaps only available on mainnet".to_string());
    }

    // Get our wallet address
    let wallet_address = get_solana_address()?;

    let slippage = slippage_bps.unwrap_or(50);

    // Step 1: Get quote
    let quote_url = format!(
        "{}?inputMint={}&outputMint={}&amount={}&slippageBps={}",
        JUPITER_QUOTE_API, input_mint, output_mint, amount, slippage
    );

    let quote_request = CanisterHttpRequestArgument {
        url: quote_url,
        max_response_bytes: Some(20_000),
        method: HttpMethod::GET,
        headers: vec![],
        body: None,
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_solana_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let cycles = 50_000_000_000u128;

    let (quote_response,): (HttpResponse,) = http_request(quote_request, cycles)
        .await
        .map_err(|(code, msg)| format!("Quote HTTP error: {:?} - {}", code, msg))?;

    let quote_body = String::from_utf8(quote_response.body)
        .map_err(|e| format!("Quote UTF-8 error: {}", e))?;

    let quote_json: serde_json::Value = serde_json::from_str(&quote_body)
        .map_err(|e| format!("Quote JSON error: {}", e))?;

    if let Some(error) = quote_json.get("error") {
        return Err(format!("Jupiter quote error: {}", error));
    }

    // Step 2: Get swap transaction
    let swap_request_body = serde_json::json!({
        "quoteResponse": quote_json,
        "userPublicKey": wallet_address,
        "wrapAndUnwrapSol": true,
        "dynamicComputeUnitLimit": true,
        "prioritizationFeeLamports": "auto"
    });

    let swap_request = CanisterHttpRequestArgument {
        url: JUPITER_SWAP_API.to_string(),
        max_response_bytes: Some(50_000),
        method: HttpMethod::POST,
        headers: vec![
            HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            },
        ],
        body: Some(swap_request_body.to_string().into_bytes()),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_solana_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let (swap_response,): (HttpResponse,) = http_request(swap_request, cycles)
        .await
        .map_err(|(code, msg)| format!("Swap HTTP error: {:?} - {}", code, msg))?;

    let swap_body = String::from_utf8(swap_response.body)
        .map_err(|e| format!("Swap UTF-8 error: {}", e))?;

    let swap_json: serde_json::Value = serde_json::from_str(&swap_body)
        .map_err(|e| format!("Swap JSON error: {}", e))?;

    if let Some(error) = swap_json.get("error") {
        return Err(format!("Jupiter swap error: {}", error));
    }

    // Get the serialized transaction
    let swap_tx_base64 = swap_json["swapTransaction"]
        .as_str()
        .ok_or("No swap transaction in response")?;

    // Decode the transaction
    let tx_bytes = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        swap_tx_base64
    ).map_err(|e| format!("Base64 decode error: {}", e))?;

    // Jupiter returns a versioned transaction that needs to be signed
    // The transaction message is after the signatures section
    // For versioned transactions: [num_signatures][signatures...][message]

    if tx_bytes.is_empty() {
        return Err("Empty transaction".to_string());
    }

    let num_signatures = tx_bytes[0] as usize;
    let signature_section_len = 1 + (num_signatures * 64);

    if tx_bytes.len() < signature_section_len {
        return Err("Transaction too short".to_string());
    }

    // Extract the message portion (everything after signatures)
    let message = &tx_bytes[signature_section_len..];

    // Sign the message with our key
    let signature = sign_solana_message(message)?;

    // Reconstruct the transaction with our signature
    let mut signed_tx = Vec::new();
    signed_tx.push(1u8); // We're the only signer needed
    signed_tx.extend_from_slice(&signature);
    signed_tx.extend_from_slice(message);

    // Encode and send
    let signed_tx_base64 = base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        &signed_tx
    );

    let send_request_body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "sendTransaction",
        "params": [
            signed_tx_base64,
            {
                "encoding": "base64",
                "skipPreflight": false,
                "preflightCommitment": "confirmed",
                "maxRetries": 3
            }
        ]
    });

    let send_request = CanisterHttpRequestArgument {
        url: network_config.rpc_url.clone(),
        max_response_bytes: Some(2_000),
        method: HttpMethod::POST,
        headers: vec![
            HttpHeader {
                name: "Content-Type".to_string(),
                value: "application/json".to_string(),
            },
        ],
        body: Some(send_request_body.to_string().into_bytes()),
        transform: Some(TransformContext {
            function: TransformFunc(candid::Func {
                principal: ic_cdk::id(),
                method: "transform_solana_response".to_string(),
            }),
            context: vec![],
        }),
    };

    let tx_signature = match http_request(send_request, cycles).await {
        Ok((response,)) => {
            let body = String::from_utf8(response.body)
                .map_err(|e| format!("UTF-8 error: {}", e))?;

            let json: serde_json::Value = serde_json::from_str(&body)
                .map_err(|e| format!("JSON error: {} - Body: {}", e, body))?;

            if let Some(error) = json.get("error") {
                return Err(format!("Solana RPC error: {}", error));
            }

            json["result"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("No signature in response: {}", body))?
        }
        Err((code, msg)) => return Err(format!("HTTP error: {:?} - {}", code, msg)),
    };

    // Record transaction
    let out_amount = quote_json["outAmount"].as_str().unwrap_or("0").to_string();

    SOLANA_WALLET_STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.tx_counter += 1;
        let tx_record = SolanaTransactionRecord {
            id: s.tx_counter,
            signature: Some(format!("SWAP:{}->{}:{}", input_mint, output_mint, tx_signature)),
            to: format!("Jupiter:{}->{}", input_mint, output_mint),
            amount_lamports: amount,
            timestamp: ic_cdk::api::time(),
            status: SolanaTransactionStatus::Submitted(tx_signature.clone()),
        };
        s.transaction_history.push(tx_record);

        if s.transaction_history.len() > 500 {
            s.transaction_history.remove(0);
        }
    });

    ic_cdk::println!("Jupiter swap: {} {} -> {} {}, sig: {}",
        amount, input_mint, out_amount, output_mint, tx_signature);

    Ok(tx_signature)
}

/// Get Solana transaction history
#[query]
fn get_solana_transaction_history(limit: Option<u32>) -> Vec<SolanaTransactionRecord> {
    let limit = limit.unwrap_or(50) as usize;

    SOLANA_WALLET_STATE.with(|state| {
        let s = state.borrow();
        s.transaction_history
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    })
}

/// Reset Solana wallet (Admin only) - WARNING: This destroys the current wallet
#[update]
fn reset_solana_wallet() -> Result<(), String> {
    require_admin()?;

    SOLANA_WALLET_STATE.with(|s| {
        let mut state = s.borrow_mut();
        state.initialized = false;
        state.public_key = None;
        state.encrypted_secret_key = None;
        state.cached_address = None;
        // Keep transaction history and networks
    });

    Ok(())
}

// ========== Portfolio Analysis ==========

/// Asset information for portfolio
#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct PortfolioAsset {
    pub chain: String,
    pub symbol: String,
    pub address: String,
    pub balance: String,
    pub token_address: Option<String>,
}

/// Full portfolio overview
#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct Portfolio {
    pub icp: PortfolioAsset,
    pub evm_assets: Vec<PortfolioAsset>,
    pub solana_assets: Vec<PortfolioAsset>,
    pub total_chains: u32,
    pub last_updated: u64,
}

/// Get complete portfolio overview
#[update]
async fn get_portfolio() -> Result<Portfolio, String> {
    let now = ic_cdk::api::time();

    // ICP Balance
    let icp_address = get_wallet_address();
    let icp_balance = match check_icp_balance().await {
        Ok(balance) => balance.to_string(),
        Err(_) => "0".to_string(),
    };

    let icp_asset = PortfolioAsset {
        chain: "ICP".to_string(),
        symbol: "ICP".to_string(),
        address: icp_address,
        balance: icp_balance,
        token_address: None,
    };

    // EVM Balances
    let mut evm_assets = Vec::new();
    let evm_address = match get_evm_address().await {
        Ok(addr) => addr,
        Err(_) => String::new(),
    };

    if !evm_address.is_empty() {
        let configured_chains: Vec<EvmChainConfig> = EVM_WALLET_STATE.with(|s| {
            s.borrow().configured_chains.clone()
        });

        for chain in configured_chains.iter() {
            let balance = match get_evm_balance(chain.chain_id).await {
                Ok(b) => b,
                Err(_) => "0".to_string(),
            };

            evm_assets.push(PortfolioAsset {
                chain: chain.chain_name.clone(),
                symbol: chain.native_symbol.clone(),
                address: evm_address.clone(),
                balance,
                token_address: None,
            });
        }
    }

    // Solana Balance
    let mut solana_assets = Vec::new();
    let solana_address = match get_solana_address() {
        Ok(addr) => addr,
        Err(_) => String::new(),
    };

    if !solana_address.is_empty() {
        let configured_networks: Vec<SolanaNetworkConfig> = SOLANA_WALLET_STATE.with(|s| {
            s.borrow().configured_networks.clone()
        });

        for network in configured_networks.iter() {
            if network.network_name == "mainnet" {
                let balance = match get_solana_balance(network.network_name.clone()).await {
                    Ok(b) => b.to_string(),
                    Err(_) => "0".to_string(),
                };

                solana_assets.push(PortfolioAsset {
                    chain: "Solana".to_string(),
                    symbol: "SOL".to_string(),
                    address: solana_address.clone(),
                    balance,
                    token_address: None,
                });
                break;
            }
        }
    }

    let total_chains = 1 + evm_assets.len() as u32 + if solana_assets.is_empty() { 0 } else { 1 };

    Ok(Portfolio {
        icp: icp_asset,
        evm_assets,
        solana_assets,
        total_chains,
        last_updated: now,
    })
}

/// Get wallet addresses summary
#[query]
fn get_wallet_addresses() -> Vec<(String, String)> {
    let mut addresses = Vec::new();

    // ICP
    let icp_address = get_wallet_address();
    addresses.push(("ICP".to_string(), icp_address));

    // EVM
    if let Some(evm_address) = EVM_WALLET_STATE.with(|s| s.borrow().cached_address.clone()) {
        addresses.push(("EVM".to_string(), evm_address));
    }

    // Solana
    if let Some(sol_address) = SOLANA_WALLET_STATE.with(|s| s.borrow().cached_address.clone()) {
        addresses.push(("Solana".to_string(), sol_address));
    }

    addresses
}

// Candid export
ic_cdk::export_candid!();
