# Build Your Own On-chain Agent in 30 Minutes

This guide walks you through customizing this template to create your own unique AI agent on the Internet Computer.

---

## Table of Contents

1. [Fork & Clone](#1-fork--clone)
2. [Customize Your Character](#2-customize-your-character)
3. [Configure LLM Provider](#3-configure-llm-provider)
4. [Customize the Frontend](#4-customize-the-frontend)
5. [Deploy to Mainnet](#5-deploy-to-mainnet)
6. [Advanced Customization](#6-advanced-customization)

---

## 1. Fork & Clone

```bash
# Fork this repo on GitHub, then:
git clone https://github.com/YOUR_USERNAME/coo-icp.git my-agent
cd my-agent

# Setup
make setup
```

---

## 2. Customize Your Character

The AI's personality is defined in `src/eliza_backend/src/lib.rs`. Here's what you can customize:

### 2.1 Character Definition

Find the `default_character()` function (around line 60):

```rust
fn default_character() -> Character {
    Character {
        name: "Coo".to_string(),  // <-- Change this
        system_prompt: r#"You are Coo, a helpful AI assistant..."#.to_string(),  // <-- Customize this
        bio: vec![
            "On-chain AI agent powered by elizaOS...".to_string(),  // <-- Update these
        ],
        style: vec![
            "Friendly".to_string(),
            "Helpful".to_string(),
        ],
    }
}
```

### 2.2 Example: Create a DeFi Advisor Agent

```rust
fn default_character() -> Character {
    Character {
        name: "DeFiBot".to_string(),
        system_prompt: r#"You are DeFiBot, an expert AI assistant specializing in decentralized finance on the Internet Computer.

You are knowledgeable about:
- ICP DeFi protocols (ICPSwap, Sonic, etc.)
- Liquidity pools and yield farming
- Token swaps and DEX mechanics
- Risk assessment in DeFi

Guidelines:
- Always remind users to DYOR (Do Your Own Research)
- Never give financial advice
- Explain DeFi concepts in simple terms
- Be helpful but cautious about speculative topics"#.to_string(),
        bio: vec![
            "DeFi knowledge expert on the Internet Computer".to_string(),
            "Helps users understand decentralized finance".to_string(),
        ],
        style: vec![
            "Educational".to_string(),
            "Cautious".to_string(),
            "Precise".to_string(),
        ],
    }
}
```

### 2.3 Example: Create a DAO Governance Assistant

```rust
fn default_character() -> Character {
    Character {
        name: "GovBot".to_string(),
        system_prompt: r#"You are GovBot, a governance assistant for DAOs on the Internet Computer.

Your capabilities:
- Explain governance proposals in plain language
- Summarize voting options and implications
- Track proposal status and deadlines
- Educate users about SNS DAOs

You maintain neutrality and present all sides of proposals fairly."#.to_string(),
        bio: vec![
            "DAO governance specialist".to_string(),
            "Helps communities make informed decisions".to_string(),
        ],
        style: vec![
            "Neutral".to_string(),
            "Informative".to_string(),
            "Structured".to_string(),
        ],
    }
}
```

### 2.4 Runtime Character Updates

You can also update the character without redeploying:

```bash
dfx canister call eliza_backend update_character '(record {
  name = "MyAgent";
  system_prompt = "You are MyAgent, specialized in...";
  bio = vec { "Bio line 1"; "Bio line 2" };
  style = vec { "Professional"; "Technical" }
})'
```

---

## 3. Configure LLM Provider

### 3.1 Provider Options

| Provider | Use Case | Setup Required |
|----------|----------|----------------|
| `OnChain` | Production (full decentralization) | None - just deploy to mainnet |
| `OpenAI` | Higher quality responses | API key required |
| `Fallback` | Local development | None |

### 3.2 Set Default Provider

In `lib.rs`, find the `init()` function:

```rust
#[init]
fn init() {
    // ...
    CONFIG.with(|cfg| {
        *cfg.borrow_mut() = Some(Config {
            llm_provider: LlmProvider::OnChain,  // <-- Change default here
            max_conversation_length: 50,
            admin: caller,
        });
    });
}
```

### 3.3 Switch Provider at Runtime

```bash
# Use IC LLM (Llama 3.1 8B) - recommended for mainnet
dfx canister call eliza_backend set_llm_provider '(variant { OnChain })'

# Use OpenAI API
dfx canister call eliza_backend set_llm_provider '(variant { OpenAI })'

# Use fallback (local dev)
dfx canister call eliza_backend set_llm_provider '(variant { Fallback })'
```

### 3.4 Configure OpenAI (Optional)

If using OpenAI mode:

```bash
# Store your API key
make set-openai-key

# Or manually:
KEY="sk-your-api-key-here"
dfx canister call eliza_backend store_encrypted_api_key "(vec { $(echo -n "$KEY" | xxd -p | sed 's/../0x&; /g' | sed 's/; $//') })"
```

---

## 4. Customize the Frontend

### 4.1 Update Branding

Edit `src/eliza_frontend/src/App.tsx`:

```tsx
// Find and update the header section
<header className="app-header">
  <h1>🤖 MyAgent</h1>  {/* <-- Change title */}
  <p>Your custom AI assistant on the Internet Computer</p>  {/* <-- Change subtitle */}
</header>
```

### 4.2 Update Styling

Edit `src/eliza_frontend/src/index.css`:

```css
/* Change the gradient colors */
.app-header {
  background: linear-gradient(135deg, #your-color-1 0%, #your-color-2 100%);
}

/* Change accent colors */
:root {
  --primary-color: #your-brand-color;
  --secondary-color: #your-secondary-color;
}
```

### 4.3 Rebuild Frontend

```bash
cd src/eliza_frontend
npm run build
cd ../..
dfx deploy eliza_frontend
```

---

## 5. Deploy to Mainnet

### 5.1 Prepare Cycles

```bash
# Check your balance
dfx cycles balance --network ic

# If needed, convert ICP to cycles
dfx cycles convert --amount 1 --network ic
```

### 5.2 Deploy

```bash
# One-click deployment
make deploy-ic

# Or manually:
cd src/eliza_frontend && npm run build && cd ../..
dfx deploy --network ic
```

### 5.3 Enable On-chain AI

```bash
dfx canister call eliza_backend set_llm_provider '(variant { OnChain })' --network ic
```

### 5.4 Verify Deployment

```bash
# Health check
dfx canister call eliza_backend health --network ic

# Get config
dfx canister call eliza_backend get_config --network ic

# Test chat
dfx canister call eliza_backend chat '("Hello!")' --network ic
```

---

## 6. Advanced Customization

### 6.1 Add Custom Functions

You can extend the backend with additional capabilities. In `lib.rs`:

```rust
// Example: Add a function to summarize governance proposals
#[update]
async fn summarize_proposal(proposal_id: String) -> Result<String, String> {
    // 1. Fetch proposal data (would need HTTPS outcalls in production)
    let proposal_text = format!("Proposal {}: ...", proposal_id);

    // 2. Create a summary prompt
    let prompt = format!(
        "Summarize this governance proposal in 3 bullet points:\n{}",
        proposal_text
    );

    // 3. Use the chat function
    chat(prompt).await
}
```

### 6.2 Conversation Memory Customization

Adjust conversation length in `lib.rs`:

```rust
CONFIG.with(|cfg| {
    *cfg.borrow_mut() = Some(Config {
        llm_provider: LlmProvider::OnChain,
        max_conversation_length: 100,  // <-- Increase for longer memory
        admin: caller,
    });
});
```

### 6.3 Add Rate Limiting

```rust
thread_local! {
    static RATE_LIMIT: RefCell<HashMap<Principal, (u64, u32)>> = RefCell::new(HashMap::new());
}

fn check_rate_limit() -> Result<(), String> {
    let caller = ic_cdk::caller();
    let now = ic_cdk::api::time();
    let window = 60_000_000_000u64; // 1 minute in nanoseconds
    let max_requests = 10;

    RATE_LIMIT.with(|rl| {
        let mut limits = rl.borrow_mut();
        let (last_reset, count) = limits.get(&caller).copied().unwrap_or((now, 0));

        if now - last_reset > window {
            limits.insert(caller, (now, 1));
            Ok(())
        } else if count >= max_requests {
            Err("Rate limit exceeded. Please wait.".to_string())
        } else {
            limits.insert(caller, (last_reset, count + 1));
            Ok(())
        }
    })
}
```

### 6.4 Update Candid Interface

If you add new functions, update `src/eliza_backend/eliza_backend.did`:

```candid
service : {
  // ... existing methods ...

  // Add your new method
  summarize_proposal : (text) -> (variant { Ok : text; Err : text });
}
```

Then regenerate TypeScript types:

```bash
dfx generate eliza_backend
```

---

## Quick Reference: Key Files

| File | Purpose |
|------|---------|
| `src/eliza_backend/src/lib.rs` | Backend logic, character definition |
| `src/eliza_backend/eliza_backend.did` | Candid interface definition |
| `src/eliza_frontend/src/App.tsx` | Frontend UI component |
| `src/eliza_frontend/src/index.css` | Styling |
| `dfx.json` | Canister configuration |
| `Makefile` | Build & deploy commands |

---

## Troubleshooting

### "Only admin can update character"

You need to use the same identity that deployed the canister:

```bash
dfx identity list
dfx identity use <your-identity>
```

### "No response from IC LLM"

IC LLM only works on mainnet:

```bash
# Check you're on mainnet
dfx canister call eliza_backend get_config --network ic

# Ensure provider is OnChain
dfx canister call eliza_backend set_llm_provider '(variant { OnChain })' --network ic
```

### "Rate limit on HTTPS Outcalls"

OpenAI mode has cycle costs. Ensure you have sufficient cycles:

```bash
dfx cycles balance --network ic
```

---

## Need Help?

- [Internet Computer Developer Docs](https://internetcomputer.org/docs)
- [IC LLM Canister Guide](https://internetcomputer.org/docs/current/developer-docs/ai/ai-on-chain)
- [Rust CDK Reference](https://docs.rs/ic-cdk/latest/ic_cdk/)

---

Happy building! 🚀
