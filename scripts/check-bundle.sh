#!/usr/bin/env bash
# Fails unless the production bundle in <dist> exists and contains no development-only code: the mock backend and
# the development identity (src/dev/). Fails closed: no bundle files, or a search that cannot read them, fails too.
#
#   scripts/check-bundle.sh <dist directory>
#
# Run by `make build`, `make build-ic` and CI. (The bundle `make e2e-local` builds with VITE_ALLOW_DEV_IDENTITY=1
# contains the development identity on purpose; it is only ever deployed to the local replica.)
set -uo pipefail

dist="${1:?usage: scripts/check-bundle.sh <dist directory>}"
shopt -s nullglob
files=("$dist"/assets/*.js)
if [ "${#files[@]}" -eq 0 ]; then
  echo "No bundle in $dist/assets: nothing to check, which is a failure (build it first)."
  exit 1
fi

grep -l -E 'createMockBackend|dev-identity' "${files[@]}"
status=$?
case "$status" in
  1)
    echo "No development-only code in the bundle (${#files[@]} script files checked)."
    exit 0
    ;;
  0)
    echo "Development-only code is in the production bundle (the files above)."
    exit 1
    ;;
  *)
    echo "Could not search the bundle (grep exit status $status)."
    exit 1
    ;;
esac
