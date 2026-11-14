#!/usr/bin/env bash
# The second half of `make e2e-local` (the backend is deployed by then): builds the frontend with the development
# identity allowed (make build-e2e), deploys that bundle to the LOCAL frontend canister, runs e2e/headers.mjs and
# e2e/local.mjs against it, and puts the normal bundle back — on any exit: success, failure, or an interruption
# (Ctrl-C, SIGTERM, SIGHUP). If putting it back fails, it says so loudly: until that is redone, ?dev-identity works
# on the local frontend canister (only there: the switch is for local hosts only, see src/main.tsx).
set -uo pipefail

cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH" # dfx must find rustup's cargo (it has the wasm32 target)
MAKE="${MAKE:-make}"
FRONTEND=src/eliza_frontend

restored=false
put_back() {
  $restored && return 0
  restored=true
  trap '' INT TERM HUP
  echo
  echo "== Putting the normal bundle back on the local frontend canister"
  # Another Ctrl-C must not stop this half-way. Ignoring the signal here is not enough (node resets it for itself, so
  # the build would still die), so the restore runs as a process group of its own, which a Ctrl-C at the terminal
  # does not reach.
  set -m
  ("$MAKE" build && dfx deploy eliza_frontend --network local) &
  local job=$!
  wait "$job"
  local status=$?
  set +m
  if [ "$status" -eq 0 ]; then
    echo "The normal bundle is deployed again."
    return 0
  fi
  {
    echo
    echo "!!! COULD NOT PUT THE NORMAL BUNDLE BACK. The local frontend canister still serves the e2e test bundle, in"
    echo "!!! which ?dev-identity works. Redo it with:  make build && dfx deploy eliza_frontend --network local"
  } >&2
  return 1
}

on_exit() {
  local status=$?
  put_back || status=1
  echo
  if [ "$status" -eq 0 ]; then echo "e2e-local: all passed"; else echo "e2e-local: FAILED (exit status $status)"; fi
  exit "$status"
}
trap on_exit EXIT
# An interruption ends the script through on_exit too.
trap 'exit 130' INT
trap 'exit 143' TERM
trap 'exit 129' HUP

"$MAKE" build-e2e || exit 1
dfx deploy eliza_frontend --network local || exit 1
url="http://$(dfx canister id eliza_frontend --network local).localhost:4943"
(cd "$FRONTEND" && node e2e/headers.mjs "$url/" && BASE="$url" node e2e/local.mjs)
