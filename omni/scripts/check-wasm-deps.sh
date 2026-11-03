#!/bin/sh
# Usage: check-wasm-deps.sh <package>
# Fails when native-only runtimes leak into the wasm32-unknown-unknown
# dependency graph of <package>, e.g. through workspace feature unification
# re-enabling tokio's `net`/`fs`.
set -eu

package="$1"
status=0

tree=$(cargo tree -p "$package" --target wasm32-unknown-unknown -e normal,build --prefix none --format '{p}')

for dep in mio socket2 tokio-util aws-sdk-s3; do
    if printf '%s\n' "$tree" | grep -q "^$dep "; then
        echo "error: '$dep' is in the wasm32-unknown-unknown dependency graph of $package" >&2
        status=1
    fi
done

exit $status
