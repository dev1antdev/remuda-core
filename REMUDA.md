# remuda-core

This repository is [remuda](https://github.com/dev1antdev/remuda)'s own clone of
[herdr-core](https://github.com/herdrdev/herdr-core) (Apache-2.0), taken at its tag v0.1.21: the
terminal runtime remuda runs on the machines it manages. It is not part of Herdr, and Herdr does not
maintain it. Its owner, dev1antdev, does: where AGENTS.md says to ask the maintainer, ask the owner.

AGENTS.md and CONTRIBUTING.md hold here too. Where this file says otherwise, this file wins.

## What is different here

- **Pull requests and issues** go to this repository, never upstream: pass
  `--repo dev1antdev/remuda-core` to every `gh` command, since this clone also has an `upstream`
  remote and gh may pick it. The owner reviews and merges pull requests, squashed (an upstream merge
  is the one exception, below); no one else's approval is needed, and the owner agrees to behaviour
  changes.
- **Commits** keep upstream's lowercase conventional subject, may carry a body and may end with a
  `Co-Authored-By:` trailer for the agent that wrote them: AGENTS.md's "single line, no AI co-author
  lines" does not hold here.
- **Releases** are ours, on this repository's GitHub Releases, which `rc update`, the installers and
  `rc machine add` read. The owner runs `scripts/release X.Y.Z` on master: it sets the version, marks
  the API as released, pushes, waits for CI and tags `remuda-vX.Y.Z`, and
  `.github/workflows/remuda-release.yml` builds the five targets and publishes them. The version line
  is our own: it begins at 0.2.0, a patch for each of our releases, a minor for each upstream merge.
  macOS builds are signed ad-hoc (no Apple Developer ID): macOS asks again for its privacy access
  after each update. Windows builds are unsigned. Upstream's `.github/workflows/release.yml` signs
  with Herdr's secrets and publishes to Herdr's servers: it stays disabled, never enable it; the
  justfile's `release-prepare` and `release-publish` are its recipes (a `v*` tag, a CHANGELOG entry):
  never run them. CHANGELOG.md is upstream's history; our release notes are GitHub's, from the pull
  requests. If `scripts/release` fails, run it again, after a fix on master too: once Cargo.toml says
  that version, it waits for HEAD's CI and tags HEAD, and it stops if the tag is on origin already;
  a flaky job is rerun first (`gh run rerun <run> --failed`, the run it names). If the publish fails, look before acting:
  `gh release view remuda-vX.Y.Z --repo dev1antdev/remuda-core --json isDraft,assets`; a draft is
  deleted (`gh release delete … --yes`, the tag stays) and the tag's whole run re-run, a published
  release is left as it is. Publishes wait for each other, and an older version published after a
  newer one is not made latest.
- **Names** are ours, never Herdr's: the binary `remuda-core` with the command `rc`, `RC_*`
  variables, our own socket and state paths, the bundle id `dev.mardeus.remuda-core`.
  `scripts/rebrand.sh` puts them in.
- **The API** differs from herdr-core's in names: the error kind `rc_missing` (theirs
  `hc_missing`), the `<rc …>` notes an agent is typed, and the `RC_*` variables; and in what we
  add, listed below (`terminal.keys`'s `now`, `terminal.attach`'s `keep_size`, `terminal.read`'s
  `format`, `events.subscribe`'s `output`, `terminal.send`'s `foreground`, `terminal.run`'s
  `literal`, every status word in `terminal.send`'s `if`, the record's `literal`, the foreground's
  `started_ms` and `started_since_boot`).

## Keeping upstream's fixes applying

- New behaviour goes into new methods in new files. Do not rewrite holder, restart or platform code,
  or the body of an existing method; when one has to change, change the fewest lines.
- Every upstream file we change is listed below with what changed: our record of the changes
  Apache-2.0 §4(b) asks us to state.

### Upstream files we changed

- Every file upstream's names reach, renamed and moved by `scripts/rebrand.sh`, which also points
  CI's upgrade test at our `remuda-v*` tags. A file of ours that names upstream on purpose goes on
  the script's list of exclusions.
- `AGENTS.md`, `README.md`: one line each pointing here; README's Install names our installers.
- `scripts/install-local.sh`: makes the `rc` link only when no other `rc` is on PATH.
- Our releases' addresses and their check: `crates/remuda-core/src/releases.rs` (the GitHub layout,
  `latest/download` and `download/remuda-v<version>`), `distribution/install.sh` and `install.ps1`,
  `scripts/manifest`, the tests' fake releases, and the ad-hoc signature accepted in
  `platform/mod.rs`, `platform/macos_bundle.rs` and `install.sh` (which checks the identifier before
  anything in the bundle runs, for any release address); `docs/security.md` says so. `macos_bundle.rs` still tries Herdr's
  team first, upstream's lines kept as they are: no build of ours carries it.
