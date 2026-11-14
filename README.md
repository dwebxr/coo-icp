# Coo — an AI chat agent in an Internet Computer canister

<div align="center">
  <img src="overview.png" alt="Coo ICP" width="100%" />
  <p>
    <strong>A chat agent whose backend is one Rust canister on the Internet Computer. Replies come from Llama 3.1 8B through the Internet Computer's LLM canister; you sign in with Internet Identity.</strong>
  </p>
</div>

**Frontend:** https://4res3-liaaa-aaaas-qdqcq-cai.icp0.io/
**Backend (Candid UI):** https://a4gq6-oaaaa-aaaab-qaa4q-cai.raw.icp0.io/?id=4wfup-gqaaa-aaaas-qdqca-cai

| Canister | Mainnet ID |
|----------|------------|
| Backend  | `4wfup-gqaaa-aaaas-qdqca-cai` |
| Frontend | `4res3-liaaa-aaaas-qdqcq-cai` |

This README describes version **0.7.0**. What changed since 0.6.0 is at the end.

## What it is, and what it is not

- **The canister** (`src/eliza_backend`, Rust) keeps one conversation per signed-in caller, builds each model
  request, and guarantees that a message is saved at most once however often it is re-sent.
- **The model** is not in this canister. Each message is an inter-canister call to DFINITY's LLM canister
  (`w36hm-eqaaa-aaaal-qr76a-cai`, Llama 3.1 8B). How and where that canister runs inference is outside this
  repository, so "on-chain" here means: your messages go from your browser to canisters on the Internet Computer
  and nowhere else that this code controls.
