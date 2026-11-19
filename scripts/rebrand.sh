#!/bin/sh
# Renames upstream's herdr-core, Herdr Core, hc and HC_* to ours across the tree, and points CI's
# upgrade test at our release tags: once for the rename, and again after every upstream merge
# (REMUDA.md). A second run changes nothing.
set -eu
cd "$(git rev-parse --show-toplevel)"

# File by file, so a file an upstream merge left in an old folder joins the moved one; a file with a
# copy at both places stops the run before anything moves.
tab=$(printf '\t')
pairs() { git ls-files -- "$1" | while IFS= read -r from; do printf '%s\t%s\n' "$from" "$2${from#"$1"}"; done; }
moves() {
  pairs crates/herdr-core crates/remuda-core
  pairs skills/herdr-core skills/remuda-core
  pairs schema/hc.openrpc.json schema/rc.openrpc.json
}
both=$(moves | while IFS=$tab read -r from to; do if [ -e "$to" ]; then printf '  %s  %s\n' "$from" "$to"; fi; done)
if [ -n "$both" ]; then
  printf 'rebrand.sh: these files exist at both places; keep one of each (git mv -f takes upstream'\''s, git rm keeps ours), then run again:\n%s\n' "$both" >&2
  exit 1
fi
moves | while IFS=$tab read -r from to; do
  mkdir -p "$(dirname "$to")"
  git mv "$from" "$to"
done
for old in crates/herdr-core skills/herdr-core; do
  if [ -d "$old" ]; then find "$old" -type d -empty -delete; fi
done

# Text files only (-I), never vendored code, upstream's history and licenses, or our own files
# that name upstream on purpose.
git grep -l -z -I -e '' -- . ':!vendor' ':!CHANGELOG.md' ':!LICENSE' ':!distribution/notices' ':!distribution/windows' \
  ':!REMUDA.md' ':!CLAUDE.md' ':!scripts/rebrand.sh' |
  xargs -0 perl -pi -e '
    s{herdrdev/herdr-core}{dev1antdev/remuda-core}g;
    s{dev\.herdr\.core}{dev.mardeus.remuda-core}g;
    s{Herdr Core}{remuda core}g;
    s{herdr-core}{remuda-core}g;
    s{herdr_core}{remuda_core}g;
    s{in Herdr terminals}{in remuda core terminals}g;
    s{HERDR_RELEASES_URL}{RC_RELEASES_URL}g;
    s{(?:(?<![A-Za-z0-9])|(?<=\\[nrt0]))HC_}{RC_}g;
    s{(?:(?<![A-Za-z0-9])|(?<=\\[nrt]))hc(?![A-Za-z0-9])}{rc}g;
    s{--match \x27v\*\x27}{--match \x27remuda-v*\x27}g;
  '

# The longer names move some lines past rustfmt's width, and the renamed package its place in the lock.
if command -v cargo > /dev/null; then
  cargo fmt --all
  cargo update --workspace --offline --quiet
else
  echo "rebrand.sh: no cargo here: run cargo fmt --all and cargo update --workspace --offline" >&2
fi
