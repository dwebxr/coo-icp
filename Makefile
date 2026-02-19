# =============================================================================
# Coo ICP - On-chain LLM Chat Starter
# One-click deployment commands for Internet Computer
# =============================================================================

.PHONY: help install setup deploy-local deploy-ic build clean test \
        start stop logs set-openai-key set-provider info

# Default target
help:
	@echo ""
	@echo "╔═══════════════════════════════════════════════════════════════════╗"
	@echo "║         Coo ICP - On-chain LLM Chat Starter                       ║"
	@echo "╚═══════════════════════════════════════════════════════════════════╝"
	@echo ""
	@echo "🚀 Quick Start:"
	@echo "  make setup          - Install all dependencies"
	@echo "  make deploy-local   - Deploy to local replica (5 min)"
	@echo "  make deploy-ic      - Deploy to IC mainnet"
	@echo ""
	@echo "📦 Build Commands:"
	@echo "  make build          - Build frontend and generate types"
	@echo "  make clean          - Clean all build artifacts"
	@echo ""
	@echo "🔧 Development:"
	@echo "  make start          - Start local dfx replica"
	@echo "  make stop           - Stop local dfx replica"
	@echo "  make logs           - View canister logs"
	@echo "  make test           - Run health check"
	@echo ""
	@echo "⚙️  Configuration:"
	@echo "  make set-openai-key - Set OpenAI API key (encrypted)"
	@echo "  make set-provider   - Change LLM provider"
	@echo "  make info           - Show deployment info"
	@echo ""

# =============================================================================
# Setup & Installation
# =============================================================================

install:
	@echo "📦 Installing dependencies..."
	@command -v dfx >/dev/null 2>&1 || { echo "❌ dfx not found. Install: sh -ci \"\$$(curl -fsSL https://internetcomputer.org/install.sh)\""; exit 1; }
	@command -v rustup >/dev/null 2>&1 || { echo "❌ Rust not found. Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"; exit 1; }
	rustup target add wasm32-unknown-unknown
	cd src/eliza_frontend && npm install
	@echo "✅ Dependencies installed!"

setup: install
	@echo "✅ Setup complete! Run 'make deploy-local' to start."

# =============================================================================
# Local Development
# =============================================================================

start:
	@echo "🚀 Starting local dfx replica..."
	dfx start --background --clean
	@echo "✅ Local replica started!"

stop:
	@echo "🛑 Stopping local dfx replica..."
	dfx stop
	@echo "✅ Replica stopped."

deploy-local: start
	@echo "📦 Deploying to local replica..."
	dfx deps pull
	dfx deps init
	dfx deps deploy
	dfx deploy
	@echo ""
	@echo "╔═══════════════════════════════════════════════════════════════════╗"
	@echo "║  ✅ Local deployment complete!                                    ║"
	@echo "╚═══════════════════════════════════════════════════════════════════╝"
	@echo ""
	@echo "🌐 Frontend: http://localhost:4943/?canisterId=$$(dfx canister id eliza_frontend)"
	@echo "📡 Backend:  http://localhost:4943/?canisterId=$$(dfx canister id eliza_backend)"
	@echo ""
	@echo "💡 Note: Local uses 'Fallback' mode (pattern matching)."
	@echo "   On-chain LLM is only available on IC mainnet."
	@echo ""

# =============================================================================
# IC Mainnet Deployment
# =============================================================================

deploy-ic:
	@echo "🌐 Deploying to IC mainnet..."
	@echo ""
	@echo "⚠️  This will deploy to the Internet Computer mainnet."
	@echo "   Cycles will be consumed. Make sure your wallet has sufficient balance."
	@echo ""
	@read -p "Continue? [y/N] " confirm && [ "$$confirm" = "y" ] || exit 1
	cd src/eliza_frontend && npm run build
	dfx deploy --network ic
	@echo ""
	@echo "╔═══════════════════════════════════════════════════════════════════╗"
	@echo "║  ✅ IC Mainnet deployment complete!                               ║"
	@echo "╚═══════════════════════════════════════════════════════════════════╝"
	@echo ""
	@dfx canister --network ic id eliza_frontend 2>/dev/null && echo "🌐 Frontend: https://$$(dfx canister --network ic id eliza_frontend).icp0.io/"
	@dfx canister --network ic id eliza_backend 2>/dev/null && echo "📡 Backend:  https://$$(dfx canister --network ic id eliza_backend).icp0.io/"
	@echo ""

# =============================================================================
# Build Commands
# =============================================================================

build:
	@echo "🔨 Building project..."
	dfx generate eliza_backend
	cd src/eliza_frontend && npm run build
	@echo "✅ Build complete!"

clean:
	@echo "🧹 Cleaning build artifacts..."
	rm -rf target/
	rm -rf src/eliza_frontend/dist/
	rm -rf src/eliza_frontend/node_modules/
	rm -rf .dfx/
	@echo "✅ Clean complete!"

# =============================================================================
# Configuration & Testing
# =============================================================================

test:
	@echo "🧪 Running health check..."
	@dfx canister call eliza_backend health 2>/dev/null && echo "✅ Backend is healthy!" || echo "❌ Backend not responding. Is it deployed?"

logs:
	@echo "📜 Fetching canister logs..."
	dfx canister logs eliza_backend