- `distribution/macos-signing.md`: a line saying it describes upstream's release, not ours.
- `terminal.keys` takes `now` (`rc keys --now`): keys a person pressed in remuda go without the
  quiet pause after their typing, and past a `keys` call already waiting for that pause. They take no
  turn in the terminal's one input queue, only the write that keeps them out of an active send, keys
  sequence or history read (`type_in`'s `ahead` and the `writes` lock in `runtime.rs`, which
  `stitch.rs` takes too); `interrupt` goes the same way (`api/types.rs`, `input.rs`, `api.rs`, a line
  in `src/SKILL.md`, the generated docs and schema; the tests are ours, `tests/remuda.rs`). A core
  older than this on an added machine ignores `now` and waits as before.
- `terminal.attach` takes `keep_size`: a viewer that types but never sets the terminal's size,
  and leaves it as it is when no other viewer sets it (`api/types.rs`, `api.rs` with its
  `keep_size` feature in hello, `viewers.rs`, `remote/viewers.rs` calling our
  `remote/remuda.rs`, which refuses it towards a core without that feature and holds its test;
  a reattach that any core on the path refuses as `needs_update` now ends after the queued output
  with that error as its CLOSE reason,
  `remote.rs` declaring that module, a paragraph in `schema/README.md`, the generated schema,
  `keep_size: false` in the `AttachRequest` literals of `src/runtime/tests.rs` and
  `src/viewers/tests.rs`; the other test is ours, in `tests/remuda.rs`).
- `terminal.read` takes `format: "vt"` (`rc read --format vt`): a live terminal's rows with their
  colors as SGR, each starting with a style reset, history above the screen on the main screen
  only (a full-screen program is not scrolled for it), as many of the newest rows as the caller asks
  for while their JSON fits half a frame (8 MiB) and at most 5000 of them, which is as far as a
  client's find asks: a page's find reads deeper than its own rows, and `complete` says only that
  rows were really dropped — read off the terminal's `scrolled` counter, rows its output pushed into
  the history (what the history grew by, and one for a write that left it no larger: a wrapped line
  with no line break counts too, where `printed` counting line breaks saw nothing); a finished terminal answers
  its plain text. **A read is worked out on a blocking thread, its lock held only for the screen's
  snapshot** (`runtime.rs`'s `screen_bytes`, one call into libghostty; `restored` rebuilds it
  outside): the core's runtime runs its tasks on one thread, so a deep history formatted there
  would hold every other terminal's input and output for as long as it took (in `api.rs`'s
  `terminal_read`, plain text through `plain_read` the same; `TestADeepReadLeavesTheOtherTerminalsAnswering`).
  A core older than this on an added machine still stops at 1000 rows and calls the
  read incomplete whenever more are asked. The rows are encoded cell by cell from
  libghostty-vt's cell styles (its formatter drops a blank cell's background), a row walked along
  its page from its first cell; the underline color is left out. Ours: `ghostty-vt/src/styled.rs` (`styled_screen`, `styled_history`, with their
  tests) and `remuda-core/src/styled.rs` (`ReadFormat`, the read and its budget, `rc read`'s notes
  on stderr for vt); in upstream's files `mod styled;` in `ghostty-vt/src/lib.rs`,
  `pub mod styled;` in `lib.rs`, the param in `api/types.rs`, its branch in `api.rs`, the default
  in `api/run.rs`, the print call in `cli.rs`, the generated docs and schema; the other tests are
  ours, in `tests/remuda.rs`. A core
  older than this on an added machine ignores `format` and answers plain text (no row starts with
  `ESC[0m`).
