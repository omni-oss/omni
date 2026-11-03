#!/bin/sh
# Usage: worker-build.sh <worker-build version>
# Builds the worker bundle in the current directory, installing worker-build
# first when it is not available.
set -eu

version="$1"

if ! command -v worker-build >/dev/null 2>&1; then
    cargo install worker-build --locked --version "$version"
fi

exec worker-build --release