- **The frontend** (`src/eliza_frontend`, React) is served by an asset canister.
- **Character format** — a name, a system prompt, bio lines and style lines — is inspired by
  [elizaOS](https://github.com/elizaOS/eliza). Coo is an independent Rust implementation; it contains no elizaOS
  code and does not run the elizaOS runtime.
- **Administration is centralised.** Whoever controls the canisters can replace their code, change the character
  and read the stored conversations. Conversations are not encrypted.

Earlier versions also contained multi-chain wallets, social posting, an OpenAI provider and more. All of that was
removed (most of it never worked); what is described here is what the code does.

## Limits

Every limit is enforced by the canister (`src/eliza_backend/src/limits.rs`).

| What | Limit | When it is reached |
|------|-------|--------------------|
| Message | 4 000 bytes (UTF-8) | refused (`Invalid`) |
| Saved reply | 16 000 bytes | cut |
| Conversation | 50 turns (25 exchanges) | oldest turns are dropped |
| Model request | 10 messages, 9 000 bytes; system message always first | older turns are left out or shortened |
| Stored conversations | 32 MiB counted, 100 000 entries | least recently used conversations are removed |
| Chats in progress, all callers | 20 | `Busy` |
| Model calls per minute, all callers | 60 | `Busy` |
| One model call | 300 s | `Failed`; nothing is saved |
| Character | name 64 B, system prompt 2 000 B, 8 bio lines × 200 B, 8 style lines × 100 B, 3 000 B combined | refused |

### Known gaps — read before relying on it

- **No admission control.** Anyone can create identities for free, so the limits above are over all callers, not
  per person. Someone who wants to can keep the 20 slots or the 60 calls per minute busy, so that everyone else
  sees "Coo is busy", and can push other people's idle conversations out of storage by filling it. What they
  cannot do: grow the canister's state without bound, make upgrades fail, or get a message saved twice. A captcha,
  an allow-list or payment would be needed to stop the rest; none is implemented.
- **The limits bound model calls and storage, not cycles.** The canister pays for every update call it accepts.
  `canister_inspect_message` refuses anonymous callers, oversized arguments, non-controller administration and
  queries sent as updates before they are paid for; a signed-in caller's well-formed calls are paid for even when
  they end in `Busy`.
- **History is best effort.** When storage is full, cleared conversations go first, then the conversation that has
  not been used for the longest time. Its owner then finds it empty.
- **`Clear` empties the conversation in the canister.** It does not reach snapshots or backups the operator took.
- **The JavaScript SDK is deprecated.** The frontend uses `@dfinity/agent` and `@dfinity/auth-client` 2.4.1 (pinned);
  their successor is `@icp-sdk`. The send/re-send logic depends on the exact behaviour of 2.4.1, so the migration is
  a project of its own and has not been done.
- **Sign-in still goes to `identity.internetcomputer.org`.** DFINITY recommends `id.ai`; both are the same service
  and no deadline has been announced. The switch is deferred until it is confirmed that no user's principal
  changes with it.
- **Operator risk.** The canister's controller is a single key. Keep it in an encrypted dfx identity or a hardware
  wallet; `dfx` warns when a plaintext identity is used on mainnet, and that warning should not be silenced.

## Run it locally

Needs [dfx](https://internetcomputer.org/docs/building-apps/getting-started/install) 0.30.2, Rust (the toolchain in
`rust-toolchain.toml` is installed by rustup on first use) and Node 22 or newer (`.nvmrc` says 24).

```bash
make setup           # wasm target, npm ci
make deploy-local    # local replica, Internet Identity, backend (fallback provider), frontend
```

The LLM canister exists only on mainnet. A local deployment uses the **fallback provider**, which echoes the start
of your message and says so.

Just the UI, without a replica: `cd src/eliza_frontend && npm run dev`, then open
`http://localhost:5173/?mock=chat` (other states: `signedout`, `empty`, `rich`, `slow`, `error`, `busy`, `offline`,
`lostreply`, `inprogress`, `historyerror`, `expiring`, …; see `src/dev/mockBackend.ts`). The real client against a
local canister, without Internet Identity: `http://localhost:5173/?dev-identity`. Both exist in development builds
only.

### Tests

| Command | What it runs |
|---------|--------------|
| `make test` | canister unit tests, frontend type check and unit tests |
| `make test-canister` | the above for the canister plus PocketIC integration tests, including the upgrade from the version deployed on mainnet (built from its commit) and an upgrade at the full storage budget |
| `make e2e` | browser tests against the mock (Chromium and WebKit) and the accessibility check; needs `npm run dev` running |
| `make e2e-local` | browser tests of the real client against a canister on a local replica, and the security headers of a locally deployed frontend |
| `make check` | formatting, clippy, and that the Candid file and the generated bindings match the code |

First time: `cd src/eliza_frontend && npx playwright install chromium webkit`.

## Deploying to mainnet

### First installation

```bash
make install-ic      # builds, asks for confirmation, runs `dfx deploy --network ic`
dfx canister update-settings eliza_backend --wasm-memory-limit 512MiB --network ic
```

A fresh canister uses the on-chain model and the default character. The Wasm memory limit keeps a runaway heap
from reaching the point where the canister can no longer be upgraded. It applies to update calls and to
`post_upgrade`, not to `pre_upgrade`. 512 MiB is about four times the most the tests measure — 117 MiB, while a
store at its full budget is being saved (`make test-canister` prints the numbers for several shapes of state).

### Upgrading a canister that holds conversations

Conversations live in the heap and cross an upgrade through `pre_upgrade` → stable memory → `post_upgrade`.

```bash
make test-canister   # includes the upgrade from the deployed version
make upgrade-ic      # scripts/upgrade.sh ic — the steps below, stopping at the first problem
```

What the script does, and what to check:

1. Builds the backend and shows `health` and `get_metrics` of the running canister.
2. **Stops** the canister. A version before 0.7.0 can take minutes to stop while a model call is outstanding.
3. Takes a **snapshot** (a canister holds a limited number; delete an old one first if it is refused).
4. Installs the new code in upgrade mode. dfx warns that the Candid interface changed incompatibly when methods
   were removed; that is expected for 0.6.0 → 0.7.0.
5. Compares two log lines: `pre_upgrade: N bytes serialized to stable memory; …` and
   `State restored from stable memory successfully (N bytes; …)`. The same `N` proves the state written just before
   the upgrade is the one restored. If they differ or one is missing, the script leaves the canister stopped and
   prints the command that loads the snapshot. Both lines end with the Wasm memory and the instructions the hook
   used — the only place where the memory needed by `pre_upgrade` can be seen.
6. Starts the canister and shows `health` and `get_metrics` again.

Before the **first upgrade from 0.6.0**, look at the size of what it holds: `health` (conversations) and
`dfx canister status eliza_backend --network ic` (memory). 0.7.0 reads the whole saved state before cutting it to
its limits, which takes about twice the state's size in memory for ordinary conversations and several times that
for a state made of very many tiny entries (close to 1 GiB near 0.6.0's 100 MB maximum). So do not set the Wasm
memory limit before this upgrade; set it afterwards, and only if `get_metrics` shows `wasm_memory_bytes` well
below it (memory a canister has grown into is not given back until the next upgrade).

Then, by hand:

- **Send one real message** through the frontend. The model call is a bounded-wait call since 0.7.0; this first
  message is the test that the LLM canister answers it. If it does not, roll back.
- `make upgrade-frontend-ic` deploys the frontend, and
  `cd src/eliza_frontend && node e2e/headers.mjs https://4res3-liaaa-aaaas-qdqcq-cai.icp0.io/` checks its headers.
- `dfx canister update-settings eliza_backend --wasm-memory-limit 512MiB --network ic` if not set yet (see above).

Deploy the backend first. A browser tab still running the 0.6.0 frontend keeps working against 0.7.0 (the four
methods it calls kept their types; a test checks this).

**Rolling back.** 0.6.0 cannot read the state 0.7.0 writes, so the only way back is the snapshot:
`dfx canister stop eliza_backend --network ic && dfx canister snapshot load eliza_backend <id> --network ic`, then
install the old Wasm and start. Everything saved after the snapshot is lost, and revisions go back; browsers that
saw a newer revision detect that and reload the conversation from the canister.

**If a hook traps.** A trap in `pre_upgrade` or `post_upgrade` aborts the upgrade and the old version keeps running
with its data. Fix the cause and upgrade again. Never use `--skip-pre-upgrade`: it restores the state of the
previous upgrade, or none.

**What the 0.6.0 → 0.7.0 upgrade changes in the state.** The system message and the character copy stored in every
conversation are dropped. A stored OpenAI API key is dropped. The default character is replaced by the new default
if it was never customised. Conversations larger than the new limits are cut from the oldest turn; if the total is
over the storage budget, the least recently used conversations are removed. When anything was cut, `post_upgrade`
logs `Brought the state of schema version 2 within this version's limits: N turns and M entries dropped`; on a
canister whose conversations are ordinary chat messages this line does not appear.

### Operating it

```bash
make status NETWORK=ic                                  # health, config, metrics
make logs NETWORK=ic
make set-provider PROVIDER=Fallback                     # local; add NETWORK=ic for mainnet
make set-character FILE=character.did NETWORK=ic        # a Candid record in a file
```

`get_metrics` reports `stored_bytes` (against the 32 MiB budget), `entries`, `revision_floor` (it rises when
conversations are removed to make room), `chats_in_flight`, and the sizes of stable and Wasm memory. `health` only
says that the canister answers and has its state; it does not test the model.

Anonymous update calls are refused before they reach a method, so the Candid UI can call queries only (or use
`dfx canister call` with an identity).

## Interface (0.7.0)

The Candid file is `src/eliza_backend/eliza_backend.did`; a test fails when it does not match the code.

### Chat

```candid
send_message : (request_id : text, base_revision : nat64, content : text)
    -> (variant { Ok : record { reply : text; revision : nat64 }; Err : ChatError });
get_conversation : () -> (record { messages : vec Message; revision : nat64 }) query;
get_message_status : (request_id : text) -> (variant { Unknown; Pending; Done : record { reply : text; revision : nat64 } }) query;
clear_conversation : () -> (nat64);   // the new revision
```

The client picks a request id (1–64 printable ASCII bytes, e.g. a UUID) and names the revision of the conversation
it continues, as it got it from `get_conversation`. Every saved exchange and every clear moves the revision on, and
a caller's revision never goes back — also when its conversation is removed to make room.

- Repeating a request id returns the saved reply; the model is not asked again. So after a lost response the same
  request can simply be sent again.
- A request whose revision the conversation has moved past is a `Conflict` and never runs. This is what makes one
  (request id, revision) pair saved **at most once**, and why nothing sent before a clear can be saved after it.
  (The guarantee is about revisions the canister handed out. It also ends where the state itself is rolled back
  to a snapshot.)

`ChatError`:

| Variant | Meaning | The client may |
|---------|---------|----------------|
| `Invalid(text)` | wrong whenever it is sent: anonymous caller, malformed id, id already used for another message, empty or too long | not retry this request |
| `Busy` | nothing saved; another message of this caller is being answered, or the canister is at a limit | send the same request again later |
| `InProgress` | this request id is being answered | poll `get_message_status` |
| `Conflict` | the conversation is not at `base_revision` | reload the conversation |
| `Cleared` | the conversation was cleared while the reply was generated | reload |
| `Failed(text)` | the model did not answer, or not within five minutes; nothing saved | send the same request again |

### Character and configuration

```candid
get_character : () -> (Character) query;
update_character : (Character) -> (variant { Ok; Err : text });      // controllers only
get_config : () -> (record { llm_provider : LlmProvider }) query;
set_llm_provider : (LlmProvider) -> (variant { Ok; Err : text });    // controllers only; OnChain | Fallback
```

The whole character — name, system prompt, bio and style — is the system message of every model request, and a
new character applies to existing conversations from their next message. Both settings survive upgrades.

### Status

`health`, `version`, `get_metrics`, `get_conversation_count` are public queries.

## Project structure

```
├── dfx.json, canister_ids.json, Cargo.toml, rust-toolchain.toml, Makefile
├── scripts/upgrade.sh                 # the upgrade runbook
├── src/
│   ├── eliza_backend/                 # the canister
│   │   ├── eliza_backend.did          # Candid interface (generated: make candid)
│   │   ├── src/
│   │   │   ├── lib.rs                 # entry points, upgrade hooks, ingress filter
│   │   │   ├── api.rs                 # public types
│   │   │   ├── limits.rs              # every limit, input validation
│   │   │   ├── store.rs               # conversations, byte budget, eviction, revision floor
│   │   │   ├── flight.rs              # chats in progress, concurrency and rate limits
│   │   │   ├── llm.rs                 # model request, call to the LLM canister, local fallback
│   │   │   ├── chat.rs                # send / status / clear
│   │   │   ├── stable.rs              # state across upgrades, migration from 0.6.0
│   │   │   └── state.rs
│   │   └── tests/pocketic.rs          # integration tests
│   ├── mock_llm/                      # test double for the LLM canister (never deployed)
│   └── eliza_frontend/
│       ├── public/.ic-assets.json5    # security headers of the asset canister
│       ├── e2e/                       # smoke.mjs (mock), local.mjs (real canister), headers.mjs, a11y.mjs
│       └── src/
│           ├── backend.ts             # Internet Identity + canister calls
│           ├── network.ts             # local replica or mainnet
│           ├── useCoo.ts              # session and chat state
│           ├── lib/                   # delivery (send/re-send), floors, markdown, thread, sessionSync, limits
│           ├── components/
│           ├── declarations/          # Candid bindings (generated: make declarations)
│           └── dev/                   # development-only mock backend and identity
└── docs/BUILD_YOUR_OWN_AGENT.md
```

## Security notes

- **Callers.** Chat methods require a non-anonymous caller; each caller sees only its own conversation.
  `update_character` and `set_llm_provider` require a controller of the canister.
- **Model output is untrusted.** The frontend renders replies with its own Markdown subset as React elements —
  no HTML is interpreted, images are not loaded, links are `http(s)` only and cannot show one site while pointing
  to another. The asset canister sends a Content-Security-Policy without inline script or style
  (`public/.ic-assets.json5`).
- **Sessions.** The Internet Identity delegation is checked before every call; ten minutes without activity in any
  tab signs every tab out, as does signing out in one of them.
- **Stored data.** Conversations are stored in the canister unencrypted; node providers of the subnet and the
  canister's controllers can read them.

## Tech stack

Rust, ic-cdk 0.20, candid · React 18, TypeScript, Vite 6 · `@dfinity/agent` and `@dfinity/auth-client` 2.4.1 ·
Internet Identity · PocketIC, vitest, Playwright.

## Changes in 0.7.0

Breaking: `chat`, `get_conversation_history`, `set_admin`, `store_encrypted_api_key` and
`transform_openai_response` are removed, together with the OpenAI provider (it could not reach consensus on
mainnet, and its key was stored in plaintext). `get_character` and `get_config` return records instead of `opt`,
and `Config` only holds the provider. Administration is for controllers.

New: all the limits above; eviction with a revision floor; the ingress filter; bounded-wait model calls; the whole
character in the system message, which is no longer dropped after five exchanges; the provider setting survives
upgrades; security headers and a Content-Security-Policy for the frontend; sign-out and idle sign-out across tabs.

## License

MIT · Repository: https://github.com/dwebxr/coo-icp
