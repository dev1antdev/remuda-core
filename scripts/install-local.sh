#!/bin/sh
# Builds a release binary into ~/.local/bin (replacing any older one), then
# restarts the core the planned way so running terminals pick up the new build
# (never handing this shell's environment to a core it has to start).
set -eu
cd "$(dirname "$0")/.."
nice -n 10 cargo build --release
bin="${RC_INSTALL_DIR:-$HOME/.local/bin}"
mkdir -p "$bin"
if [ "$(uname -s)" = Darwin ]; then
  app="target/release/remuda core.app"
  rm -rf "$app"
  sh scripts/mac-bundle target/release/remuda-core "$app"
  bash scripts/mac-sign "$app"
  "$app/Contents/MacOS/remuda-core" --rc-install-bundle "$app/Contents/MacOS/remuda-core" "$bin/remuda-core"
else
  install -m 755 target/release/remuda-core "$bin/remuda-core.new"
  mv -f "$bin/remuda-core.new" "$bin/remuda-core"
fi
command -v rc > /dev/null || [ -e "$bin/rc" ] || [ -L "$bin/rc" ] || ln -s remuda-core "$bin/rc"
sh scripts/restart-core.sh "$bin/remuda-core"
