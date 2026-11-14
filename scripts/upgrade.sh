#!/usr/bin/env bash
# Upgrades the backend canister the careful way: stop, snapshot, upgrade, check, and only then start. See
# "Upgrading a canister that holds conversations" in the README.
#
#   scripts/upgrade.sh <network>            network: ic | local
#
# What the checks after the upgrade establish, and no more:
# - the upgrade hooks of THIS upgrade ran: their log lines are newer than the newest line before the install;
# - the state pre_upgrade wrote has the same length as the state post_upgrade read back and decoded. post_upgrade
#   traps (and the upgrade is undone) when it cannot decode the state, so this is a length check on top of a
#   successful decode — not proof that the contents are identical;
# - the code now installed is the Wasm this script built (module hashes).
#
# It stops at the first problem and says which step failed, whether the canister is stopped or running as far as the
# script knows, and the command that goes back to the snapshot. A problem before the start leaves the canister
# STOPPED, so nothing is served from a state that was not checked; a problem in the checks after the start leaves it
# running. It never passes --yes: dfx asks before installing an interface that is not backward compatible.
set -euo pipefail

NETWORK="${1:?usage: scripts/upgrade.sh <network>   (ic | local)}"
case "$NETWORK" in
  ic | local) ;;
  *) echo "usage: scripts/upgrade.sh <network>   (ic | local)"; exit 2 ;;
esac
CANISTER=eliza_backend
export PATH="$HOME/.cargo/bin:$PATH" # dfx must find rustup's cargo (it has the wasm32 target)
# What `dfx canister install` installs: dfx's copy of the built Wasm (dfx.json does not ask for gzip).
BUILT_WASM=".dfx/$NETWORK/canisters/$CANISTER/$CANISTER.wasm"

START="dfx canister start $CANISTER --network $NETWORK"
STOP="dfx canister stop $CANISTER --network $NETWORK"
STATUS="dfx canister status $CANISTER --network $NETWORK"

# Where the script is, for the report if it stops: one of
#   before-stop stopping stopped snapshotted installing installed starting started
phase=before-stop
step="building"
snapshot_id=""
reported=false

call() { dfx canister call "$CANISTER" "$1" --query --network "$NETWORK"; }
say() { printf '\n== %s\n' "$*"; }
load() { echo "dfx canister snapshot load $CANISTER $snapshot_id --network $NETWORK"; }

# What state the canister is in, as far as the script knows, and how to go on from there.
where_it_is() {
  case "$phase" in
    before-stop)
      echo "The canister was not touched: it is running the version it ran before." ;;
    stopping)
      echo "The stop did not confirm: the canister may be stopping, stopped or running. Look with: $STATUS"
      echo "Start it again with: $START" ;;
    stopped)
      echo "The canister is STOPPED and unchanged. Start it again with: $START"
      echo "(If a snapshot was taken after all, 'dfx canister snapshot list $CANISTER --network $NETWORK' shows it.)" ;;
    snapshotted)
      echo "The canister is STOPPED and unchanged (snapshot $snapshot_id was taken). Start it again with: $START" ;;
    installing)
      echo "The canister is STOPPED. The install did not report success."
      echo "If it did not happen (a hook trapped, say), the old version and its state are in place: $START"
      echo "To be sure either way, go back to the snapshot (its code and state) and start:"
      echo "  $(load) && $START" ;;
    installed)
      echo "The canister is STOPPED, with the new version installed. To go back to the code and state of the snapshot:"
      echo "  $(load) && $START" ;;
    starting)
      echo "The start did not confirm: the canister may be running the new version. Look with: $STATUS"
      echo "To go back to the code and state of the snapshot:"
      echo "  $STOP && $(load) && $START" ;;
    started)
      echo "The canister is RUNNING the new version. To go back to the code and state of the snapshot:"
      echo "  $STOP && $(load) && $START" ;;
  esac
}

# Stops the script with a reason.
fail() {
  echo
  echo "STOPPED at '$step': $*"
  where_it_is
  reported=true
  exit 1
}

# Any other failure (a dfx command that fails under set -e) is reported the same way.
on_exit() {
  local status=$?
  if [ "$status" -ne 0 ] && ! $reported; then
    echo
    echo "STOPPED at '$step': a command failed (exit status $status)."
    where_it_is
  fi
}
trap on_exit EXIT

