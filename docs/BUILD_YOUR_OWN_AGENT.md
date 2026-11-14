# Build your own agent from Coo

Coo is a small, complete chat agent: one Rust canister, one React frontend. This guide shows what to change to
make it yours. The [README](../README.md) describes how it works, its limits, and how to deploy and upgrade it;
read its "Known gaps" before putting anything public on mainnet.

## 1. Get it running

```bash
git clone https://github.com/<you>/coo-icp && cd coo-icp
make setup
make deploy-local
```

A local deployment answers with the fallback provider (an echo that says what it is): the LLM canister exists
on mainnet only.

## 2. Give it a character

A character is a name, a system prompt, and optional bio and style lines. All of it becomes the system message of
every model request:

```
<system prompt>

About you:
- Your name is <name>
- <bio line>

Your style:
- <style line>
```

**At runtime** (controllers of the canister only) — put a Candid record in a file and apply it:

```bash
cat > character.did <<'CHAR'
(record {
  name = "Ada";
  system_prompt = "You are Ada, a patient maths tutor. Explain step by step and check each step with the student.";
  bio = vec { "A tutor for school mathematics"; "Never gives the final answer before the student has tried" };
  style = vec { "Encouraging"; "Precise" }
})
CHAR
make set-character FILE=character.did                 # local
make set-character FILE=character.did NETWORK=ic      # mainnet
```

It applies to every conversation from its next message. Limits: name 64 bytes, system prompt 2 000 bytes, at most
8 bio lines of 200 bytes and 8 style lines of 100 bytes, and 3 000 bytes for the combined system message; the
canister refuses a character over them.

**As the default of your build** — edit `default_character()` in `src/eliza_backend/src/state.rs`. It is what a
freshly installed canister starts with. An existing canister keeps the character it has.

The model is Llama 3.1 8B with a 10 KiB request limit. Keep the prompt short and concrete; it does not follow long
rule lists well.

## 3. Make the frontend yours

| What | Where |
|------|-------|
| Name, tagline, sign-in screen | `src/eliza_frontend/src/components/SignIn.tsx`, `index.html` |
| Suggestions on an empty conversation | `SUGGESTIONS` in `src/components/Chat.tsx` |
| Avatar | `src/assets/coo.webp`, `coo-sm.webp`, `public/favicon.png`, `public/apple-touch-icon.png` |
| Colours, spacing | custom properties at the top of `src/index.css` (light and dark) |
| Footer links | `src/links.ts` |
| Your canister id | `MAINNET_BACKEND` in `src/network.ts` (after your first mainnet installation) |

`npm run dev` with `?mock=chat` (or `empty`, `signedout`, …) shows the UI states without a canister.
The Content-Security-Policy in `public/.ic-assets.json5` allows only the app's own files and the Internet
Computer's API; if you add fonts, images or requests from elsewhere, add their origins there, and run
`make e2e-local` to see that the policy still lets the app work.

## 4. Deploy

```bash
make install-ic          # first installation; asks before it does anything
```

Put the backend id that dfx prints (also in `canister_ids.json`) into `src/network.ts`, then
`make upgrade-frontend-ic`. From then on use `make upgrade-ic` for the backend: it stops the canister, takes a
snapshot and verifies the state after the upgrade (README, "Upgrading a canister that holds conversations").

## 5. Change the canister

| To change | Look at |
|-----------|---------|
| Limits (message size, turns kept, storage budget, chats at once, model calls per minute) | `src/eliza_backend/src/limits.rs` — the constants are checked against each other at compile time |
| What the model is sent | `build_prompt` and `system_text` in `llm.rs` (pure functions with unit tests) |
| The model | `LLM_MODEL` in `llm.rs`; the LLM canister's README lists the models it serves |
| Adding a method | `lib.rs` (the method), `inspect_message` in `lib.rs` and `accepts_ingress` in `limits.rs` (who may call it as an update), then `make candid && make declarations` |
| What is kept across upgrades | `stable.rs` — add fields as `opt`, bump `SCHEMA_VERSION`, and add a test that reads state written by the previous version |

Rules that keep it safe:

- Every method that changes state checks its caller itself. `inspect_message` only saves cycles; calls from other
  canisters never pass through it.
- Anything a caller can make the canister store must be bounded and counted in `store.rs`.
- Run `make test-canister` before every mainnet upgrade. It upgrades from the version that is deployed
  (`DEPLOYED_COMMIT` in the Makefile — set it to the commit you deployed) and at the full storage budget.
- The model's output is untrusted text. Do not let it trigger anything: the canister passes no tools to the model,
  and the frontend renders replies without interpreting HTML.

## Troubleshooting

- **"Only a controller of the canister can do this"** — `update_character` and `set_llm_provider` are for the
  canister's controllers: `dfx canister info eliza_backend --network ic` lists them.
- **"not accepted: sign in, call queries as queries…"** — an update call was refused at ingress: anonymous caller,
  a query method sent as an update, or an argument that is too large.
- **Replies are "[Local fallback: …]"** — the provider is `Fallback`. On mainnet: `make set-provider PROVIDER=OnChain NETWORK=ic`.
- **"The model did not answer."** — the LLM canister rejected the call or did not answer within five minutes;
  `make logs NETWORK=ic` shows the reason. Nothing was saved; the same message can be sent again.
- **"Coo is busy."** — another message of yours is being answered, or the canister is at one of its limits over
  all callers (README, "Limits").
