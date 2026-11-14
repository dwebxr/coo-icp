# Coo — build, test, deploy.  `make help` lists the targets.
#
# NETWORK selects the network for the targets that talk to a canister (default: local). Nothing here passes --yes
# to dfx or touches mainnet without NETWORK=ic or an explicit *-ic target.

SHELL := /bin/bash
NETWORK ?= local
FRONTEND := src/eliza_frontend
# dfx and cargo must find rustup's toolchain (it has the wasm32 target), also where another cargo is first in PATH.
CARGO := PATH="$$HOME/.cargo/bin:$$PATH" cargo
DFX := PATH="$$HOME/.cargo/bin:$$PATH" dfx
WASM_BUILD := $(CARGO) build --target wasm32-unknown-unknown --release --locked

.PHONY: help setup start stop deploy-local build build-ic candid declarations check test test-canister e2e \
        e2e-local install-ic upgrade upgrade-ic upgrade-frontend upgrade-frontend-ic status logs set-provider \
        set-character clean

help:
	@echo "Coo — an AI chat agent in an Internet Computer canister"
	@echo
	@echo "Local"
	@echo "  make setup              install the wasm target and the frontend's dependencies"
	@echo "  make deploy-local       local replica + Internet Identity + backend (fallback provider) + frontend"
	@echo "  make stop               stop the local replica"
	@echo
	@echo "Tests"
	@echo "  make test               canister unit tests, frontend type check and unit tests"
	@echo "  make test-canister      canister unit tests + PocketIC integration tests (incl. upgrade from mainnet's version)"
	@echo "  make e2e                browser tests against the mock + accessibility (needs 'npm run dev' running)"
	@echo "  make e2e-local          browser tests of the real client on a local replica + deployed security headers"
	@echo "  make check              formatting, clippy, Candid file and generated bindings up to date"
	@echo
	@echo "Generated files"
	@echo "  make candid             src/eliza_backend/eliza_backend.did from the code"
	@echo "  make declarations       the frontend's Candid bindings from that file"
	@echo
	@echo "Mainnet (each asks before it changes anything)"
	@echo "  make install-ic         first installation"
	@echo "  make upgrade-ic         upgrade the backend: stop, snapshot, upgrade, verify, start (scripts/upgrade.sh)"
	@echo "  make upgrade-frontend-ic  build and upload the frontend"
	@echo
	@echo "Operating (NETWORK=local by default; add NETWORK=ic for mainnet)"
	@echo "  make status             health, config and metrics"
	@echo "  make logs               canister log"
	@echo "  make set-provider PROVIDER=OnChain|Fallback"
	@echo "  make set-character FILE=<file with a Candid Character record>"

# ---------------------------------------------------------------------------------------------- local

setup:
	@command -v dfx >/dev/null || { echo "dfx not found: https://internetcomputer.org/docs/building-apps/getting-started/install"; exit 1; }
	@command -v rustup >/dev/null || { echo "rustup not found: https://rustup.rs"; exit 1; }
	@command -v node >/dev/null || { echo "node not found (22 or newer; .nvmrc says 24)"; exit 1; }
	PATH="$$HOME/.cargo/bin:$$PATH" rustup target add wasm32-unknown-unknown
	cd $(FRONTEND) && npm ci

# Starts the local replica unless one is running. (State of an earlier run is kept; `dfx start --clean` wipes it.)
start:
	@$(DFX) ping local >/dev/null 2>&1 || $(DFX) start --background

stop:
	$(DFX) stop

# The frontend is built after the backend is deployed: the build reads the local canister ids from .env.
deploy-local: start
	$(DFX) deps pull
	$(DFX) deps init
	$(DFX) deps deploy
	$(DFX) deploy eliza_backend
	$(MAKE) set-provider PROVIDER=Fallback NETWORK=local
	$(MAKE) build
	$(DFX) deploy eliza_frontend
	@echo
	@echo "Frontend: http://$$($(DFX) canister id eliza_frontend).localhost:4943/"
	@echo "The LLM canister exists on mainnet only; this deployment answers with the fallback provider."

# Production build of the frontend for a LOCAL deployment (it contains the local canister ids from .env).
build:
	cd $(FRONTEND) && npm run build

# Production build for mainnet: the same, with no local canister ids in it, so it does not depend on what was
# last deployed locally.
build-ic:
	cd $(FRONTEND) && CANISTER_ID_ELIZA_BACKEND= CANISTER_ID_INTERNET_IDENTITY= npm run build

# ---------------------------------------------------------------------------------------------- generated files

# The Candid file from the code (a unit test fails when it is out of date).
candid:
	$(WASM_BUILD) -p eliza_backend
	candid-extractor target/wasm32-unknown-unknown/release/eliza_backend.wasm > src/eliza_backend/eliza_backend.did

# The frontend's bindings from the Candid file. dfx also writes an index.js that creates an actor from process.env;
# the app creates its own (src/backend.ts), so that file is not kept.
declarations:
	$(DFX) generate eliza_backend
	rm -f $(FRONTEND)/src/declarations/generated/index.js $(FRONTEND)/src/declarations/generated/index.d.ts

check:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --locked -p eliza_backend -p mock_llm --all-targets --features test-hooks -- -D warnings
	$(CARGO) test --locked -p eliza_backend --lib the_candid_file_matches_the_code
	$(MAKE) declarations
	git diff --exit-code -- $(FRONTEND)/src/declarations/generated \
		|| { echo "The generated bindings were out of date; they are regenerated now — commit them."; exit 1; }
	cd $(FRONTEND) && npx tsc --noEmit

# ---------------------------------------------------------------------------------------------- tests

