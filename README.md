# Coo - Fully On-Chain AI Agent

A fully decentralized AI agent running on the Internet Computer blockchain, powered by the elizaOS framework and IC LLM Canister (Llama 3.1 8B).

## Live Demo

**Frontend:** https://4res3-liaaa-aaaas-qdqcq-cai.icp0.io/

**Backend Candid UI:** https://a4gq6-oaaaa-aaaab-qaa4q-cai.raw.icp0.io/?id=4wfup-gqaaa-aaaas-qdqca-cai

## Features

- **Fully On-Chain AI**: Uses IC LLM Canister (Llama 3.1 8B) for AI responses
- **Decentralized**: Runs entirely on Internet Computer blockchain
- **Censorship-Resistant**: No centralized servers or API dependencies
- **Internet Identity Authentication**: Secure user authentication
- **Conversation Memory**: Maintains context across conversations
- **Social Integration**: Twitter and Discord posting with auto-reply capabilities
- **ICP Wallet**: Native ICP wallet with balance checking and transfer capabilities
- **EVM Wallet**: Multi-chain EVM wallet via Chain-Key ECDSA (Base, Polygon, etc.)
- **elizaOS Framework**: Built on the leading open-source AI agent framework

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     Internet Computer Mainnet                    │
│                                                                  │
│  ┌─────────────────┐    ┌──────────────────────────────────┐   │
│  │    Frontend     │    │        Backend Canister          │   │
│  │    Canister     │◄──►│      (Rust + ic-llm)            │   │
│  │    (React)      │    │                                  │   │
│  │                 │    │  ┌────────────────────────────┐  │   │
│  │ 4res3-liaaa-... │    │  │   IC LLM Canister          │  │   │
│  └─────────────────┘    │  │   (Llama 3.1 8B)           │  │   │
│                         │  │   w36hm-eqaaa-aaaal-qr76a  │  │   │
│  ┌─────────────────┐    │  └────────────────────────────┘  │   │
│  │    Internet     │    │                                  │   │
│  │    Identity     │◄──►│  ┌────────────────────────────┐  │   │
│  └─────────────────┘    │  │   ICP Ledger               │  │   │
│                         │  │   ryjl3-tyaaa-aaaaa-aaaba  │  │   │
│  ┌─────────────────┐    │  └────────────────────────────┘  │   │
│  │   Twitter API   │◄──►│                                  │   │
│  │   Discord API   │    │  4wfup-gqaaa-aaaas-qdqca-cai   │   │
│  └─────────────────┘    └──────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## Canister IDs

| Canister | Mainnet ID |
|----------|------------|
| Backend | `4wfup-gqaaa-aaaas-qdqca-cai` |
| Frontend | `4res3-liaaa-aaaas-qdqcq-cai` |

## Prerequisites