- A **reply is one frame** (`rpc.rs`'s `reply_bytes`, `MAX_REQUEST_ID`): one larger than `wire::MAX_PAYLOAD`
  is answered instead by the `too_large` error, so a big result never ends a connection, and a request whose
  id is longer than 1024 bytes of JSON is refused before dispatch (`invalid_request`, with a null id) — an id
  comes back whole in its reply, so a request of megabytes used to make its own reply past the frame and the
  receiver dropped the line (`an_id_too_long_for_a_reply_is_an_error_not_a_broken_connection`). Ours: in
  upstream's `rpc.rs` the bound and the id length, in `server.rs` the check before dispatch and the two
  writes — one framed, one plain — that go through it.
- `events.subscribe` takes `output` watches (`output_watches` in hello's features): each row that
  comes into a watch's last rows of a terminal here matching its substring or regex (the `regex`
  crate, Unicode `\d` and `\w`) is an `output` event carrying the row's `pos`, its place in the
  terminal's history and screen (the same text at another place is another row), the rows already
  there `fresh`; at most 256
  watches, 1 to 200 rows, 4 KB a pattern, 256 bytes an id; rows seen wait in their watch's queue
  and are told in turn across watches, 256 events (1 MB) a look; what still waits past 1000 rows
  or 64 KB loses its oldest (never its newest), counted as `lost` on the next row told; a row over
  64 KB is told as the 64 KB around what matched, with `cut`; a terminal without a live screen (finished, or kept between shells) is read from
  its snapshot; `invalid_regex` names the
  watch; a terminal on another machine is refused. Ours: `remuda-core/src/watches.rs` (the watches, a task per subscription
  looking every 250 ms at the terminals that printed) and `recent_rows` in
  `ghostty-vt/src/styled.rs` (the last rows' text with soft wraps joined, as `plain_text` reads it);
  in upstream's files `#[derive(Clone)]` on `Sink` in `events.rs`, the param in `api/types.rs`, the compile and
  start in `api.rs`'s `events_subscribe` with its errors, description and the feature,
  `pub mod watches;` in `lib.rs`, `regex` in `remuda-core/Cargo.toml` and `Cargo.lock`, the
  regenerated `THIRD-PARTY-NOTICES` and schema, the `output` row and a paragraph in
  `schema/README.md`; the tests are ours, in `watches.rs`,
  `styled.rs` and `tests/remuda.rs`.
- `events.subscribe` refuses a call whose framed connection is already gone: detaching the whole
  connection marks it ended under the state lock, so a call still in flight sees it and registers
  nothing — no subscription, no heartbeat, no watches (`wire.rs`'s `Conn::end`, `viewers.rs`'s
  `detach`, `api.rs`'s `events_subscribe`; the test is in `api.rs`).
- `terminal.send` takes `foreground` (API only, no CLI flag; `send_foreground` in hello's features):
  it types only while that process group leads the terminal — refused as `status_mismatch` when the
  last look already saw another, then looked at afresh before the paste (refused `status_mismatch`,
  nothing typed, or `cancelled` if the caller left) and again before Enter (refused `not_submitted`,
  its Enter held back) — so an agent that left while the send waited for a person's draft never has
  its message run by the shell behind it; a relay refuses the field towards a core without the
  feature (`needs_update`). Ours: `remuda-core/src/front.rs` (`send_for`, `guard`, `last_seen`,
  `submit`, `in_front`, `ask`, `Asked`, `answering`, `link_ended`, `linked`, with their tests) and
  `refused` with the shared `older` in `remote/remuda.rs` (with its test); in
  upstream's files the `Front` and `Submit` steps and their arms in `input.rs`'s `type_in` (a
  refusal still ends the step's turn and hands the person's held keys over; the Enter is checked
  under the lock it is written under, blocked first unless `force`, then the `if` word),
  `send_message` made `pub(crate)` with a
  `front` param and its check in the prepare closure (`send` and `send_first_task` pass `None`),
  `Core::foreground` asking through `front::ask` (the request sent and its lookup noted with the
  holder asked, under one lock; none without a link) and the import it no longer uses; in
  `runtime.rs` the waiting lookups typed `front::Asked`, a reply answering only the oldest lookup
  asked of its holder (`front::answering`, where it answered every lookup waiting) and a holder's
  ended link dropping its lookups (`front::link_ended`), and in `holder.rs`'s `FOREGROUND` arm the
  reply skipped for a command read after a newer core's hello, as `GOODBYE` does, so a look is never
  an earlier sample; `own_replies` in `holder_proto.rs`'s `Welcome` (true in `holder.rs`, `false` in
  the four literals of `runtime/tests.rs`), noted per holder in `State.own_replies` by
  `front::linked` where `runtime.rs` keeps a new link, and a guarded send refused as `needs_update`
  on a terminal whose holder lacks it (a holder kept across `rc update`), each of its looks asking
  only such a holder (`front::ask` with `trusted`); the param in `api/types.rs`, `send_for` with its
  description, the `not_submitted` and `needs_update` errors and the feature in `api.rs`, the two
  refusal calls in `remote.rs`, `mod front;` in `lib.rs`, a paragraph in `schema/README.md`, the
  generated schema; the end-to-end test is ours, in `tests/remuda.rs`.
- `terminal.run` takes `literal` (API only, no CLI flag: the CLI says no hello, so it could not tell
  whether the core takes it; `run_literal` in hello's features): argv starts as it is, no template
  looked up by its first word and no first message sent, so a program named like a template runs
  itself and a missing one is `spawn_failed`; it needs argv (`invalid_params` with `line` or with
  neither); a relay to an added machine checks it as that core would (`invalid_params`) and refuses it
  towards a core without the feature (`needs_update`), which would start the template instead (a core
  that added us is never asked to run, upstream's rule). remuda lays a scene's pane command out with
  it. Ours: `refused`'s `terminal.run` arm and `literal_invalid` in `remote/remuda.rs` (with their
  tests and `test_core`, which the keep-size test now shares); in upstream's files the param in
  `api/types.rs`, the check and the template lookup skipped in `api/run.rs`, the description, the
  `needs_update` error and the feature in `api.rs`, a paragraph in `schema/README.md` under a section
  of its own, "Call options newer cores add", before Events (the `foreground` paragraph moved there
  too), the generated schema; the end-to-end tests are ours, in `tests/remuda.rs`.
- `terminal.send`'s `if` (`rc send --if`) takes every status word a program reports, `done`,
  `blocked` and `error` besides `idle` and `working` (`send_if_any_status` in hello's features): a
  send for a program at rest whose status moved while it waited for a person's draft, or by the time
  its paste is written (checked under the lock it is written under, after any `foreground` look), is
  refused as `status_mismatch`, nothing typed, rather than land in the turn that person started
  (`blocked` keeps its own refusal first, unless `force`). An older core refuses the newer words as `invalid_params`, nothing typed, and a relay
  refuses them the same way, with the update in its hint, towards the core that would type when it
  lacks the feature (the kind kept, since remuda reads `invalid_params` as an older core). Ours:
  `refused`'s `if` arm in `remote/remuda.rs`, and `conditional`, `paste_if` and `still` in
  `front.rs` (with their tests); in upstream's files the three variants and a doc line on
  `IdleOrWorking` in `api/types.rs` (its name kept, so upstream's lines still apply), the `Paste`
  step and its arm in `input.rs`'s `type_in` with the line in `send_message` that makes a send's
  paste one, the feature in `api.rs`, a paragraph in `schema/README.md`, the generated docs and
  schema; the end-to-end tests are ours, in `tests/remuda.rs`.
- A terminal's record carries `literal` (`literal_record` in hello's features): true for a terminal
  a `literal` run started, false for any other and once keep's shell has started in place of that
  program (true still while it waits for that shell, through a failed replacement, or has finished:
  `pid` and `reason` say whether anything runs), so remuda tells a scene pane's command (a shell's
  `sh -c '…'` too) from the shell a reboot gives that pane, whatever their argv; a record a core
  without the feature wrote has none, also once an update adopts it (unknown, never false), so
  remuda runs `literal` only on a core with both features and reads a missing one as unknown. In upstream's files the field in `terminals.rs`'s `Terminal` and
  `NewTerminal` with its line in `Registry::create`, `literal: p.literal` in `api/run.rs`, its
  reset in `runtime.rs`'s `spawn_result` for a spawn ending a loss (a `reason` other than
  `spawn_pending`, the adoption after a core crash included), the feature in `api.rs`, a paragraph
  in `schema/README.md`'s Events after keep's, the generated schema; the tests are ours, in
  `tests/remuda.rs`.
- A terminal's `foreground` carries `started_ms` (`foreground_started` in hello's features): when
  that process was created, unix ms, a floor, as the OS records it (Linux `/proc/<pid>/stat`'s
  start after the boot time, `/proc/stat`'s whole second with the fraction the clocks tell (the
  real clock read between two looks at the time since boot a millisecond apart at most,
  `bracketed`), kept until either moves past a reading's jitter, so its second is ps's
  `lstart` or the next, and a step moves it as it moves ps's, the core telling each foreground
  again once; macOS `proc_pidinfo`'s BSD info; Windows `GetProcessTimes`), read between two of the
  holder's answers naming that process group, its start before and after the rest, and on Unix
  only for a process of the terminal's session (`getsid`, its program being its session's
  leader), a look where either moved or that found another terminal's process telling none; so
  remuda tells the process the core saw from a later one given its pid before it reads that
  process's session. Ours: `remuda-core/src/started.rs` (`of`, `boot_ms`, `bracketed`, `settle`,
  `in_session`, `Program` and `Core::foreground_now`, with their tests); in upstream's files the field and
  its literals in `foreground.rs`, which hold `..Default::default()` since `started_since_boot`
  (below), `watch_foreground` calling `foreground_now` where it asked the holder and read `about`, and
  keeping its look only while the terminal's program is the one it was read under (`Program`: its
  pid and which of the core's spawns started it, counted in memory and never back, so a `lost` or
  a restart meanwhile, the same pid's too whatever the clock says, leaves it for the next look),
  `spawn_result` in `runtime.rs` noting each spawn (`crate::started::spawned`) and `Core::remove`
  forgetting it (`crate::started::forget`),
  `pub mod started;` in `lib.rs`, the feature in `api.rs`, the `foreground` row and a section of
  its own, "Fields newer cores add", in `schema/README.md`, the generated schema; the end-to-end
  test is ours, in `tests/remuda.rs`.