test:
	$(CARGO) test --locked -p eliza_backend --lib
	cd $(FRONTEND) && npx tsc --noEmit && npm test

# Canister unit and PocketIC integration tests. POCKET_IC_BIN defaults to the server bundled with dfx.
# The upgrade test needs the version that runs on mainnet: it is built from DEPLOYED_COMMIT (needs the git history).
DEPLOYED_COMMIT := 5e9c5a29587dc75553d37d43bf1ce4add6aeabd5
DEPLOYED_WASM := target/deployed/eliza_backend.wasm

$(DEPLOYED_WASM):
	rm -rf target/deployed && mkdir -p target/deployed/src
	git archive $(DEPLOYED_COMMIT) Cargo.toml Cargo.lock src/eliza_backend/Cargo.toml src/eliza_backend/src/lib.rs \
		| tar -x -C target/deployed/src
	cd target/deployed/src && $(WASM_BUILD) -p eliza_backend
	cp target/deployed/src/target/wasm32-unknown-unknown/release/eliza_backend.wasm $@

test-canister: $(DEPLOYED_WASM)
	$(WASM_BUILD) -p eliza_backend -p mock_llm
	$(WASM_BUILD) -p eliza_backend --features test-hooks --target-dir target/test-hooks
	$(WASM_BUILD) -p eliza_backend --features small-limits,test-hooks --target-dir target/small-limits
	POCKET_IC_BIN="$${POCKET_IC_BIN:-$$(dfx cache show)/pocket-ic}" ELIZA_OLD_WASM="$(CURDIR)/$(DEPLOYED_WASM)" \
		$(CARGO) test --locked -p eliza_backend -- --include-ignored --test-threads=1

# Against the mock backend of a running dev server (cd src/eliza_frontend && npm run dev).
e2e:
	cd $(FRONTEND) && npm run e2e && npm run a11y

# The real client against a canister on the local replica (fallback provider), and the headers of a frontend
# deployed there. Starts its own dev server on port 5199 for the duration.
e2e-local: start
	$(DFX) deploy eliza_backend
	$(MAKE) set-provider PROVIDER=Fallback NETWORK=local
	$(MAKE) build
	$(DFX) deploy eliza_frontend
	cd $(FRONTEND) && node e2e/headers.mjs "http://$$($(DFX) canister id eliza_frontend).localhost:4943/"
	cd $(FRONTEND) && { npx vite --port 5199 --strictPort >/dev/null 2>&1 & server=$$!; \
		trap 'kill $$server 2>/dev/null' EXIT; \
		for i in $$(seq 1 50); do curl -s -o /dev/null http://localhost:5199/ && break; sleep 0.2; done; \
		BASE=http://localhost:5199 node e2e/local.mjs; }

# ---------------------------------------------------------------------------------------------- mainnet

# First installation only. A canister that holds conversations is upgraded with `make upgrade-ic`.
install-ic:
	@echo "This creates and installs the canisters on MAINNET with the identity '$$(dfx identity whoami)' and spends cycles."
	@read -r -p "Type 'install' to continue: " answer && [ "$$answer" = "install" ] || { echo "Aborted."; exit 1; }
	$(MAKE) build-ic
	$(DFX) deploy --network ic
	@echo "Now set the Wasm memory limit:  dfx canister update-settings eliza_backend --wasm-memory-limit 512MiB --network ic"

upgrade:
	scripts/upgrade.sh $(NETWORK)

upgrade-ic:
	scripts/upgrade.sh ic

# Uploads the frontend only (`dfx deploy eliza_frontend` would also redeploy the backend it depends on).
upgrade-frontend:
	$(MAKE) $(if $(filter ic,$(NETWORK)),build-ic,build)
	$(DFX) canister install eliza_frontend --mode upgrade --network $(NETWORK)

upgrade-frontend-ic:
	$(MAKE) upgrade-frontend NETWORK=ic

# ---------------------------------------------------------------------------------------------- operating

status:
	@$(DFX) canister call eliza_backend health --query --network $(NETWORK)
	@$(DFX) canister call eliza_backend get_config --query --network $(NETWORK)
	@$(DFX) canister call eliza_backend get_metrics --query --network $(NETWORK)

logs:
	$(DFX) canister logs eliza_backend --network $(NETWORK)

# Fails unless the canister answers Ok (a non-controller gets Err, which dfx alone would report as success).
set-provider:
	@[ "$(PROVIDER)" = "OnChain" ] || [ "$(PROVIDER)" = "Fallback" ] || { echo "usage: make set-provider PROVIDER=OnChain|Fallback [NETWORK=ic]"; exit 1; }
	@result="$$($(DFX) canister call eliza_backend set_llm_provider '(variant { $(PROVIDER) })' --network $(NETWORK))"; \
		echo "$$result"; echo "$$result" | grep -q 'Ok' || { echo "The provider was NOT changed."; exit 1; }
	@$(DFX) canister call eliza_backend get_config --query --network $(NETWORK)

# FILE holds one Candid record, e.g.
#   (record { name = "Coo"; system_prompt = "You are Coo."; bio = vec { "..." }; style = vec { "Friendly" } })
set-character:
	@[ -f "$(FILE)" ] || { echo "usage: make set-character FILE=<file with a Candid Character record> [NETWORK=ic]"; exit 1; }
	@result="$$($(DFX) canister call eliza_backend update_character --argument-file "$(FILE)" --network $(NETWORK))"; \
		echo "$$result"; echo "$$result" | grep -q 'Ok' || { echo "The character was NOT changed."; exit 1; }

clean:
	rm -rf target $(FRONTEND)/dist
