# Coo — build, test, deploy.  `make help` lists the targets.
#
# NETWORK selects the network for the targets that talk to a canister (default: local); it is local or ic. The local
# targets name the local network in every dfx call, so a DFX_NETWORK in the environment cannot send them anywhere
# else. Nothing here passes --yes to dfx or touches mainnet without NETWORK=ic or an explicit *-ic target. Every
# target that changes something on mainnet asks first: install-ic, upgrade-ic (and upgrade NETWORK=ic),
# upgrade-frontend-ic (and upgrade-frontend NETWORK=ic), and set-provider and set-character with NETWORK=ic.
# (status and logs with NETWORK=ic only read.)

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

# The question a target asks before it changes mainnet: $(call confirm-ic,<what it does>,<word to type>).
confirm-ic = @read -r -p "This $(1) on MAINNET with the identity '$$($(DFX) identity whoami)'. Type '$(2)' to continue: " answer && [ "$$answer" = "$(2)" ] || { echo "Aborted; nothing was changed."; exit 1; }

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
	@echo "Mainnet (each asks before it changes anything; so do set-provider and set-character with NETWORK=ic)"
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
# The upgrade tests start from two older builds, made from their commits (so they need the git history):
#   DEPLOYED_COMMIT  what runs on mainnet now. Set it to the released commit after every mainnet upgrade.
#   V060_COMMIT      0.6.0, the last version that wrote the old state layout (schema 2). Kept for as long as
#                    stable.rs still reads that layout.
# The commit is part of the file name, so changing one of them rebuilds it.
DEPLOYED_COMMIT := 5879071ef912e9df391805dccd7c6d4dbf9ac2b1
V060_COMMIT := 5e9c5a29587dc75553d37d43bf1ce4add6aeabd5
DEPLOYED_PATHS := Cargo.toml Cargo.lock rust-toolchain.toml src/eliza_backend src/mock_llm
DEPLOYED_WASM := target/older/deployed-$(DEPLOYED_COMMIT).wasm
# The deployed version built with tiny limits, so a test can make it evict (raising its revision floor) first.
DEPLOYED_SMALL_WASM := target/older/deployed-small-$(DEPLOYED_COMMIT).wasm
V060_WASM := target/older/v060-$(V060_COMMIT).wasm

# $(call build-older,<commit>,<paths to take from it>,<output>,<cargo flags>)
# The sources are extracted to, and built in, a directory named after the output (<output> without .wasm, plus -src),
# so two outputs share nothing, also when they are made from the same commit and built in parallel (make -j).
# The result is copied under a temporary name and renamed, so an interrupted copy is never taken for a build.
define build-older
	rm -rf $(basename $(3))-src && mkdir -p $(basename $(3))-src
	git archive $(1) $(2) | tar -x -C $(basename $(3))-src
	cd $(basename $(3))-src && $(WASM_BUILD) -p eliza_backend $(4)
	cp $(basename $(3))-src/target/wasm32-unknown-unknown/release/eliza_backend.wasm $(3).tmp
	mv $(3).tmp $(3)
endef

$(DEPLOYED_WASM):
	$(call build-older,$(DEPLOYED_COMMIT),$(DEPLOYED_PATHS),$@,)

$(DEPLOYED_SMALL_WASM):
	$(call build-older,$(DEPLOYED_COMMIT),$(DEPLOYED_PATHS),$@,--features small-limits)

# 0.6.0 was one file, in a workspace without the mock canister and without a pinned toolchain.
$(V060_WASM):
	$(call build-older,$(V060_COMMIT),Cargo.toml Cargo.lock src/eliza_backend/Cargo.toml src/eliza_backend/src/lib.rs,$@,)

