# Coo — build, test, deploy.  `make help` lists the targets.
#
# NETWORK selects the network for the targets that talk to a canister (default: local); it is local or ic. The local
# targets name the local network in every dfx call, so a DFX_NETWORK in the environment cannot send them anywhere
# else. Nothing here passes --yes to dfx or touches mainnet without NETWORK=ic or an explicit *-ic target, and each
# of those asks before it changes anything.

SHELL := /bin/bash
NETWORK ?= local

# The variables an operator sets end up in command lines, so they are checked here, by make, before any shell sees
# them (`make upgrade 'NETWORK=ic; echo INJECTED'` stops here).
ifneq ($(NETWORK),local)
ifneq ($(NETWORK),ic)
$(error NETWORK must be local or ic)
endif
endif
ifneq ($(origin PROVIDER),undefined)
ifneq ($(PROVIDER),OnChain)
ifneq ($(PROVIDER),Fallback)
$(error PROVIDER must be OnChain or Fallback)
endif
endif
endif

FRONTEND := src/eliza_frontend
# dfx and cargo must find rustup's toolchain (it has the wasm32 target), also where another cargo is first in PATH.
CARGO := PATH="$$HOME/.cargo/bin:$$PATH" cargo
DFX := PATH="$$HOME/.cargo/bin:$$PATH" dfx
WASM_BUILD := $(CARGO) build --target wasm32-unknown-unknown --release --locked

.PHONY: help setup start stop deploy-local build build-ic candid declarations check test test-canister e2e \
        e2e-local build-e2e install-ic upgrade upgrade-ic upgrade-frontend upgrade-frontend-ic status logs \
        set-provider set-character clean

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
	@echo "  make e2e-local          the real client, served by the local asset canister under its security headers"
	@echo "  make check              formatting, clippy, Candid file and generated bindings up to date"
	@echo
	@echo "Generated files"
	@echo "  make candid             src/eliza_backend/eliza_backend.did from the code"
	@echo "  make declarations       the frontend's Candid bindings from that file"
	@echo
	@echo "Mainnet (each asks before it changes anything)"
	@echo "  make install-ic         first installation; refuses if the canisters already have code"
	@echo "  make upgrade-ic         upgrade the backend: stop, snapshot, upgrade, check, start (scripts/upgrade.sh)"
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
	@command -v node >/dev/null || { echo "node not found (22.12+, 24 or 26+; .nvmrc says 24)"; exit 1; }
	PATH="$$HOME/.cargo/bin:$$PATH" rustup target add wasm32-unknown-unknown
	cd $(FRONTEND) && npm ci

# Starts the local replica unless one is running. (State of an earlier run is kept; `dfx start --clean` wipes it.)
# `dfx start` and `dfx stop` only ever act on the local replica.
start:
	@$(DFX) ping local >/dev/null 2>&1 || $(DFX) start --background

stop:
	$(DFX) stop

# The frontend is built after the backend is deployed: the build reads the local canister ids from .env.
# (`dfx deps pull` downloads Internet Identity's Wasm from mainnet, which is where it is published; it changes nothing
# there. Everything else is deployed to the local replica.)
deploy-local: start
	$(DFX) deps pull --network ic
	$(DFX) deps init --network local
	$(DFX) deps deploy --network local
	$(DFX) deploy eliza_backend --network local
	$(MAKE) set-provider PROVIDER=Fallback NETWORK=local
	$(MAKE) build
	$(DFX) deploy eliza_frontend --network local
	@echo
	@echo "Frontend: http://$$($(DFX) canister id eliza_frontend --network local).localhost:4943/"
	@echo "The LLM canister exists on mainnet only; this deployment answers with the fallback provider."

# Production build of the frontend for a LOCAL deployment (it contains the local canister ids from .env). Fails if
# development-only code got into it.
build:
	cd $(FRONTEND) && npm run build
	scripts/check-bundle.sh $(FRONTEND)/dist

# Production build for mainnet: the same, with no local canister ids in it, so it does not depend on what was
# last deployed locally.
build-ic:
	cd $(FRONTEND) && CANISTER_ID_ELIZA_BACKEND= CANISTER_ID_INTERNET_IDENTITY= npm run build
	scripts/check-bundle.sh $(FRONTEND)/dist

# For `make e2e-local` only: a production build in which ?dev-identity works on a local host (src/main.tsx), so the
# real client can be tested without Internet Identity under the deployed Content-Security-Policy. It is deployed to
# the local replica only, and replaced by the normal bundle afterwards.
build-e2e:
	cd $(FRONTEND) && VITE_ALLOW_DEV_IDENTITY=1 npm run build

# ---------------------------------------------------------------------------------------------- generated files

# The Candid file from the code (a unit test fails when it is out of date).
candid:
	$(WASM_BUILD) -p eliza_backend
	candid-extractor target/wasm32-unknown-unknown/release/eliza_backend.wasm > src/eliza_backend/eliza_backend.did

