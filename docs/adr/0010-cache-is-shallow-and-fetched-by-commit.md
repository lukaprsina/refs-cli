---
status: accepted
---

# The Cache holds one shallow, blobless fetch per Pin, not a clone

The Cache was created with `git clone --bare --filter=blob:none`, which still downloads every commit and tree in history. It is now created with `git init --bare` and filled one Pin at a time with `git fetch --no-tags --depth 1 --filter=blob:none origin <sha>`. A Cache entry holds one small pack per Pin and no history.

## Why

- Nothing in `src/` uses history: Refs are resolved with `ls-remote`, which never touches the Cache, and every Cache command reads one commit's tree or blobs.
- On `rust-lang/cargo` the clone took 5.5 s and 26 MB and a depth-1 fetch 0.6 to 0.8 s and 0.2 MB. On a large repo the first sync drops from about a minute to a few seconds, and an `add` that is cancelled after `outline` (ADR 0011) wastes almost nothing.
- Fetching by commit already worked: it is how a Pin missing from the clone was fetched. It was verified, with the blob-by-id fetch after it, on GitHub, GitLab.com, Codeberg, Gitea.com, Bitbucket Cloud and SourceHut (git 2.55). Protocol v2, the client default since git 2.26, accepts any commit the server has.

## Decision

- No ref-based fallback. A server that refuses a commit want fails with `refs::git::commit_unavailable`, which already matches both refusal messages. Every host tested is on protocol v2, and a Pin is a commit `ls-remote` just advertised. Add a fallback when a real host needs one.
- Keep the `<name>.tmp` then rename step when creating an entry, so a crash leaves no half-made one. The entry still has no fetch refspec (ADR 0001).
- `tree:0` and `--shallow-since` are not options: at depth 1 `tree:0` saves nothing and `ls-tree -r` fails without lazy fetch, which the no-lazy-fetch rule forbids.

## Consequences

- The Cache is not history, so the glossary and spec say "fetched commits".
- A host without `uploadpack.allowFilter` (SourceHut) ignores the filter with a warning and exit 0: the blobs arrive with the commit, and the "assert the clone is partial" test must not be run against such a remote.
- Not verified: Azure DevOps, Bitbucket Data Center and self-managed GitLab or Forgejo; git older than 2.55 (the minimum is 2.36); a commit that no ref reaches; `gc` after many small packs on Windows. The full experiments are in `docs/research/add-ux-open-questions.md`.
- TODO: try a fetch and a sync on a very large repo (chromium or linux) and measure it.
