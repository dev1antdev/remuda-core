#!/bin/sh
# Render CLI help and check human examples using the binary built by Cargo.
set -eu
case "${1:---check}" in
  --write) export RC_UPDATE_DOCS=1 ;;
  --check) unset RC_UPDATE_DOCS ;;
  *) echo 'usage: sh scripts/docs.sh [--write|--check]' >&2; exit 2 ;;
esac
cargo test --locked -p remuda-core --test cli_docs
