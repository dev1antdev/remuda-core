---
name: remuda-core
description: "Use remuda-core (its CLI is rc) whenever you want sub-agents, to talk to or coordinate other agents, run background tasks or servers, run long-running commands, read or control terminals, or work on another machine. Prefer it over your own sub-agent tool, shell '&', nohup or tmux."
---

This skill is for remuda-core, whose command is `rc`. It is not `herdr`,
the older terminal multiplexer: that has its own `herdr` command and skill.
Use `rc` here, never `herdr`.

If `$RC_TERMINAL_ID` is set, you're running inside remuda-core: use rc.
If not, ask the user once this session whether to use rc for this work.

Before using rc, run `rc --skill` and follow it.