- A terminal's `foreground` carries `started_since_boot` on Linux (`foreground_since_boot` in
  hello's features): that process's start as its boot counts it, `{boot_id, time_ns, ticks}`
  (`/proc/sys/kernel/random/boot_id` through `platform::boot_id`, which the restart reads too, the
  core's own time namespace from `/proc/self/ns/time`, whose boot-time offset Linux adds to the
  ticks a reader sees — none told when that link cannot be read — and `/proc/<pid>/stat`'s
  starttime), which no step
  of the wall clock moves, so a process born as much later as the clock went back is not taken for
  the one before, as `started_ms` alone would take it; read with `started_ms`, the look kept only
  while both are the same before and after it; absent on macOS and Windows, whose `started_ms` is
  the creation the OS stored, which no later step moves, though a step back between two births
  still gives both one `started_ms` there (a known limit, said in `schema/README.md`). Ours:
  `SinceBoot`, `since_boot` and `start_ticks` (which `of` now reads through) in
  `remuda-core/src/started.rs` with their test, `foreground_now` there reading it, and
  `impl Default for Foreground` there; in upstream's files the field, and `..Default::default()`
  in place of `started_ms: None` in the three `about`s of `foreground.rs` and its test's literal, so
  a later field of ours touches none of them (rustfmt lays the test's `root` literal out a field a
  line, 123 columns being past the width, so that whole literal is ours), the feature in `api.rs`,
  the `foreground` row and a paragraph in `schema/README.md`'s "Fields newer cores add", the
  generated schema; the end-to-end test is ours, in `tests/remuda.rs`.