sha256() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
installed_hash() { dfx canister info "$CANISTER" --network "$NETWORK" | sed -n 's/^Module hash: 0x//p'; }
# The canister log with the index of each line ("[N. timestamp]: ..."); prints the lines numbered above $1.
logs_after() {
  awk -v after="$1" '
    /^\[[0-9]+\. / { index_ = substr($0, 2, index($0, ".") - 2) + 0 }
    index_ > after { print }
  ' <<<"$2"
}
highest_index() { sed -n 's/^\[\([0-9][0-9]*\)\. .*/\1/p' <<<"$1" | sort -n | tail -1; }

say "Building $CANISTER"
dfx build "$CANISTER" --network "$NETWORK"
[ -f "$BUILT_WASM" ] || fail "dfx built no $BUILT_WASM"
built_hash="$(sha256 "$BUILT_WASM")"
echo "Built: $BUILT_WASM, sha256 $built_hash"

step="reading the running version"
say "Running version"
call health
running_hash="$(installed_hash)"
echo "Running module: sha256 ${running_hash:-none}"
# dfx skips installing a module that is already installed (and then no upgrade hook runs).
if [ "$running_hash" = "$built_hash" ]; then
  echo
  echo "The canister already runs this build: there is nothing to upgrade. Nothing was changed."
  reported=true
  exit 0
fi

if [ "$NETWORK" = "ic" ]; then
  echo
  echo "This upgrades the canister on MAINNET with the identity '$(dfx identity whoami)'."
  read -r -p "Type the canister name ($CANISTER) to continue: " answer
  [ "$answer" = "$CANISTER" ] || { echo "Aborted; nothing was changed."; reported=true; exit 1; }
fi

step="stopping"
say "Stopping (a version before 0.7.0 can take minutes while a model call is outstanding)"
phase=stopping
dfx canister stop "$CANISTER" --network "$NETWORK"
phase=stopped

step="taking a snapshot"
say "Taking a snapshot"
snapshot_output="$(dfx canister snapshot create "$CANISTER" --network "$NETWORK" 2>&1)" || {
  echo "$snapshot_output"
  fail "no snapshot was taken (a canister holds a limited number: see 'dfx canister snapshot list $CANISTER --network $NETWORK')."
}
echo "$snapshot_output"
snapshot_id="$(sed -n 's/.*[Ss]napshot ID: *\([0-9a-fA-F][0-9a-fA-F]*\).*/\1/p' <<<"$snapshot_output" | tail -1)"
# Without the id there is no way back to print, so nothing is installed.
[ -n "$snapshot_id" ] || fail "could not read the snapshot id in dfx's output above; nothing was installed."
phase=snapshotted

step="reading the log before the install"
say "Reading the canister log before the install"
log_before="$(dfx canister logs "$CANISTER" --network "$NETWORK")"
index_before="$(highest_index "$log_before")"
index_before="${index_before:--1}"
echo "Newest log line before the install: ${index_before/#-1/none}"

step="installing"
say "Installing the new version (upgrade mode; the canister stays stopped)"
phase=installing
dfx canister install "$CANISTER" --mode upgrade --network "$NETWORK" ||
  fail "the upgrade did not go through. If a hook trapped, the old version and its state are still in place."
phase=installed

step="checking the installed code"
say "Checking the installed code"
installed="$(installed_hash)"
echo "Installed module: sha256 ${installed:-unknown}"
echo "Built Wasm:       sha256 $built_hash"
[ "$installed" = "$built_hash" ] || fail "the installed module is not the Wasm that was built."

step="checking the upgrade hooks"
say "Checking the log lines of this upgrade's hooks (newer than line $index_before)"
log_after="$(dfx canister logs "$CANISTER" --network "$NETWORK")"
this_upgrade="$(logs_after "$index_before" "$log_after")"
printf '%s\n' "$this_upgrade" | grep -E 'pre_upgrade: |Restoring state|Brought the state|State restored' || true
saved="$(grep -oE 'pre_upgrade: [0-9]+ bytes' <<<"$this_upgrade" | grep -oE '[0-9]+' || true)"
restored="$(grep -oE 'State restored from stable memory successfully \([0-9]+ bytes' <<<"$this_upgrade" | grep -oE '[0-9]+' || true)"
[ "$(grep -c . <<<"$saved")" = 1 ] && [ "$(grep -c . <<<"$restored")" = 1 ] ||
  fail "expected one pre_upgrade line and one 'State restored' line from this upgrade; found saved '${saved//$'\n'/,}', restored '${restored//$'\n'/,}'."
[ "$saved" = "$restored" ] ||
  fail "pre_upgrade saved $saved bytes, post_upgrade read back $restored."
echo "pre_upgrade saved $saved bytes; post_upgrade read back and decoded $restored bytes: the lengths match."
echo "(A length check on top of a decode that did not trap — not proof that the contents are identical.)"

step="starting"
say "Starting"
phase=starting
dfx canister start "$CANISTER" --network "$NETWORK"
phase=started

step="checking the running canister"
call health
call get_metrics

reported=true
cat <<EOF

Done. Still to do by hand:
  1. Send one real message through the frontend: it is the test that the model answers.
     If it does not, go back to the code and state of the snapshot:
       $STOP && $(load) && $START
  2. Deploy the frontend:  make upgrade-frontend NETWORK=$NETWORK
  3. Once you are sure, the snapshot can be deleted:
       dfx canister snapshot delete $CANISTER $snapshot_id --network $NETWORK
EOF