test-canister: $(DEPLOYED_WASM) $(DEPLOYED_SMALL_WASM) $(V060_WASM)
	$(WASM_BUILD) -p eliza_backend -p mock_llm
	$(WASM_BUILD) -p eliza_backend --features test-hooks --target-dir target/test-hooks
	$(WASM_BUILD) -p eliza_backend --features small-limits,test-hooks --target-dir target/small-limits
	POCKET_IC_BIN="$${POCKET_IC_BIN:-$$(dfx cache show)/pocket-ic}" \
		ELIZA_DEPLOYED_WASM="$(CURDIR)/$(DEPLOYED_WASM)" ELIZA_DEPLOYED_SMALL_WASM="$(CURDIR)/$(DEPLOYED_SMALL_WASM)" \
		ELIZA_V060_WASM="$(CURDIR)/$(V060_WASM)" \
		$(CARGO) test --locked -p eliza_backend -- --include-ignored --test-threads=1

# Against the mock backend of a running dev server (cd src/eliza_frontend && npm run dev).
e2e:
	cd $(FRONTEND) && npm run e2e && npm run a11y

# The real client against a canister on the local replica (fallback provider), served by the local asset canister
# under the security headers it sends: sign-in, queries, update calls and the certified read all run under the
# deployed Content-Security-Policy, and a violation fails the tests. scripts/e2e-local.sh deploys the test bundle
# (build-e2e) and puts the normal one back on any exit, an interruption included.
e2e-local: start
	$(DFX) deploy eliza_backend --network local
	$(MAKE) set-provider PROVIDER=Fallback NETWORK=local
	scripts/e2e-local.sh

# ---------------------------------------------------------------------------------------------- mainnet

# First installation only. A canister that holds conversations is upgraded with `make upgrade-ic`. The check says
# early, and in words, when the canisters named in canister_ids.json already have code on mainnet; the deploy itself
# is in install mode, which the Internet Computer refuses for a canister that has code (so code installed between
# the check and the deploy is not replaced either).
install-ic:
	scripts/require-no-code.sh ic eliza_backend eliza_frontend
	$(call confirm-ic,creates and installs the canisters and spends cycles,install)
	$(MAKE) build-ic
	$(DFX) deploy --network ic --mode install
	@echo "Now set the Wasm memory limit:  dfx canister update-settings eliza_backend --wasm-memory-limit 512MiB --network ic"

upgrade:
	scripts/upgrade.sh $(NETWORK)

upgrade-ic:
	scripts/upgrade.sh ic

# Uploads the frontend only (`dfx deploy eliza_frontend` would also redeploy the backend it depends on).
upgrade-frontend:
ifeq ($(NETWORK),ic)
	$(call confirm-ic,uploads the frontend,upgrade)
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
ifeq ($(NETWORK),ic)
	$(call confirm-ic,sets the model provider to $(PROVIDER),$(PROVIDER))
endif
	@result="$$($(DFX) canister call eliza_backend set_llm_provider '(variant { $(PROVIDER) })' --network $(NETWORK))"; \
		echo "$$result"; echo "$$result" | grep -q 'Ok' || { echo "The provider was NOT changed."; exit 1; }
	@$(DFX) canister call eliza_backend get_config --query --network $(NETWORK)

# FILE holds one Candid record, e.g.
#   (record { name = "Coo"; system_prompt = "You are Coo."; bio = vec { "..." }; style = vec { "Friendly" } })
# The file name reaches the shell as data (an environment variable), never as part of the command line.
set-character: export COO_CHARACTER_FILE = $(value FILE)
set-character:
	@[ -n "$$COO_CHARACTER_FILE" ] && [ -f "$$COO_CHARACTER_FILE" ] || { echo "usage: make set-character FILE=<file with a Candid Character record> [NETWORK=ic]"; exit 1; }
ifeq ($(NETWORK),ic)
	$(call confirm-ic,replaces the character,character)
endif
	@result="$$($(DFX) canister call eliza_backend update_character --argument-file "$$COO_CHARACTER_FILE" --network $(NETWORK))"; \
		echo "$$result"; echo "$$result" | grep -q 'Ok' || { echo "The character was NOT changed."; exit 1; }

clean:
	rm -rf target $(FRONTEND)/dist