- `terminal.attach`'s first paint is bounded by the stream's queue: `first_paint` asks our
  `paint::within` (`remuda-core/src/paint.rs`) for the newest history rows that fit
  `wire::MAX_PENDING` (4 MiB of pending DATA), the oldest cut, so the screen and the state always go
  with it; and `history` itself is taken at most as far as the rows the painter replays
  (`kept_history_rows` and `primary_screen`, ours in `ghostty-vt/src/styled.rs`: the primary screen,
  restored once, which a full-screen program hides, so a client's number can neither wrap the painter's
  `u32` nor make the cut search rows that are not there) — a paint past the queue
  was dropped whole and the attach never checked, so a densely coloured history left a viewer blank.
  A screen that alone passes the queue still cannot be sent: there is no history left to cut. Ours:
  `paint.rs` with `viewers.rs`'s `first_paint`, and `paint_with` in `ghostty-vt/src/lib.rs` (the
  prepared primary above goes in, so a candidate restores nothing); in upstream's files `paint`
  delegating to it and `mod paint;` in `lib.rs`; the `history` param, the generated schema and
  `docs/commands.md`, one
  paragraph in `schema/README.md`; the test is ours, in `tests/remuda.rs`. A core older than this on
  an added machine still drops the paint whole, so a blank viewer there means an update.
