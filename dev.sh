#!/bin/sh
# Builds the working copy and runs it in a window, on top of whatever desktop you are
# in (including an installed edex-rs session). Nothing installed is touched, so this is
# the safe way to try a change. Arguments go to edex-comp; see `./dev.sh --help`.
set -eu
cd "$(dirname "$0")"
cargo build
exec ./target/debug/edex-comp --backend winit "$@"
