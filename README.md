# Coo - Fully On-Chain AI Agent

<div align="center">
  <img src="overview.png" alt="Coo ICP" width="100%" />
  <p>
    <strong>A fully decentralized AI chat agent running on the Internet Computer blockchain, powered by the elizaOS framework and IC LLM Canister (Llama 3.1 8B).</strong>
  </p>
</div>

> **Note:** This canister was recently stripped down to its core chat-agent functionality. Earlier versions also included multi-chain wallets (ICP/EVM/Solana), DeFi swaps and bridges, ERC-8004 trustless-agent identity, social posting (Twitter/Discord), and a governance/whitelist layer. Those features were removed in a refactor and are not documented here; they may be reintroduced properly in the future.

## Live Demo

**Frontend:** https://4res3-liaaa-aaaas-qdqcq-cai.icp0.io/

**Backend Candid UI:** https://a4gq6-oaaaa-aaaas-qaa4q-cai.raw.icp0.io/?id=4wfup-gqaaa-aaaas-qdqca-cai

## Features

- **Fully On-Chain AI**: Uses IC LLM Canister (Llama 3.1 8B) for AI responses
- **Decentralized**: Runs entirely on Internet Computer blockchain
- **Censorship-Resistant**: No centralized servers or API dependencies
- **Internet Identity Authentication**: Secure user authentication (frontend)
- **Conversation Memory**: Maintains context per-caller (Principal) across messages
- **Configurable Character**: Admin can customize Coo's name, system prompt, bio, and style
- **Pluggable LLM Provider**: Switch between OnChain (IC LLM), OpenAI (HTTPS outcalls), and Fallback (pattern matching for local dev)
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
│                         │  └────────────────────────────┘  │   │
│  ┌─────────────────┐    │                                  │   │
│  │    Internet     │    │  ┌────────────────────────────┐  │   │
│  │    Identity     │    │  │   OpenAI API (optional)    │  │   │
│  └─────────────────┘    │  │   via HTTPS Outcalls        │  │   │
│                         │  └────────────────────────────┘  │   │
│                         │                                  │   │
│                         │  4wfup-gqaaa-aaaas-qdqca-cai    │   │
│                         └──────────────────────────────────┘   │
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
# Health check (returns dynamic status: ok/degraded + version + conversation count)
dfx canister call eliza_backend health

# Get version
dfx canister call eliza_backend version

# Get canister metrics (conversations, stable memory pages, heap size)
dfx canister call eliza_backend get_metrics

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
eliza-icp/
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

Send a message and receive an AI response from Coo. Conversation history is maintained per caller (Principal).

### Character Management

```candid
update_character: (Character) -> (variant { Ok; Err: text });
get_character: () -> (opt Character) query;
```

`update_character` is admin-only; `get_character` is public. Used to customize Coo's personality (name, system prompt, bio, style).

### Configuration

```candid
set_llm_provider: (LlmProvider) -> (variant { Ok; Err: text });
get_config: () -> (opt Config) query;
```

`set_llm_provider` is admin-only. `get_config` is public and returns the current LLM provider, max conversation length, and admin principal.

LLM providers:
- `OnChain` - IC LLM Canister (Llama 3.1 8B) - **mainnet only**
- `OpenAI` - HTTPS Outcalls to OpenAI API
- `Fallback` - Simple pattern matching (local dev)

### Admin

```candid
set_admin: (principal) -> (variant { Ok; Err: text });
```

Admin-only. Transfers admin rights to a new principal; the new admin gains control over `set_llm_provider`, `update_character`, and `store_encrypted_api_key`.

```bash
# Transfer admin to another principal
dfx canister call eliza_backend set_admin '(principal "aaaaa-bbbbb-ccccc-ddddd-eeeee-fffff-ggggg-hhhhh-iiiii-jjjjj-jae")' --network ic

# Verify admin change
dfx canister call eliza_backend get_config --network ic
```

> **Note:** You retain **Controller** rights for deployment and Cycles management even after transferring Admin rights.

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

> **Note:** `transform_openai_response` is an internal query callback used by the IC HTTP-outcalls subsystem to normalize the OpenAI response before multi-replica consensus. It is not meant to be called directly.

## Monitoring

### Health Check

```bash
# Dynamic health status (returns config/character state + version + conversation count)
dfx canister call eliza_backend health --network ic
# "ok | v0.5.0 | 42 conversations"  or  "degraded | config=false character=true"
```

### Canister Metrics

```bash
# Get comprehensive canister metrics
dfx canister call eliza_backend get_metrics --network ic
```

Returns:
```
record {
  version = "0.5.0";
  conversations = 42;
  stable_memory_pages = 128;
  heap_memory_bytes = 8388608;
}
```

### Metrics API Reference

| Function | Access | Description |
|----------|--------|--------------|
| `health` | Public | Dynamic health status |
| `version` | Public | Canister version string |
| `get_metrics` | Public | Comprehensive canister metrics |

## Tech Stack

- **Backend**: Rust + ic-cdk + ic-llm
- **Frontend**: React + TypeScript + Vite
- **AI Model**: Llama 3.1 8B (via IC LLM Canister), with optional OpenAI (GPT-4o-mini) fallback
- **Auth**: Internet Identity
- **Framework**: elizaOS

## Security

- All conversations are stored per-user (by Principal)
- Admin functions require the current admin's identity
- No external API calls with OnChain mode
- API keys for OpenAI mode are stored in canister state without client-side encryption (the `encrypted_*` naming is a placeholder; real vetKeys encryption is future work)

## About elizaOS

[elizaOS](https://github.com/elizaOS/eliza) is the leading open-source framework for building autonomous AI agents. Coo is built on this framework to leverage its powerful agent capabilities while running fully decentralized on the Internet Computer.

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

**Repository:** https://github.com/dwebxr/coo-icp

## License

MIT