- The paint keeps the rows a program soft-wrapped: `paint_with` joins them (`unwrap`, the row's width written
  out) so a client's own find crosses a word broken at the wrap — in the history, on the screen and in
  `replay` — and leaves them as rows where the grid forbids it: a row whose wrap flag and the row below it
  disagree, or that holds no text (the formatter writes a blank row no cells, so its one break would stand
  for two rows), a wrap a wide character forces (the guard that keeps a client without Unicode 11 widths from
  re-wrapping the row elsewhere), a client whose own width is not the grid and that was not told it before
  the paint, and a history block ending on such a spacer (the formatter moves that end onto the glyph below,
  which the screen would then paint twice). A client that draws joined rows counts a cell's width as we do,
  a promise `schema/README.md` makes and remuda keeps with xterm's Unicode 11 widths. Autowrap off is the
  program's, but the rows are drawn while it is on, a joined row continuing only by wrapping: it goes back
  after the last cell. Ours: `ghostty-vt/src/wrap.rs`
  (`joins_wraps`, `ends_on_a_spacer_head`, `ALL_ROWS`, with their tests) and `viewers.rs`'s
  `Viewer::knows_the_grid` with `wraps_join`; in upstream's files the `unwrap` line in `format_with`,
  `mod wrap;` and `paint_with` in `ghostty-vt/src/lib.rs`, and a sentence in `schema/README.md`'s attach
  paragraph, where a viewer applies a paint at the grid it set or was told. A core older than this on an
  added machine still paints a soft-wrapped row as two lines.
- `.github/workflows/ci.yml`: Windows tests on `windows-2022` (as our release builds Windows):
  `windows-2025`'s Visual Studio 18 refuses Zig 0.16's clang; go back once upstream's Zig builds there.