info:
	@echo ""
	@echo "📋 Deployment Information"
	@echo "========================="
	@echo ""
	@echo "Local Canisters:"
	@dfx canister id eliza_frontend 2>/dev/null && echo "  Frontend: $$(dfx canister id eliza_frontend)" || echo "  Frontend: (not deployed)"
	@dfx canister id eliza_backend 2>/dev/null && echo "  Backend:  $$(dfx canister id eliza_backend)" || echo "  Backend:  (not deployed)"
	@echo ""
	@echo "IC Mainnet Canisters:"
	@dfx canister --network ic id eliza_frontend 2>/dev/null && echo "  Frontend: $$(dfx canister --network ic id eliza_frontend)" || echo "  Frontend: (not deployed)"
	@dfx canister --network ic id eliza_backend 2>/dev/null && echo "  Backend:  $$(dfx canister --network ic id eliza_backend)" || echo "  Backend:  (not deployed)"
	@echo ""
	@echo "Current Config:"
	@dfx canister call eliza_backend get_config 2>/dev/null || echo "  (Backend not accessible)"
	@echo ""

# =============================================================================
# LLM Provider Configuration
# =============================================================================

set-provider:
	@echo ""
	@echo "🔧 Select LLM Provider:"
	@echo "  1) OnChain  - IC LLM Canister (Llama 3.1 8B) [Mainnet only]"
	@echo "  2) OpenAI   - OpenAI API via HTTPS Outcalls"
	@echo "  3) Fallback - Pattern matching (Local dev)"
	@echo ""
	@read -p "Enter choice [1-3]: " choice; \
	case $$choice in \
		1) dfx canister call eliza_backend set_llm_provider '(variant { OnChain })' ;; \
		2) dfx canister call eliza_backend set_llm_provider '(variant { OpenAI })' ;; \
		3) dfx canister call eliza_backend set_llm_provider '(variant { Fallback })' ;; \
		*) echo "Invalid choice" ;; \
	esac
	@echo ""
	@echo "✅ Provider updated!"

set-openai-key:
	@echo ""
	@echo "🔐 Set OpenAI API Key (for OpenAI provider mode)"
	@echo ""
	@echo "⚠️  Note: In production, use vetKeys for proper encryption."
	@echo "   This command stores the key with basic encoding."
	@echo ""
	@read -p "Enter your OpenAI API key: " key; \
	if [ -n "$$key" ]; then \
		encoded=$$(echo -n "$$key" | xxd -p | tr -d '\n' | sed 's/../0x&, /g' | sed 's/, $$//'); \
		dfx canister call eliza_backend store_encrypted_api_key "(vec { $$encoded })"; \
		echo "✅ API key stored!"; \
	else \
		echo "❌ No key provided."; \
	fi

# =============================================================================
# Character Customization
# =============================================================================

set-character:
	@echo ""
	@echo "🎭 Update AI Character"
	@echo ""
	@echo "This will update the AI's personality. Edit the values below:"
	@echo ""
	@read -p "Name [Coo]: " name; \
	read -p "Bio (short description): " bio; \
	read -p "System prompt: " prompt; \
	read -p "Style (casual/formal/technical): " style; \
	name=$${name:-Coo}; \
	dfx canister call eliza_backend update_character "(record { \
		name = \"$$name\"; \
		system_prompt = \"$$prompt\"; \
		bio = vec { \"$$bio\" }; \
		style = record { all = vec { \"$$style\" }; chat = vec {}; post = vec {} } \
	})"
	@echo ""
	@echo "✅ Character updated!"

# =============================================================================
# D2A Briefing API Configuration
# =============================================================================

set-d2a:
	@echo ""
	@echo "🔗 Configure D2A Briefing API (Aegis)"
	@echo ""
	@echo "This enables RAG-enhanced chat using scored content from Aegis."
	@echo ""
	@read -p "Aegis URL [https://aegis.dwebxr.xyz]: " url; \
	read -p "Enable x402 payment? (y/n) [n]: " x402; \
	url=$${url:-https://aegis.dwebxr.xyz}; \
	x402_val=$$([ "$$x402" = "y" ] && echo "true" || echo "false"); \
	dfx canister call eliza_backend configure_d2a "(record { \
		enabled = true; \
		aegis_url = \"$$url\"; \
		fetch_interval_seconds = 3600; \
		cache_ttl_seconds = 3600; \
		min_composite_score = 4.0; \
		max_context_items = 3; \
		x402_enabled = $$x402_val; \
		x402_network = \"eip155:8453\"; \
		x402_usdc_contract = \"0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913\"; \
	})"
	@echo ""
	@echo "✅ D2A configured!"

d2a-fetch:
	@echo "📡 Fetching D2A briefing from Aegis..."
	@dfx canister call eliza_backend trigger_d2a_fetch

d2a-status:
	@echo "📊 D2A Briefing Status:"
	@dfx canister call eliza_backend get_d2a_status

d2a-start:
	@echo "🔄 Starting D2A polling (1 hour interval)..."
	@dfx canister call eliza_backend start_d2a_polling '(3600)'
	@echo "✅ D2A polling started!"

d2a-stop:
	@echo "⏹ Stopping D2A polling..."
	@dfx canister call eliza_backend stop_d2a_polling
	@echo "✅ D2A polling stopped!"
