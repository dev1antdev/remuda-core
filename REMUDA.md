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
- **Releases** are ours, tagged `remuda-v*` on a version line of our own. Upstream's
  `.github/workflows/release.yml` signs with Herdr's secrets and publishes to Herdr's servers: it
  stays disabled, never enable it. CHANGELOG.md is upstream's history; our releases do not go in it.
- **Names** are ours, never Herdr's: the binary `remuda-core` with the command `rc`, `RC_*`
  variables, our own socket and state paths, the bundle id `dev.mardeus.remuda-core`.
  `scripts/rebrand.sh` puts them in. Until our release setup lands, `rc update`, the installers and
  `rc machine add` to a machine of another OS or CPU do not work: they look for our names in Herdr's
  release archives and for our bundle id under Herdr's signature, and refuse before installing
  anything. Leave them to the release setup.
- **The API** differs from herdr-core's in names alone: the error kind `rc_missing` (theirs
  `hc_missing`), the `<rc …>` notes an agent is typed, and the `RC_*` variables.

## Keeping upstream's fixes applying

- New behaviour goes into new methods in new files. Do not rewrite holder, restart or platform code,
  or the body of an existing method; when one has to change, change the fewest lines.
- Every upstream file we change is listed below with what changed: our record of the changes
  Apache-2.0 §4(b) asks us to state.

### Upstream files we changed

- Every file upstream's names reach, renamed and moved by `scripts/rebrand.sh`, which also points
  CI's upgrade test at our `remuda-v*` tags. A file of ours that names upstream on purpose goes on
  the script's list of exclusions.
- `AGENTS.md`, `README.md`: one line each pointing here; README's Install says to build from source
  until our releases exist.
- `scripts/install-local.sh`: makes the `rc` link only when no other `rc` is on PATH.

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
upstream's side: `git checkout --theirs -- <file>`. A modify/delete at an old path (git did not see
the rename) is upstream's copy over ours: `git checkout --theirs -- <old path>`, then
`git mv -f <old path> <new path>`. A "file location" conflict needs only `git add` of the file
where git put it. In a file with changes of ours beyond the rename, keep upstream's change and
apply ours on top of it, never taking our side of a whole hunk or file (`-X ours`,
`checkout --ours`).

Then rename, stage, look, and commit the merge:

```sh
scripts/rebrand.sh && git add -A && git diff --cached --stat &&
  git commit -m "chore: merge upstream vX.Y.Z"
```

The script stops, naming them, when a file still has a copy at both its old and its new place.
Update the list above if the files we change moved. Run `just check` and `just docs-check`, open a
pull request to `master` and merge it with a merge commit, never squashed or rebased: a squash drops
their history, and the next merge conflicts on the same lines again.

```sh
gh pr merge <n> --repo dev1antdev/remuda-core --merge --subject "chore: merge upstream vX.Y.Z (#<n>)"
```

CI green, then our release. Origin already holds upstream's tags up to v0.1.21 from the clone; push
no more of them (never `git push --tags`): branches and our `remuda-v*` tags only.
