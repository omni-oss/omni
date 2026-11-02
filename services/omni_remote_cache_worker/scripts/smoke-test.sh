#!/bin/sh
# Smoke test for the worker running under `wrangler dev --local` (miniflare R2).
#
#   sh scripts/smoke-test.sh [config.json]
#
# Defaults to the service's orcs.config.json (tenant1/org1/ws1/env1, key1).
# Requires: curl, cmp, and either `wrangler` or `bunx`.
set -eu

cd "$(dirname "$0")/.."

config="${1:-../omni_remote_cache_service/orcs.config.json}"
port="${PORT:-8799}"
base="http://localhost:$port"
api="$base/api/v1/artifacts"
tmp="$(mktemp -d)"
log="$tmp/wrangler.log"

wrangler="wrangler"
command -v wrangler >/dev/null 2>&1 || wrangler="bunx wrangler@latest"

cleanup() {
    [ -n "${pid:-}" ] && kill "$pid" 2>/dev/null || true
    rm -rf "$tmp"
}
trap cleanup EXIT

compact="$(tr -d '\n' <"$config")"
$wrangler dev --local --port "$port" \
    --var "ORCS_CONFIG:$compact" --var ENABLE_OPENAPI:true >"$log" 2>&1 &
pid=$!

i=0
until grep -q "Ready on" "$log" 2>/dev/null; do
    i=$((i + 1))
    if [ "$i" -gt 120 ]; then
        echo "wrangler did not become ready" >&2
        cat "$log" >&2
        exit 1
    fi
    sleep 2
done

key="x-api-key: key1"
tenant="x-omni-tenant: tenant1"
q="org=org1&ws=ws1&env=env1"
fail=0

expect() {
    name="$1"
    want="$2"
    got="$3"
    if [ "$want" = "$got" ]; then
        echo "ok   $name ($got)"
    else
        echo "FAIL $name: expected $want, got $got" >&2
        fail=1
    fi
}

status() { curl -s -o /dev/null -w "%{http_code}" "$@"; }

expect "openapi" 200 "$(status "$base/openapi/v1/json")"
expect "missing api key" 400 "$(status -H "$tenant" "$api/abc?$q")"
expect "bad api key" 403 "$(status -H "x-api-key: nope" -H "$tenant" "$api/abc?$q")"
expect "put" 204 "$(status -X PUT -H "$key" -H "$tenant" --data-binary "hello r2" "$api/abc?$q")"
expect "head" 204 "$(status -I -H "$key" -H "$tenant" "$api/abc?$q")"
expect "get body" "hello r2" "$(curl -s -H "$key" -H "$tenant" "$api/abc?$q")"
expect "list" 200 "$(status -H "$key" -H "$tenant" "$api?$q")"

head -c 20971520 /dev/urandom >"$tmp/big.bin"
expect "put 20MiB (multipart)" 204 "$(status -X PUT -H "$key" -H "$tenant" --data-binary @"$tmp/big.bin" "$api/big?$q")"
if curl -s -H "$key" -H "$tenant" "$api/big?$q" | cmp -s - "$tmp/big.bin"; then
    echo "ok   20MiB round trip identical"
else
    echo "FAIL 20MiB round trip differs" >&2
    fail=1
fi

expect "delete" 204 "$(status -X DELETE -H "$key" -H "$tenant" "$api/abc?$q")"
expect "get after delete" 404 "$(status -H "$key" -H "$tenant" "$api/abc?$q")"

exit $fail
