# remuda-core

This repository is [remuda](https://github.com/dev1antdev/remuda)'s own clone of
[herdr-core](https://github.com/herdrdev/herdr-core) (Apache-2.0), taken at its tag v0.1.21: the
terminal runtime remuda runs on the machines it manages. It is not part of Herdr, and Herdr does not
maintain it. Its owner, dev1antdev, does: where AGENTS.md says to ask the maintainer, ask the owner.

AGENTS.md holds here too. Where this file says otherwise, this file wins.

## What stays as AGENTS.md says

Everything this file does not replace, among it: core is a runtime, not a multiplexer; status comes
only from OSC 7501; fresh code, nothing copied from Herdr 1; TDD; OS code behind `platform/`; file
sizes; tests in isolated sessions; the schema regenerated from `api/types.rs`; `just check`,
`just lint` and `just docs-check` before a pull request.

## What is different here

- **Pull requests and issues** go to this repository, never upstream. The owner reviews and merges
  them, squashed; no one else's approval is needed, and the owner agrees to behaviour changes.
- **Commits** are lowercase conventional commits with a one-line subject, as upstream's, and may end
  with a `Co-Authored-By:` trailer for the agent that wrote them.
- **Releases** are ours, tagged `remuda-v*` on a version line of our own. Upstream's
  `.github/workflows/release.yml` signs with Herdr's secrets and publishes to Herdr's servers: it
  stays disabled, never enable it. CHANGELOG.md is upstream's history; our releases do not go in it.
- **Names**: what we ship carries our own name, bundle id, socket and state paths, never Herdr's.

## Keeping upstream's fixes applying

- New behaviour goes into new methods in new files. Do not rewrite holder, restart or platform code,
  or the body of an existing method; when one has to change, change the fewest lines.
- Every upstream file we change is listed below with what changed (Apache-2.0 §4(b)).

### Upstream files we changed

- `AGENTS.md`: one line pointing here.

## Taking upstream's changes

For each upstream release tag, on a branch of its own:

```sh
git fetch upstream tag vX.Y.Z --no-tags
git switch -c upstream-vX.Y.Z origin/master
git merge --no-ff vX.Y.Z -m "chore: merge upstream vX.Y.Z"
```

Resolve the conflicts keeping our changes, run `just check`, open a pull request to `master` and
merge it with a merge commit, never squashed: a squash drops their history, and the next merge
conflicts on the same lines again. CI green, then our release. Never push upstream's tags to origin
(`git push --tags`): branches and our `remuda-v*` tags only.
