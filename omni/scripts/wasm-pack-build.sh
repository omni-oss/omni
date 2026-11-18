#!/bin/sh
# Set WASM_PACK_NO_OPT=true (e.g. in the project's .env.local) to skip wasm-opt.
set -eu

set -- build --target nodejs --out-name index --out-dir ./dist
case "${WASM_PACK_NO_OPT:-}" in
1 | true | TRUE | True | yes) set -- "$@" --no-opt ;;
esac

exec wasm-pack "$@"
