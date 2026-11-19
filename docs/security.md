# Security and trust

Core runs commands with your account's access. It is not a sandbox or a permission broker for agents.
Anyone who can connect to its API can run commands, read and change files, and control its terminals with that access.

[Back to the README](../README.md) · [API](../schema/README.md) · [Report a vulnerability](../SECURITY.md)

## What runs

The CLI starts a background core when it needs one.
Holder processes own the terminals so a core restart or update can leave programs running.
Programs run as the account that runs core, with its filesystem and network access.
Templates are shell commands and opening instructions, not isolation rules.

Do not run core as root or as an administrator unless you intend to give its clients that access.
A status of blocked can prevent a normal send, but it is not an access boundary against another client.

## Who can connect

On Unix, core listens on a local Unix socket, not a public TCP port.
It creates its state and socket directories with mode 0700.
It does not repair permissions on directories that already exist.
Keep custom socket paths in directories only your account can enter.
There is no separate API password or per-client permission check.
Other programs running as your account are part of the trust boundary.

On Windows, core uses named pipes. Their access rules allow the current account and SYSTEM.
The integrity label prevents a lower-integrity process from controlling an elevated core.
State files use the filesystem's directory permissions; core does not install a separate Windows state-directory ACL.

Do not expose the socket or forward the raw API to people you would not trust with a shell on that machine.

## Sessions

The default session is named default.
Setting `RC_SESSION` selects a separate core, terminals, and state directory:

```sh
RC_SESSION=scratch rc ls
```

Sessions are useful for tests and separate sets of work.
They are not security boundaries: another process under the same account can select the same session.
Config and templates are shared across sessions in the same config home.
Tests should also use scratch XDG directories so they do not touch your real state or config.

## Remote trust

SSH handles login, host keys, and encryption. Core carries its protocol through that connection.
A custom connection command must provide its own trust and transport guarantees.
Core does not add another encryption layer or authenticate a remote user itself.

Adding a machine gives local clients access to its rc account's commands, files, and terminals.
Trust the destination, the SSH target, and any custom connection command.
Machine installation or update copies executable code there and restarts that session's core.
Forgetting a machine removes the connection here, not its remote programs or state.
See [machines](machines.md).

## State and endpoints

| Item | Default |
| --- | --- |
| Unix state | `~/.local/state/remuda-core/<session>/` |
| Unix config | `~/.config/remuda-core/config.toml` |
| Windows state | `%LOCALAPPDATA%\remuda-core\<session>\` |
| Windows config | `%APPDATA%\remuda-core\config.toml` |
| Templates | A templates directory beside the config file |

`XDG_STATE_HOME` and `XDG_CONFIG_HOME` override the state and config homes on both platforms.
On Unix, sockets live under `$XDG_RUNTIME_DIR/remuda-core/` when set.
Otherwise core uses `/run/user/<uid>/remuda-core/` if that runtime directory exists, then falls back to the session's state directory.
The client socket is named `<session>.sock`; holder sockets sit beside it.
On Windows, pipe names include the account name and session.

`RC_SOCKET` overrides the client endpoint. Otherwise the socket setting in config can override it.
A custom endpoint does not move the state directory.
Keep Unix socket paths short; the OS limits their length.

State includes terminal records, saved screens and history, machine connections, peer records, and API documents.
Core and holder logs also live there. Treat the whole directory as private: terminal output and saved screens may contain secrets.
State is not encrypted by core. Removing state is destructive, not a way to stop running programs safely.

## Updates and signing

```sh
rc update
rc update --version 0.2.1
```

The version form means at least that version, not an exact downgrade.
Updates restart the core while holders keep the terminals running.
An update may also restart an older running core when the installed binary is already current.

Release installers and updates download a manifest and check the archive's SHA-256 against it.
The default release service is this repository's GitHub Releases, over HTTPS.
This trusts GitHub and HTTPS; a checksum is not an independently signed release manifest.
The manifest is not verified with a separate signing key.

Windows builds are not signed. A browser download of one may be blocked by SmartScreen; the installer and `rc update` fetch it with curl, which marks nothing.

`rc update --from /path/to/remuda-core` installs a local build instead of downloading one.
Only use a build you trust. The updater's compatibility checks are not a review of its code.

On macOS, a release that ships `remuda core.app` installs as that bundle when rc is installed under your home folder and not as root.
Releases are signed ad-hoc, with no Apple Developer ID: the installer and `rc update` refuse a bundle unless `codesign` finds its identifier `dev.mardeus.remuda-core`.
An ad-hoc signature changes with every build, so macOS asks again for the privacy access it gave the previous one.
Once rc runs from the bundle, an update will not replace it with a bare executable.
As root or outside your home folder, they install the plain `remuda-core` executable, checked only by its SHA-256.
With another `RC_RELEASES_URL`, or `rc update --from` a bundle, that service or file is trusted as it is, under the same check.
The identifier check catches a damaged or wrong bundle, not a forged one: anyone can sign ad-hoc with that identifier, and only GitHub and HTTPS stand behind the download.
