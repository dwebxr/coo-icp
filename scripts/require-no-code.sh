#!/usr/bin/env bash
# Refuses (exit 1) when any of the named canisters already has code on <network>. A first installation must never
# replace a running canister: that is what the upgrade targets are for (snapshot first, then a checked upgrade).
#
#   scripts/require-no-code.sh <network> <canister>...
#
# `dfx canister info` only reads (it asks the network for the canister's controllers and module hash). A canister
# that has no id on <network> yet has not been created, which is fine. Anything this cannot check — the network
# unreachable, an answer it does not understand — is a refusal too.
set -uo pipefail

NETWORK="${1:?usage: scripts/require-no-code.sh <network> <canister>...}"
shift
[ "$#" -gt 0 ] || { echo "usage: scripts/require-no-code.sh <network> <canister>..."; exit 2; }
export PATH="$HOME/.cargo/bin:$PATH"

refused=0
for canister in "$@"; do
  if ! info="$(dfx canister info "$canister" --network "$NETWORK" 2>&1)"; then
    if grep -qi 'cannot find canister id' <<<"$info"; then
      echo "$canister: not created on $NETWORK yet."
    else
      echo "$canister: could not check whether it has code on $NETWORK:"
      echo "$info"
      refused=1
    fi
    continue
  fi
  hash="$(sed -n 's/^Module hash: *//p' <<<"$info")"
  case "$hash" in
    None) echo "$canister: created on $NETWORK, no code installed yet." ;;
    0x*)
      echo "$canister: ALREADY HAS CODE on $NETWORK (module hash $hash)."
      refused=1
      ;;
    *)
      echo "$canister: could not read its module hash on $NETWORK:"
      echo "$info"
      refused=1
      ;;
  esac
done

if [ "$refused" -ne 0 ]; then
  if [ "$NETWORK" = ic ]; then upgrade="make upgrade-ic"; frontend="make upgrade-frontend-ic"; else
    upgrade="make upgrade NETWORK=$NETWORK"; frontend="make upgrade-frontend NETWORK=$NETWORK"; fi
  cat <<EOF

Nothing was installed. A first installation would replace what runs there.
- To update canisters that already run: $upgrade (the backend), $frontend (the frontend).
- If the ids in canister_ids.json are not yours (a fork of this repository), remove them from canister_ids.json
  before the first installation (docs/BUILD_YOUR_OWN_AGENT.md, "Deploy").
EOF
  exit 1
fi