# The frontend's bindings from the Candid file. dfx also writes an index.js that creates an actor from process.env;
# the app creates its own (src/backend.ts), so that file is not kept.
declarations:
	$(DFX) generate eliza_backend --network local
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

# The real client against a canister on the local replica (fallback provider), served by the local asset canister
# under the security headers it sends: sign-in, queries, update calls and the certified read all run under the
# deployed Content-Security-Policy, and a violation fails the tests. The test bundle (build-e2e) is replaced by the
# normal one afterwards, whatever the result. (The tests' exit status waits in a file while that happens, so that
# the bundle is put back in recipe lines of their own: a line that runs $(MAKE) is run even by `make -n`, and the
# tests must not be.)
E2E_STATUS := target/e2e-local.status
e2e-local: start
	$(DFX) deploy eliza_backend --network local
	$(MAKE) set-provider PROVIDER=Fallback NETWORK=local
	$(MAKE) build-e2e
	$(DFX) deploy eliza_frontend --network local
	@mkdir -p $(dir $(E2E_STATUS)) && echo 1 > $(E2E_STATUS)
	url="http://$$($(DFX) canister id eliza_frontend --network local).localhost:4943"; \
		( cd $(FRONTEND) && node e2e/headers.mjs "$$url/" && BASE="$$url" node e2e/local.mjs ); \
		echo $$? > $(E2E_STATUS)
	@echo; echo "Putting the normal bundle back on the local frontend canister"
	$(MAKE) build
	$(DFX) deploy eliza_frontend --network local
	@if [ "$$(cat $(E2E_STATUS))" = 0 ]; then echo "e2e-local: all passed"; else echo "e2e-local: FAILED (above)"; exit 1; fi

# ---------------------------------------------------------------------------------------------- mainnet

# First installation only. A canister that holds conversations is upgraded with `make upgrade-ic`; this refuses to
# run when the canisters named in canister_ids.json already have code on mainnet.
install-ic:
	scripts/require-no-code.sh ic eliza_backend eliza_frontend
	@echo "This creates and installs the canisters on MAINNET with the identity '$$($(DFX) identity whoami)' and spends cycles."
	@read -r -p "Type 'install' to continue: " answer && [ "$$answer" = "install" ] || { echo "Aborted."; exit 1; }
	$(MAKE) build-ic
	scripts/require-no-code.sh ic eliza_backend eliza_frontend
	$(DFX) deploy --network ic
	@echo "Now set the Wasm memory limit:  dfx canister update-settings eliza_backend --wasm-memory-limit 512MiB --network ic"

upgrade:
	scripts/upgrade.sh $(NETWORK)

upgrade-ic:
	scripts/upgrade.sh ic

# Uploads the frontend only (`dfx deploy eliza_frontend` would also redeploy the backend it depends on).
upgrade-frontend:
ifeq ($(NETWORK),ic)
	@echo "This uploads the frontend to MAINNET with the identity '$$($(DFX) identity whoami)'."
	@read -r -p "Type 'upgrade' to continue: " answer && [ "$$answer" = "upgrade" ] || { echo "Aborted; nothing was changed."; exit 1; }
endif
	$(MAKE) $(if $(filter ic,$(NETWORK)),build-ic,build)
	$(DFX) build eliza_frontend --network $(NETWORK)
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
# PROVIDER was checked at the top.
set-provider:
	$(if $(PROVIDER),,$(error usage: make set-provider PROVIDER=OnChain|Fallback [NETWORK=ic]))
	@result="$$($(DFX) canister call eliza_backend set_llm_provider '(variant { $(PROVIDER) })' --network $(NETWORK))"; \
		echo "$$result"; echo "$$result" | grep -q 'Ok' || { echo "The provider was NOT changed."; exit 1; }
	@$(DFX) canister call eliza_backend get_config --query --network $(NETWORK)

# FILE holds one Candid record, e.g.
#   (record { name = "Coo"; system_prompt = "You are Coo."; bio = vec { "..." }; style = vec { "Friendly" } })
# The file name reaches the shell as data (an environment variable), never as part of the command line.
set-character: export COO_CHARACTER_FILE = $(value FILE)
set-character:
	@[ -n "$$COO_CHARACTER_FILE" ] && [ -f "$$COO_CHARACTER_FILE" ] || { echo "usage: make set-character FILE=<file with a Candid Character record> [NETWORK=ic]"; exit 1; }
	@result="$$($(DFX) canister call eliza_backend update_character --argument-file "$$COO_CHARACTER_FILE" --network $(NETWORK))"; \
		echo "$$result"; echo "$$result" | grep -q 'Ok' || { echo "The character was NOT changed."; exit 1; }

clean:
	rm -rf target $(FRONTEND)/dist
