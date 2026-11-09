#!/bin/sh
# Usage: worker-build.sh <worker-build version>
# Builds the worker bundle in the current directory, installing worker-build
# first when it is not available.
#
# Set WORKER_BUILD_NO_OPT=true (e.g. in the project's .env.local) to skip the
# wasm-opt step. worker-build 0.8.x renames files inside the directory it is
# iterating, which loops forever on some filesystems (e.g. ntfs3).
set -eu

version="$1"

set -- --release
case "${WORKER_BUILD_NO_OPT:-}" in
1 | true | TRUE | True | yes) set -- "$@" --no-opt ;;
esac

if ! command -v worker-build >/dev/null 2>&1; then
    cargo install worker-build --locked --version "$version"
fi

exec worker-build "$@"
