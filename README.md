# 🚀 On-chain LLM Chat Starter

> **The minimal full-stack template for building AI agents on the Internet Computer.**
>
> Fork this repo → Customize → Deploy in 5 minutes.

[![Live Demo](https://img.shields.io/badge/Demo-Live%20on%20IC-brightgreen)](https://4res3-liaaa-aaaas-qdqcq-cai.icp0.io/)
[![IC LLM](https://img.shields.io/badge/AI-Llama%203.1%208B-blue)](https://internetcomputer.org/docs/current/developer-docs/ai/ai-on-chain)
[![License](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

---

## ✨ What This Is

A production-ready starter template for on-chain AI chat applications featuring:

- **🧠 Fully On-chain AI** - IC LLM Canister (Llama 3.1 8B) with zero external API dependencies
- **🔐 Internet Identity** - Secure authentication built-in
- **💾 Per-user Memory** - Conversation history stored by Principal
- **🔄 Multi-Provider** - Switch between OnChain / OpenAI / Fallback modes
- **⚡ Full-stack** - React frontend + Rust backend, ready to deploy

## 🎯 Live Demo

**Try it now:** [https://4res3-liaaa-aaaas-qdqcq-cai.icp0.io/](https://4res3-liaaa-aaaas-qdqcq-cai.icp0.io/)

---

## 🏗️ Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                        Internet Computer Mainnet                            │
│                                                                             │
│   ┌──────────────┐      ┌──────────────────────────────────────────────┐   │
│   │   Frontend   │      │             Backend Canister                 │   │
│   │   (React)    │◄────►│               (Rust)                         │   │
│   │              │      │                                              │   │
│   │ • Chat UI    │      │  ┌─────────────────────────────────────────┐ │   │
│   │ • II Login   │      │  │         LLM Provider Switch             │ │   │
│   │ • History    │      │  │                                         │ │   │
│   └──────────────┘      │  │   ┌─────────┐ ┌───────┐ ┌──────────┐   │ │   │
│                         │  │   │OnChain  │ │OpenAI │ │ Fallback │   │ │   │
│   ┌──────────────┐      │  │   │Llama3.1 │ │ HTTPS │ │ Pattern  │   │ │   │
│   │   Internet   │      │  │   │  8B     │ │Outcall│ │ Match    │   │ │   │
│   │   Identity   │◄────►│  │   └─────────┘ └───────┘ └──────────┘   │ │   │
│   └──────────────┘      │  └─────────────────────────────────────────┘ │   │
│                         │                                              │   │
│                         │  • Per-user conversation memory              │   │
│                         │  • Admin controls for character/provider     │   │
│                         │  • vetKeys API key encryption (OpenAI mode)  │   │
│                         └──────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## ⚡ 5-Minute Quick Start

### Prerequisites

- [dfx](https://internetcomputer.org/docs/current/developer-docs/setup/install) (IC SDK)
- [Rust](https://rustup.rs/) with wasm32 target
- [Node.js](https://nodejs.org/) v18+

### Option 1: One-Click Deploy (Recommended)

```bash
# Clone and enter
git clone https://github.com/dwebxr/coo-icp.git && cd coo-icp

# Setup & deploy locally
make setup
make deploy-local
```

Done! Open the URL printed in the terminal.

### Option 2: Manual Setup

```bash
# Install dependencies
rustup target add wasm32-unknown-unknown
cd src/eliza_frontend && npm install && cd ../..

# Start local replica
dfx start --clean --background

# Deploy
dfx deps pull && dfx deps init && dfx deps deploy
dfx deploy

# Get your local URL
echo "http://localhost:4943/?canisterId=$(dfx canister id eliza_frontend)"
```

---

## 🌐 Deploy to IC Mainnet

```bash
# Ensure you have cycles (0.5+ ICP recommended)
dfx cycles convert --amount 0.5 --network ic

# Deploy
make deploy-ic

# Enable on-chain AI
dfx canister call eliza_backend set_llm_provider '(variant { OnChain })' --network ic
```

---

## 💰 Cycles Cost Estimate

| Operation | Estimated Cost |
|-----------|----------------|
| Initial deployment (both canisters) | ~0.5-1T cycles (~$0.50-1.00) |
| Single chat message (OnChain LLM) | ~1-5B cycles (~$0.001-0.005) |
| Frontend asset serving | ~0.1B cycles/request |
| Storage (conversation history) | ~100M cycles/MB/year |

> 💡 **Tip:** Start with 1T cycles. A typical development/demo session uses 0.1-0.3T cycles.

---

## 🔧 LLM Provider Modes

| Mode | Description | Best For |
|------|-------------|----------|
| **OnChain** | IC LLM Canister (Llama 3.1 8B) | Production, full decentralization |
| **OpenAI** | HTTPS Outcalls to OpenAI API | Higher quality responses |
| **Fallback** | Pattern matching | Local development |

### Switch Provider

```bash
# Interactive selection
make set-provider

# Or directly
dfx canister call eliza_backend set_llm_provider '(variant { OnChain })'
```

### Using OpenAI Mode

```bash
# Set your API key (encrypted with vetKeys)
make set-openai-key

# Switch to OpenAI
dfx canister call eliza_backend set_llm_provider '(variant { OpenAI })'
```

**How vetKeys protects your API key:**
```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Your Key      │───►│  Encrypted in   │───►│ Decrypted only  │
│   (plaintext)   │    │  Canister State │    │ at Runtime      │
└─────────────────┘    └─────────────────┘    └─────────────────┘
       Client              On-chain             HTTP Outcall
                        (never exposed)
```

---

## 🎭 Customize Your Agent

### Quick Character Update

```bash
make set-character
```

### Programmatic Update

```bash
dfx canister call eliza_backend update_character '(record {
  name = "MyAgent";
  system_prompt = "You are a helpful assistant specialized in...";
  bio = vec { "Your bio here" };
  style = record { all = vec { "friendly"; "concise" }; chat = vec {}; post = vec {} }
})'
```

See [docs/BUILD_YOUR_OWN_AGENT.md](docs/BUILD_YOUR_OWN_AGENT.md) for the full customization guide.

---

## 📁 Project Structure

```
coo-icp/
├── Makefile                 # One-click commands
├── dfx.json                 # IC project config
├── Cargo.toml               # Rust workspace
├── src/
│   ├── eliza_backend/       # 🦀 Rust backend canister
│   │   ├── src/lib.rs       # Main logic (~500 lines)
│   │   └── eliza_backend.did # Candid interface
│   └── eliza_frontend/      # ⚛️ React frontend
│       ├── src/App.tsx      # Chat UI component
│       └── src/declarations/ # Generated types
└── docs/
    └── BUILD_YOUR_OWN_AGENT.md # Customization guide
```

---

## 📡 API Reference

### Core Chat

```candid
// Send message, get AI response
chat: (text) -> (variant { Ok: text; Err: text });

// Get user's conversation history
get_conversation_history: () -> (vec Message) query;

// Clear conversation
clear_conversation: () -> ();
```

### Admin Functions

```candid
// Update AI personality
update_character: (Character) -> (variant { Ok; Err: text });

// Switch LLM provider
set_llm_provider: (LlmProvider) -> (variant { Ok; Err: text });

// Store encrypted API key
store_encrypted_api_key: (vec nat8) -> (variant { Ok; Err: text });
```

### Health & Info

```candid
health: () -> (bool) query;
version: () -> (text) query;
get_config: () -> (opt Config) query;
```

---

## 🔒 Security Model

| Feature | Implementation |
|---------|----------------|
| User isolation | Conversations stored per Principal |
| Admin access | Deployer identity required for config |
| API key protection | vetKeys encryption (OpenAI mode) |
| On-chain mode | Zero external dependencies |

---

## 🛠️ Development Commands

```bash
make help           # Show all commands
make deploy-local   # Deploy to local replica
make deploy-ic      # Deploy to IC mainnet
make test           # Health check
make logs           # View canister logs
make info           # Show deployment info
make clean          # Clean build artifacts
```

---

## 🤝 Use This Template

This repo is designed to be forked and customized:

1. **Fork** this repository
2. **Customize** the character in `src/eliza_backend/src/lib.rs`
3. **Deploy** with `make deploy-ic`
4. **Build** your unique on-chain agent!

**License:** MIT - Use freely for any purpose.

---

## 📚 Resources

- [IC LLM Canister Docs](https://internetcomputer.org/docs/current/developer-docs/ai/ai-on-chain)
- [Internet Computer Developer Docs](https://internetcomputer.org/docs)
- [Rust CDK Guide](https://internetcomputer.org/docs/current/developer-docs/backend/rust/)
- [Internet Identity](https://internetcomputer.org/docs/current/developer-docs/identity/internet-identity/overview)

---

## 🏷️ Canister IDs (Mainnet)

| Canister | ID |
|----------|-----|
| Backend | `4wfup-gqaaa-aaaas-qdqca-cai` |
| Frontend | `4res3-liaaa-aaaas-qdqcq-cai` |

---

<p align="center">
  <b>Built with ❤️ on the Internet Computer</b><br>
  <sub>Powered by IC LLM Canister (Llama 3.1 8B)</sub>
</p>