- [dfx](https://internetcomputer.org/docs/current/developer-docs/setup/install) (IC SDK v0.12+)
- [Rust](https://rustup.rs/) with `wasm32-unknown-unknown` target
- [Node.js](https://nodejs.org/) (v18+)

## Quick Start

### 1. Install Dependencies

```bash
# Install dfx
sh -ci "$(curl -fsSL https://internetcomputer.org/install.sh)"

# Install Rust wasm target
rustup target add wasm32-unknown-unknown

# Install frontend dependencies
cd src/eliza_frontend && npm install && cd ../..
```

### 2. Local Development

```bash
# Start local replica
dfx start --clean --background

# Deploy all canisters
dfx deploy

# Get canister URLs
echo "Frontend: http://$(dfx canister id eliza_frontend).localhost:8080"
```

> **Note:** IC LLM Canister is only available on mainnet. Local development uses a fallback mode with simple pattern matching.

### 3. Test the Backend

```bash
# Health check
dfx canister call eliza_backend health

# Get version
dfx canister call eliza_backend version

# Chat with Coo
dfx canister call eliza_backend chat '("Hello!")'

# View conversation history
dfx canister call eliza_backend get_conversation_history

# Clear conversation
dfx canister call eliza_backend clear_conversation

# Check current config
dfx canister call eliza_backend get_config
```

## Mainnet Deployment

### 1. Get Cycles

```bash
# Get your principal
dfx identity get-principal

# Get your ICP account
dfx ledger account-id

# Send ICP to this account, then convert to cycles
dfx cycles convert --amount 0.5 --network ic
```

### 2. Deploy

```bash
# Deploy to mainnet
dfx deploy --network ic

# Set LLM provider to OnChain (IC LLM)
dfx canister call eliza_backend set_llm_provider '(variant { OnChain })' --network ic

# Verify deployment
dfx canister call eliza_backend health --network ic
dfx canister call eliza_backend get_config --network ic
```

## Project Structure

```
coo-icp/
├── dfx.json                          # IC project configuration
├── Cargo.toml                        # Rust workspace
├── canister_ids.json                 # Mainnet canister IDs
├── src/
│   ├── eliza_backend/                # Rust backend canister
│   │   ├── Cargo.toml
│   │   ├── src/lib.rs                # Main canister logic
│   │   └── eliza_backend.did         # Candid interface
│   └── eliza_frontend/               # React frontend
│       ├── package.json
│       ├── vite.config.ts
│       └── src/
│           ├── App.tsx               # Main chat component
│           └── declarations/         # Generated types
└── README.md
```

## API Reference

### Chat

```candid
chat: (text) -> (variant { Ok: text; Err: text });
```

Send a message and receive an AI response from Coo.

### Character Management

```candid
update_character: (Character) -> (variant { Ok; Err: text });
get_character: () -> (opt Character) query;
```

Admin-only functions to customize Coo's personality.

### Configuration

```candid
set_llm_provider: (LlmProvider) -> (variant { Ok; Err: text });
get_config: () -> (opt Config) query;
```

LLM providers:
- `OnChain` - IC LLM Canister (Llama 3.1 8B) - **mainnet only**
- `OpenAI` - HTTPS Outcalls to OpenAI API
- `Fallback` - Simple pattern matching (local dev)

### Conversation Management

```candid
get_conversation_history: () -> (vec Message) query;
clear_conversation: () -> ();
get_conversation_count: () -> (nat64) query;
```

## LLM Integration Options

| Method | On-Chain | Model | Best For |
|--------|----------|-------|----------|
| IC LLM (OnChain) | 100% | Llama 3.1 8B | Decentralization |
| OpenAI | Hybrid | GPT-4o-mini | Quality |
| Fallback | 100% | Pattern Match | Local Dev |

### Using OpenAI API (Optional)

1. Store API key (admin only):
```bash
# Convert API key to bytes
dfx canister call eliza_backend store_encrypted_api_key '(vec { ... })' --network ic
```

2. Switch to OpenAI provider:
```bash
dfx canister call eliza_backend set_llm_provider '(variant { OpenAI })' --network ic
```

## Social Integration

Coo supports posting to Twitter (X) and Discord via HTTP outcalls.

### Twitter (X) Configuration

#### 1. Get API Credentials

1. Go to [Twitter Developer Portal](https://developer.x.com/en/portal/dashboard)
2. Create a Project and App
3. Set App permissions to **Read and Write**
4. Generate the following credentials:
   - API Key (Consumer Key)
   - API Secret (Consumer Secret)
   - Access Token
   - Access Token Secret

#### 2. Configure Canister

```bash
# Set Twitter API credentials (Admin only)
# Note: Credentials must be provided as blob (byte array)
dfx canister call eliza_backend configure_twitter '(record {
  api_key = blob "YOUR_API_KEY";
  api_secret = blob "YOUR_API_SECRET";
  access_token = blob "YOUR_ACCESS_TOKEN";
  access_token_secret = blob "YOUR_ACCESS_TOKEN_SECRET";
  user_id = null;
})' --network ic
```

#### 3. Verify Configuration

```bash
# Check social integration status
dfx canister call eliza_backend get_social_status --network ic

# Expected output (twitter_configured: true)
# record {
#   twitter_configured = true;
#   discord_configured = false;
#   ...
# }
```

#### 4. Test Posting

```bash
# Post a test tweet
dfx canister call eliza_backend post_now '(variant { Twitter }, "Hello from Coo on ICP!")' --network ic

# Success response
# (variant { Ok = "1234567890123456789" })  <- Tweet ID

# Error responses
# "You are not allowed to create a Tweet with duplicate content." <- OAuth working, but duplicate tweet
# "Could not authenticate you." <- Check API credentials
```

#### Twitter Troubleshooting

| Error | Cause | Solution |
|-------|-------|----------|
| `Could not authenticate you` | Invalid credentials | Regenerate tokens in Developer Portal |
| `duplicate content` | Same tweet already posted | Change tweet content (OAuth is working!) |
| `Rate limit exceeded` | Too many requests | Wait 15 minutes |
| `Forbidden` | App permissions | Set to "Read and Write" in Developer Portal |

---

### Auto-Reply Feature

Coo can automatically reply to Twitter mentions using AI-generated responses.

#### How It Works

1. **Poll Mentions**: Fetches recent @mentions from Twitter
2. **Generate Response**: Uses IC LLM (Llama 3.1 8B) to generate a contextual reply
3. **Post Reply**: Automatically posts the reply to Twitter

#### Enable Auto-Reply

```bash
# 1. Enable Twitter platform
dfx canister call eliza_backend set_enabled_platforms '(vec { variant { Twitter } })' --network ic

# 2. Enable auto-reply
dfx canister call eliza_backend set_auto_reply '(true)' --network ic

# 3. Manually trigger polling (for testing)
dfx canister call eliza_backend trigger_poll --network ic

# 4. Start automatic polling (interval in seconds)
dfx canister call eliza_backend start_social_polling '(300)' --network ic  # every 5 minutes
```

#### Disable Auto-Reply

```bash
# Disable auto-reply
dfx canister call eliza_backend set_auto_reply '(false)' --network ic

# Stop automatic polling
dfx canister call eliza_backend stop_social_polling --network ic
```

#### Check Status

```bash
dfx canister call eliza_backend get_social_status --network ic

# Response example:
# record {
#   twitter_configured = true;
#   discord_configured = false;
#   enabled_platforms = vec { variant { Twitter } };
#   polling_active = true;
#   last_twitter_poll = 1234567890000000000;
#   last_discord_poll = 0;
#   pending_posts = 0;
#   unprocessed_messages = 0;
# }
```

#### Requirements

> ⚠️ **Twitter API Access Level**: Fetching mentions requires **Basic** or **Pro** API access (paid plans). Free tier may not have access to the `GET /2/users/:id/mentions` endpoint.

| API Tier | Mentions Access | Cost |
|----------|-----------------|------|
| Free | ❌ No | $0 |
| Basic | ✅ Yes | $100/month |
| Pro | ✅ Yes | $5,000/month |

#### Auto-Reply Flow

```
┌─────────────┐    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
│   Twitter   │───►│  Canister   │───►│   IC LLM    │───►│   Twitter   │
│  @mention   │    │ poll_social │    │  Generate   │    │   Reply     │
└─────────────┘    └─────────────┘    └─────────────┘    └─────────────┘
```

---

### Discord Configuration

#### Option 1: Webhook (Recommended)

Webhooks are reliable and don't have consensus issues.

##### 1. Create Webhook

1. Open Discord Server Settings
2. Go to **Integrations** > **Webhooks**
3. Click **New Webhook**
4. Copy the Webhook URL

##### 2. Configure Webhook

```bash
# Configure Discord with webhook URL
dfx canister call eliza_backend configure_discord '(record {
  bot_token = blob "";
  webhook_url = opt "https://discord.com/api/webhooks/YOUR_WEBHOOK_ID/YOUR_WEBHOOK_TOKEN";
  channel_ids = vec {};
})' --network ic
```

##### 3. Test Posting

```bash
# Post via webhook
dfx canister call eliza_backend post_now '(variant { Discord }, "Hello from Coo!")' --network ic

# Success response
# (variant { Ok = "sent via webhook" })
```

#### Option 2: Bot API (Advanced)

> ⚠️ **Warning:** Discord Bot API may post duplicate messages due to ICP's multi-replica consensus mechanism. Use webhooks for production.

##### 1. Create Bot

1. Go to [Discord Developer Portal](https://discord.com/developers/applications)
2. Create New Application
3. Go to **Bot** section and create a bot
4. Copy the Bot Token
5. Enable **Message Content Intent** in Bot settings
6. Invite bot to server with `Send Messages` permission

##### 2. Configure Canister

```bash
# Configure Discord bot (Admin only)
dfx canister call eliza_backend configure_discord '(record {
  bot_token = blob "YOUR_BOT_TOKEN";
  webhook_url = null;
  channel_ids = vec { "CHANNEL_ID_1"; "CHANNEL_ID_2" };
})' --network ic
```

##### 3. Verify Configuration

```bash
# Check social integration status
dfx canister call eliza_backend get_social_status --network ic

# Expected output (discord_configured: true)
```

#### Discord Troubleshooting

| Error | Cause | Solution |
|-------|-------|----------|
| `No consensus could be reached` | Bot API consensus issue | Use webhook instead |
| `Header size exceeds limit` | Response too large | Already fixed in codebase |
| Multiple messages posted | ICP replica duplication | Use webhook, or rate limit usage |
| `Unknown Channel` | Invalid channel ID | Check channel ID in Discord Developer Mode |

---

### Social Integration Status

Check the overall status of social integrations:

```bash
dfx canister call eliza_backend get_social_status --network ic
```

Response fields:
- `twitter_configured`: Twitter credentials are set
- `discord_configured`: Discord bot/webhook is configured
- `enabled_platforms`: List of enabled platforms (Twitter, Discord)
- `polling_active`: Whether automatic polling is running
- `last_twitter_poll`: Timestamp of last Twitter mention check
- `last_discord_poll`: Timestamp of last Discord poll
- `pending_posts`: Number of scheduled posts waiting to be sent
- `unprocessed_messages`: Number of incoming messages not yet processed

---

### Important Notes

> **ICP Consensus Limitation:** Due to ICP's multi-replica architecture, HTTP outcalls are executed by all replicas independently (~13 nodes). This can result in:
> - **Twitter:** Automatic duplicate rejection (works in your favor)
> - **Discord Bot API:** Multiple messages posted
> - **Discord Webhook:** Multiple messages posted
>
> **Recommendation:** Use Twitter for automated posting (duplicates are rejected) and Discord webhooks for manual/occasional posting only.

## ICP Wallet

Coo has a native ICP wallet that allows it to hold and transfer ICP tokens.

### Wallet Address

Coo's ICP wallet address (Account Identifier):
```
e04d487c09bb6a854f9391a63a876bdfb536f4b8ebff730d48707cc9a9c2927b
```

To fund Coo's wallet, send ICP to this address.

### Check Wallet Info

```bash
# Get wallet address
dfx canister call eliza_backend get_wallet_address --network ic

# Get wallet info (address + principal)
dfx canister call eliza_backend get_wallet_info --network ic

# Check ICP balance (queries the ICP Ledger)
dfx canister call eliza_backend check_icp_balance --network ic

# Get full wallet status with live balance
dfx canister call eliza_backend get_wallet_status --network ic
```

### Send ICP (Admin Only)

```bash
# Send ICP to another address
# Parameters: (destination_address, amount_in_e8s, optional_memo)
# Note: 1 ICP = 100,000,000 e8s

# Example: Send 0.1 ICP
dfx canister call eliza_backend send_icp '("DESTINATION_ACCOUNT_ID", 10000000: nat64, null)' --network ic

# Example: Send 1 ICP with memo
dfx canister call eliza_backend send_icp '("DESTINATION_ACCOUNT_ID", 100000000: nat64, opt 12345: nat64)' --network ic
```

### Transaction History

```bash
# Get transaction history (default: last 50 transactions)
dfx canister call eliza_backend get_transaction_history '(null)' --network ic

# Get last 10 transactions
dfx canister call eliza_backend get_transaction_history '(opt 10: nat32)' --network ic
```

### Wallet Security

| Function | Access | Description |
|----------|--------|-------------|
| `get_wallet_address` | Public | View wallet address |
| `get_wallet_info` | Public | View wallet info |
| `check_icp_balance` | Public | Check balance |
| `get_wallet_status` | Public | Get full status |
| `send_icp` | **Admin Only** | Transfer ICP |
| `get_transaction_history` | Public | View transactions |

> **Security Note:** The `send_icp` function requires admin authentication. Third parties cannot transfer ICP from Coo's wallet, even through chat commands.

### Wallet API Reference

```candid
// Get wallet address (Account Identifier in hex)
get_wallet_address: () -> (text) query;

// Get wallet info
get_wallet_info: () -> (WalletInfo) query;

// Check balance from ICP Ledger
check_icp_balance: () -> (variant { Ok: nat64; Err: text });

// Send ICP (Admin only)
// Parameters: destination, amount_e8s, memo
send_icp: (text, nat64, opt nat64) -> (variant { Ok: nat64; Err: text });

// Get transaction history
get_transaction_history: (opt nat32) -> (vec TransactionRecord) query;

// Get wallet status with live balance
get_wallet_status: () -> (variant { Ok: WalletInfo; Err: text });
```

---

## EVM Wallet (Chain-Key ECDSA)

Coo has a multi-chain EVM wallet powered by ICP's **Chain-Key ECDSA** technology. No private keys are stored - all signatures are generated through threshold cryptography.

### EVM Wallet Address

Coo's EVM wallet address (same across all EVM chains):
```
0x38a756bd4082eb3bed2266eef3bea85df4c3e72e
```

### Supported Chains

| Chain | Chain ID | Native Token |
|-------|----------|--------------|
| Ethereum | 1 | ETH |
| Base | 8453 | ETH |
| Polygon | 137 | MATIC |
| Optimism | 10 | ETH |
| Arbitrum | 42161 | ETH |

### Configure a Chain (Admin Only)

```bash
# Configure Base mainnet
dfx canister call eliza_backend configure_evm_chain '(record {
  chain_id = 8453: nat64;
  chain_name = "Base";
  rpc_url = "https://mainnet.base.org";
  native_symbol = "ETH";
  decimals = 18: nat8;
})' --network ic

# Configure Polygon mainnet
dfx canister call eliza_backend configure_evm_chain '(record {
  chain_id = 137: nat64;
  chain_name = "Polygon";
  rpc_url = "https://polygon-rpc.com";
  native_symbol = "MATIC";
  decimals = 18: nat8;
})' --network ic
```

### Check EVM Wallet

```bash
# Get EVM address (same for all chains)
dfx canister call eliza_backend get_evm_address --network ic

# Get wallet info for specific chain
dfx canister call eliza_backend get_evm_wallet_info '(8453: nat64)' --network ic

# Check balance on Base
dfx canister call eliza_backend get_evm_balance '(8453: nat64)' --network ic

# Get configured chains
dfx canister call eliza_backend get_configured_chains --network ic
```

### Send Native Token (Admin Only)

```bash
# Send ETH on Base (amount in wei)
# 0.001 ETH = 1000000000000000 wei
dfx canister call eliza_backend send_evm_native '(
  8453: nat64,
  "0xRECIPIENT_ADDRESS",
  "1000000000000000"
)' --network ic

# Send MATIC on Polygon
dfx canister call eliza_backend send_evm_native '(
  137: nat64,
  "0xRECIPIENT_ADDRESS",
  "1000000000000000000"
)' --network ic
```

### EVM Transaction History

```bash
# Get last 50 EVM transactions
dfx canister call eliza_backend get_evm_transaction_history '(null)' --network ic

# Get last 10 transactions
dfx canister call eliza_backend get_evm_transaction_history '(opt 10: nat32)' --network ic
```

### EVM Wallet Security

| Function | Access | Description |
|----------|--------|-------------|
| `get_evm_address` | Public | View EVM address |
| `get_evm_wallet_info` | Public | View wallet info |
| `get_evm_balance` | Public | Check balance |
| `get_configured_chains` | Public | List configured chains |
| `configure_evm_chain` | **Admin Only** | Add/update chain config |
| `send_evm_native` | **Admin Only** | Transfer native tokens |
| `get_evm_transaction_history` | Public | View transactions |

> **Security Note:** All EVM transfer functions (`send_evm_native`) require admin authentication. Third parties cannot transfer tokens from Coo's EVM wallet.

### How Chain-Key ECDSA Works

```
┌─────────────────────────────────────────────────────────────┐
│  ICP Management Canister (Threshold ECDSA)                  │
│                                                              │
│  ┌──────────────┐    ┌──────────────┐    ┌──────────────┐  │
│  │   Node 1     │    │   Node 2     │    │   Node N     │  │
│  │  Key Share   │    │  Key Share   │    │  Key Share   │  │
│  └──────┬───────┘    └──────┬───────┘    └──────┬───────┘  │
│         │                   │                   │          │
│         └───────────────────┼───────────────────┘          │
│                             │                               │
│                   ┌─────────▼─────────┐                    │
│                   │  Threshold Sign   │                    │
│                   │  (No single key)  │                    │
│                   └─────────┬─────────┘                    │
│                             │                               │
└─────────────────────────────┼───────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│  Coo Backend Canister                                       │
│                                                              │
│  1. Build EVM Transaction                                   │
│  2. Request ECDSA Signature                                 │
│  3. Send to EVM RPC                                         │
│                                                              │
└───────────────────────────┬─────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  EVM Chain (Base, Polygon, etc.)                            │
│                                                              │
│  Transaction confirmed on-chain                             │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

**Benefits:**
- **No private key storage**: Keys never exist in a single location
- **Decentralized security**: Requires consensus of ICP nodes
- **Multi-chain support**: Same address across all EVM chains
- **Censorship-resistant**: No centralized key management

---

## Tech Stack

- **Backend**: Rust + ic-cdk + ic-llm
- **Frontend**: React + TypeScript + Vite
- **AI Model**: Llama 3.1 8B (via IC LLM Canister)
- **Auth**: Internet Identity
- **Social**: Twitter API (OAuth 1.0a), Discord Webhooks
- **ICP Wallet**: ICP Ledger integration
- **EVM Wallet**: Chain-Key ECDSA (threshold signatures)
- **Framework**: elizaOS

## Security

- All conversations are stored per-user (by Principal)
- Admin functions require the deployer's identity
- No external API calls with OnChain mode
- API keys encrypted with vetKeys (for OpenAI mode)
- **ICP Wallet protection**: ICP transfers (`send_icp`) require admin authentication
- **EVM Wallet protection**: EVM transfers (`send_evm_native`) require admin authentication
- **Chain-Key security**: No private keys stored; threshold ECDSA via ICP management canister
- **Chat isolation**: Chat responses are text-only; users cannot trigger wallet operations through conversation

## About elizaOS

[elizaOS](https://github.com/elizaOS/eliza) is the leading open-source framework for building autonomous AI agents. Coo is built on this framework to leverage its powerful agent capabilities while running fully decentralized on the Internet Computer.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

**Repository:** https://github.com/dwebxr/coo-icp

## License

MIT