- `crates/remuda-core/src/sweep.rs`'s tests: `members_settled` wants a process found on three reads
  25 ms apart, since macOS's python3 is a stub that execs the developer directory's python3 and then
  the framework's Python, each image readable for a few milliseconds with none at all in between; a
  sweep called on a lone sighting of such an image found nothing to end, and the test then waited out
  the sleeper's 30 s. The end the test waits for is bounded at 2 s, so a miss fails at once.
- The hints and help say how to start our core, not Herdr's: `platform/windows.rs`,
  `platform/windows_session.rs`, `platform/macos_session.rs`, `cli/server.rs`'s `--fresh` help and
  its line about this shell's environment, `src/SKILL.md`'s Windows conflict, macOS login and
  `--fresh` sentences, `docs/machines.md`'s add-machine note, and `skills/remuda-core/SKILL.md`,
  which no longer offers Herdr's window or mods; `tests/windows.rs` checks the new Windows hint.
  The line on macOS privacy keeps naming Herdr.app, the app whose grants do not transfer.

## Taking upstream's changes

For each upstream release tag, on a branch of its own, as one chain so nothing merges into the
wrong branch when a step fails. The merge stops before committing, even when it is clean, so the
rename goes into the merge commit:

```sh
git fetch origin && git fetch upstream tag vX.Y.Z --no-tags &&
  git switch -c upstream-vX.Y.Z origin/master &&
  git merge --no-ff --no-commit vX.Y.Z
```

When `upstream-vX.Y.Z` already exists (a sync begun before), do not run the chain again: switch to
that branch, check `git branch --show-current` names it, and go on from where it stopped.

Most conflicts are only the rename. In a file that carries nothing of ours but the rename, take
upstream's side: `git checkout --theirs -- <file> && git add -- <file>`. A modify/delete at an old
path (git did not see the rename) is upstream's copy over ours:
`git checkout --theirs -- <old path> && git add -- <old path> && git mv -f <old path> <new path>`.
A "file location" conflict needs only `git add` of the file where git put it. In a file with
changes of ours beyond the rename, keep upstream's change and apply ours on top of it, never taking
our side of a whole hunk or file (`-X ours`, `checkout --ours`), then `git add` it.

When `git ls-files -u` prints nothing, put our version line back (the one exception to the rule
above: our `version` in `crates/remuda-core/Cargo.toml` and our `schema/released.openrpc.json`, both
from the pre-merge HEAD, whether git merged them or not), rename, stage, look, and commit the merge:

```sh
v=$(git show HEAD:crates/remuda-core/Cargo.toml | sed -n 's/^version = "\(.*\)"/\1/p') &&
  sed -i.bak "s/^version = \".*\"/version = \"$v\"/" crates/remuda-core/Cargo.toml &&
  rm crates/remuda-core/Cargo.toml.bak && git checkout HEAD -- schema/released.openrpc.json &&
  scripts/rebrand.sh && git add -A && git diff --cached --stat &&
  git commit -m "chore: merge upstream vX.Y.Z"
```

The script stops, naming them, when a file still has a copy at both its old and its new place, and
regenerates `Cargo.lock`; the next `scripts/release` takes the next minor. Update the list above if
the files we change moved, and bring any change upstream made to the build job of
`.github/workflows/release.yml` into ours, `remuda-release.yml`. This must print nothing:
`git grep -n releases.herdr.dev -- ':!.github/workflows/release.yml' ':!CHANGELOG.md' ':!REMUDA.md'
':!crates/remuda-core/tests/distribution.rs'`. Run `just check` and `just docs-check`, open a pull
request to `master` and merge it with a merge commit, never squashed or rebased: a squash drops
their history, and the next merge conflicts on the same lines again.

```sh
gh pr merge <n> --repo dev1antdev/remuda-core --merge --subject "chore: merge upstream vX.Y.Z (#<n>)"
```

CI green, then our release. Origin already holds upstream's tags up to v0.1.21 from the clone; push
no more of them (never `git push --tags`): branches and our `remuda-v*` tags only.
