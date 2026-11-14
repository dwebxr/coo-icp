#!/usr/bin/env bash
# Upgrades the backend canister the careful way: stop, snapshot, upgrade, verify that the state written before the
# upgrade is the state restored after it, and only then start. See "Upgrading a canister that holds conversations"
# in the README.
#
#   scripts/upgrade.sh <network>            network: ic | local
#
# It stops at the first problem and leaves the canister STOPPED, so nothing is served from a state that was not
# verified. It never passes --yes: dfx asks before installing an interface that is not backward compatible.
set -euo pipefail

NETWORK="${1:?usage: scripts/upgrade.sh <network>   (ic | local)}"
CANISTER=eliza_backend
export PATH="$HOME/.cargo/bin:$PATH" # dfx must find rustup's cargo (it has the wasm32 target)

call() { dfx canister call "$CANISTER" "$1" --query --network "$NETWORK"; }
say() { printf '\n== %s\n' "$*"; }

say "Building $CANISTER"
dfx build "$CANISTER" --network "$NETWORK"

say "Running version"
call health

if [ "$NETWORK" = "ic" ]; then
  echo
  echo "This upgrades the canister on MAINNET with the identity '$(dfx identity whoami)'."
  read -r -p "Type the canister name ($CANISTER) to continue: " answer
  [ "$answer" = "$CANISTER" ] || { echo "Aborted; nothing was changed."; exit 1; }
fi

say "Stopping (a version before 0.7.0 can take minutes while a model call is outstanding)"
dfx canister stop "$CANISTER" --network "$NETWORK"

say "Taking a snapshot"
snapshot_output="$(dfx canister snapshot create "$CANISTER" --network "$NETWORK" 2>&1)" || {
  echo "$snapshot_output"
  echo "No snapshot was taken (a canister holds a limited number: see 'dfx canister snapshot list')."
  echo "The canister is stopped and unchanged. Start it again with: dfx canister start $CANISTER --network $NETWORK"
  exit 1
}
echo "$snapshot_output"
snapshot_id="$(printf '%s\n' "$snapshot_output" | grep -oE '[0-9a-f]{16,}' | tail -1 || true)"
rollback="dfx canister snapshot load $CANISTER ${snapshot_id:-<snapshot id>} --network $NETWORK && dfx canister start $CANISTER --network $NETWORK"

say "Installing the new version (upgrade mode; the canister stays stopped)"
if ! dfx canister install "$CANISTER" --mode upgrade --network "$NETWORK"; then
  echo
  echo "The upgrade did not go through. If a hook trapped, the old version and its state are still in place."
  echo "The canister is stopped. Start it again with: dfx canister start $CANISTER --network $NETWORK"
  exit 1
fi

say "Checking that the state saved is the state restored"
logs="$(dfx canister logs "$CANISTER" --network "$NETWORK")"
saved="$(printf '%s\n' "$logs" | grep -oE 'pre_upgrade: [0-9]+ bytes' | tail -1 | grep -oE '[0-9]+' || true)"
restored="$(printf '%s\n' "$logs" | grep -oE 'State restored from stable memory successfully \([0-9]+ bytes' | tail -1 | grep -oE '[0-9]+' || true)"
printf '%s\n' "$logs" | grep -E 'pre_upgrade: |Restoring state|State restored' | tail -3 || true
if [ -z "$saved" ] || [ "$saved" != "$restored" ]; then
  echo
  echo "MISMATCH: saved '${saved:-nothing}' bytes, restored '${restored:-nothing}'."
  echo "The canister is left stopped. To go back to the state before the upgrade:"
  echo "  $rollback"
  echo "(then install the previous Wasm if the code must go back too)."
  exit 1
fi
echo "Saved and restored: $saved bytes."

say "Starting"
dfx canister start "$CANISTER" --network "$NETWORK"
call health
call get_metrics

cat <<EOF

Done. Still to do by hand:
  1. Send one real message through the frontend: it is the test that the model answers.
     If it does not, go back with:
       dfx canister stop $CANISTER --network $NETWORK && $rollback
  2. Deploy the frontend:  make upgrade-frontend NETWORK=$NETWORK
  3. Once you are sure, the snapshot can be deleted:
       dfx canister snapshot delete $CANISTER ${snapshot_id:-<snapshot id>} --network $NETWORK
EOF
