#!/bin/sh
# Renames upstream's herdr-core, Herdr Core, hc and HC_* to ours across the tree, and points CI's
# upgrade test at our release tags: once for the rename, and again after every upstream merge
# (REMUDA.md). A second run changes nothing.
set -eu
cd "$(git rev-parse --show-toplevel)"

# File by file, so a file an upstream merge left in an old folder joins the moved one.
move() {
  git ls-files -- "$1" | while IFS= read -r from; do
    to="$2${from#"$1"}"
    mkdir -p "$(dirname "$to")"
    git mv "$from" "$to"
  done
  if [ -d "$1" ]; then find "$1" -type d -empty -delete; fi
}
move crates/herdr-core crates/remuda-core
move skills/herdr-core skills/remuda-core
move schema/hc.openrpc.json schema/rc.openrpc.json

# Text files only (-I), never vendored code, upstream's history and licenses, or our own files
# that name upstream on purpose.
git grep -l -z -I -e '' -- . ':!vendor' ':!CHANGELOG.md' ':!LICENSE' ':!distribution/notices' \
  ':!REMUDA.md' ':!CLAUDE.md' ':!scripts/rebrand.sh' |
  xargs -0 perl -pi -e '
    s{herdrdev/herdr-core}{dev1antdev/remuda-core}g;
    s{dev\.herdr\.core}{dev.mardeus.remuda-core}g;
    s{Herdr Core}{remuda core}g;
    s{herdr-core}{remuda-core}g;
    s{herdr_core}{remuda_core}g;
    s{in Herdr terminals}{in remuda core terminals}g;
    s{HERDR_RELEASES_URL}{RC_RELEASES_URL}g;
    s{\bHC_}{RC_}g;
    s{(?:(?<![A-Za-z0-9])|(?<=\\[nrt]))hc(?![A-Za-z0-9])}{rc}g;
    s{--match \x27v\*\x27}{--match \x27remuda-v*\x27}g;
  '

# The longer names move some lines past rustfmt's width, and the renamed package its place in the lock.
if command -v cargo > /dev/null; then
  cargo fmt --all
  cargo update --workspace --offline --quiet
fi
