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
- **Names**: what we ship will carry our own name, bundle id, socket and state paths, never Herdr's.
  Until the rename lands, the code keeps upstream's names: do not rename anything in passing.

## Keeping upstream's fixes applying

- New behaviour goes into new methods in new files. Do not rewrite holder, restart or platform code,
  or the body of an existing method; when one has to change, change the fewest lines.
- Every upstream file we change is listed below with what changed: our record of the changes
  Apache-2.0 §4(b) asks us to state.

### Upstream files we changed

- `AGENTS.md`: one line pointing here.

## Taking upstream's changes

For each upstream release tag, on a branch of its own:

```sh
git fetch origin
git fetch upstream tag vX.Y.Z --no-tags
git switch -c upstream-vX.Y.Z origin/master
git merge --no-ff vX.Y.Z -m "chore: merge upstream vX.Y.Z"
```

Resolve each conflict by keeping upstream's change and applying ours on top of it, never by taking
our side of a whole hunk or file (`-X ours`, `checkout --ours`), and update the list above if the
files we change moved. Run `just check` and `just docs-check`, open a pull request to `master` and
merge it with a merge commit, never squashed or rebased: a squash drops their history, and the next
merge conflicts on the same lines again.

```sh
gh pr merge <n> --repo dev1antdev/remuda-core --merge --subject "chore: merge upstream vX.Y.Z (#<n>)"
```

CI green, then our release. Origin already holds upstream's tags up to v0.1.21 from the clone; push
no more of them (never `git push --tags`): branches and our `remuda-v*` tags only.
