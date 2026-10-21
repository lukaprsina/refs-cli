# `refs add` UX: open questions checked against primary sources

Research for the notes in `docs/private/ux/ADD-UX-NOTES.md` and `ADD-UX-NOTES-revision.md` (hypotheses, not facts; the decisions are in ADR 0010 and 0011). Dates: experiments were run on 2026-10-21 with **git 2.55.0.windows.5** (plain git, scratch directories under `$env:TEMP`, scripts kept outside the downloaded content) against public hosts. "Experiment" means I ran it; "doc" means a primary source says so. Design vocabulary (module, interface, seam, adapter, depth, locality) is from the codebase-design skill.

Not checked, on purpose: Azure DevOps live (the Git Credential Manager popped up for a repo that was not anonymously readable, so I stopped; Azure rows are doc-only), and the node and chromium numbers (not downloaded; the notes' node figures are not re-measured).

## Verdicts

| # | Question | Verdict | Consequence for the design |
|---|---|---|---|
| A1 | Cache by `init --bare` + `fetch --depth 1 --filter=blob:none origin <sha>` works on real hosts | **CONFIRMED** on GitHub, GitLab.com, Codeberg, Gitea.com, Bitbucket Cloud, SourceHut (experiment); Azure DevOps, Bitbucket Data Center, self-managed Forgejo/GitLab not run | Adopt, keep a fallback for hosts that refuse an unadvertised sha. Detection already exists (`commit_unavailable`). |
| A2 | `--depth 1` + `--filter=blob:none` + later `fetch --stdin` of blob ids is reliable | **CONFIRMED** (5 filtering hosts, git 2.55 only) | `fetch` writes the promisor config itself; no extra setup. Older git than 2.55 untested. |
| A3 | A shallow Cache breaks history-dependent calls | **DENIED for the code**: no call site uses history | Also update GLOSSARY.md:88 and spec 7.2/7.3 wording. |
| A4 | Documented reason for 66 s vs 1.5 s; `tree:0` suits `ls-tree -r` | Reason **CONFIRMED** (doc); `tree:0` **DENIED** as a better option at depth 1 | `blob:none` clones fetch every commit and tree. No intermediate option beats `--depth 1`. |
| B1 | `inquire` 0.9.4 `MultiSelect` copes with thousands of options | **CONFIRMED** to about 50k options (experiment on the matcher); empty selection allowed; empty option list is an error | Add an empty-options guard. Header = help message only. |
| B2 | Raw mode on creation, off on drop; own UI between prompts is fine | **CONFIRMED** (file:line below) | `Prompter` seam is sound. |
| B3 | `Prompt` trait unreachable from outside the crate | **CONFIRMED** | A custom prompt type means a fork. |
| B4 | `nucleo-picker` as the fallback | **PARTLY**: crossterm 0.29 matches (no duplicate), Windows is in CI, multi-select exists; but it uses the alternate screen and brings rayon | Viable fallback; the "no spike needed" part is still open (see Still unverified). |
| C1 | `solid-js/h` is an invalid npm name; no legitimate slash name except `@scope/name` | **CONFIRMED** | Name validation is sound. |
| C2 | The solid shims have a name, no version, no `private` | **CONFIRMED** | |
| C3 | "Nested manifest with a name but no version" is a reliable discriminator | **DENIED as stated** | Counter-examples both ways; see section C. |
| C4 | Cargo/PyPI equivalents | **PARTLY**: name rules found; shims do not exist there; a "no version" rule would be wrong for Cargo, PyPI and Go | The rule must be npm-only. |
| D1 | `npm:solid --ref next` is coherent with `registry::expand` and `add::prepare` | **CONFIRMED by reading** (not run) | |
| D2 | npm packuments carry `gitHead` per version | **PARTLY**: it is set at publish time only when the package is published from a git repo root; not documented in the registry docs; solid-js latest has none | Do not rely on it. |
| E | Locking, idempotence, gc, `ls-tree` size | See section E | `ls-tree -r -d` shrinks the directory listing 35x. |

## A. Shallow, blobless Cache fetched by sha

### A1. Can a server be asked for an arbitrary sha?

**Documented behaviour**
- `uploadpack.allowTipSHA1InWant`, `allowReachableSHA1InWant` and `allowAnySHA1InWant` default to unset/false; `allowAnySHA1InWant` implies the other two. Source: git `Documentation/config/uploadpack.adoc` (raw.githubusercontent.com/git/git/master/Documentation/config/uploadpack.adoc).
- **In protocol v2 any sha the server has is accepted.** Wire spec: "`want <oid>` ... Wants can be anything and are not limited to advertised objects" (`Documentation/gitprotocol-v2.adoc`, `fetch` arguments). Server code: v2 `parse_want` (`upload-pack.c:1437-1458`) only refuses an object the repository does not have ("not our ref"); there is no reachability check in the v2 path, while the v0 path does it in `check_non_tip` (`upload-pack.c:757-780`) gated on `allow_uor`. Jeff King states it plainly: "in protocol v2, the reachability restrictions are lifted (i.e., every server behaves as if `uploadpack.allowAnySHA1InWant` is enabled)" (git mailing list, 2019-06-14, public-inbox.org/git/20190614152911.GC22984@sigill.intra.peff.net/T/). Same message: over smart HTTP in v0 a server behaves as if `allowReachableSHA1InWant` were set; over ssh/git:// in v0 it does not.
- Protocol v2 is the client default since git 2.26 (`Documentation/config/protocol.adoc`: "If unset, the default is `2`"; 2.26 release notes: "The transport protocol version 2 becomes the default one"). A v2 request for the sha is therefore the normal case; a server that only speaks v0 over ssh needs `allowReachableSHA1InWant`.
- `uploadpack.allowFilter` is what lets a server honour `--filter`; without it the client prints `warning: filtering not recognized by server, ignoring` and does a full fetch with exit status 0 (`fetch-pack.c:324`, `:1221`; docs/spec/spec.md:265 records the same for a `file://` fixture). pip hit exactly this class of problem (pypa/pip#14163, 2026-07).

**Experiments** (script: `probe.sh`; each host: fetch `HEAD` at depth 2 to learn the tip and its parent, then `git init --bare` + `fetch --no-tags --depth 1 --filter=blob:none origin <sha>` for the tip and for the parent, then a made-up sha). The parent is not a ref tip.

| Host (repo) | Advertises (`GIT_TRACE_PACKET`) | tip sha | non-tip parent sha | made-up sha | blobs omitted? |
|---|---|---|---|---|---|
| github.com (rust-lang/cargo-bisect-rustc) | v2 | ok | ok | `fatal: remote error: upload-pack: not our ref <sha>` (rc 128) | yes |
| gitlab.com (gitlab-org/cli) | v2, `fetch=shallow wait-for-done filter` | ok | ok | same message | yes |
| codeberg.org (Codeberg/pages-server; Forgejo) | v2 | ok | ok | same | yes |
| gitea.com (gitea/tea) | v2 | ok | ok | same | yes |
| bitbucket.org (atlassian/aws-s3-deploy) | v2, `fetch=shallow wait-for-done filter` | ok | ok | same (plus a harmless `unable to write response end packet: Broken pipe`) | yes |
| git.sr.ht (~sircmpwn/scdoc) | v2, `fetch=shallow wait-for-done` (no `filter`) | ok | ok | same | **no**: `warning: filtering not recognized by server, ignoring`; blobs arrive with the commit, still at depth 1 |

- Local `file://` server (git 2.55), repository with a reachable non-tip commit and an unreachable (deleted branch) commit: **v2 fetches both by sha**; **v0 refuses both** with `error: Server does not allow request for unadvertised object <sha>`; v0 with `uploadpack.allowReachableSHA1InWant=true` fetches the reachable one only; v0 with `allowAnySHA1InWant=true` fetches the unreachable one too.
- GitHub with `-c protocol.version=0` also fetched the non-tip parent (smart-HTTP behaviour described by Jeff King above).
- Blob ids: `-c fetch.negotiationAlgorithm=noop fetch --no-tags --no-write-fetch-head --recurse-submodules=no --filter=blob:none origin --stdin` with 5 blob ids on stdin worked on all five filtering hosts (0.5 to 1.0 s each; blobs missing before, present after); a `GIT_NO_LAZY_FETCH=1 checkout` of blobs not prefetched fails with `could not fetch <oid> from promisor remote`, the string `remote::missing_object` already parses (`src/source/git/remote.rs:212`).

**Hosts documented but not run**
- **GitLab self-managed**: Gitaly adds `uploadpack.allowFilter` and `uploadpack.allowAnySHA1InWant` for smart HTTP (Gitaly MR !1411, 2019, gitlab.com/gitlab-org/gitaly/-/merge_requests/1411). The 2026 pip thread reports self-hosted GitLab behind HTTPS load balancers that drop lazy blob fetches, so the blob step is the likelier failure there (secondary report; I did not reproduce it).
- **Bitbucket Data Center**: partial clone on by default since 7.13 when the server git is 2.18+, with the admin switch `plugin.bitbucket-git.hosting.allow-filter=false` (BSERV-11639, jira.atlassian.com). Nothing found on fetch-by-sha; unverified.
- **Bitbucket Cloud**: filters rolled out 2024-08-08 (BCLOUD-19847); confirmed by experiment above.
- **Azure DevOps**: blobless and treeless filters supported since Sprint 227 (Microsoft Learn release notes; devblogs.microsoft.com "Git Partial Clone Now Supported in Azure DevOps", 2023-11-01), "not yet in Azure DevOps Server". Shallow `--depth` supported. Arbitrary-sha fetch: unverified; a 2018 Azure Pipelines issue shows `Server does not allow request for unadvertised object` from the checkout task (microsoft/azure-pipelines-tasks#8725), which predates v2 use there. Treat as unknown.
- **Forgejo/Gitea self-hosted**: codeberg.org and gitea.com work; per-instance config is not documented in what I read. Unverified.

**Failure shape and fallback**
- Two messages cover it: v2 `upload-pack: not our ref <sha>` and v0 `Server does not allow request for unadvertised object <sha>`. `commit_unavailable` (`src/source/git/remote.rs:207`) already matches `not our ref` and `unadvertised object`; both were seen in the experiments above, so detection needs no new code. Missing `filter` support is not an error (exit 0, warning on stderr); it is detectable from the `warning: filtering not recognized by server` line or from `remote.origin.partialclonefilter` being set while blobs are present.
- A clean fallback, reasoning rather than experiment: when the sha is refused and the Pin's ref is known, `fetch --depth 1 origin <ref>` and accept only if the result equals the sha; otherwise `fetch --filter=blob:none origin <ref>` (today's cost), then `ensure_commit` again. `Source::resolve` already returns the ref and sha together, so the information is there. I did not build or run this.
- Caution from a primary commit message (cockroachdb/cockroach@62fa991, 2026-06): on one GitHub-hosted setup a lazy blob want for a commit not reachable from the default branch was rejected while commit-level wants worked. I did not reproduce this against the repos above (the cargo-bisect-rustc parent commit worked); the notes' 5-blob test only used default-branch history on every host. Worth one test with a tag-only or non-default-branch commit before committing to the design.

### A2. `--depth 1` + `--filter=blob:none` + later blob fetch

- `git init --bare` then `remote add origin <url>` then `fetch --no-tags --depth 1 --filter=blob:none origin <sha>` leaves, in `config`, exactly `remote.origin.promisor = true` and `remote.origin.partialclonefilter = blob:none` (experiment, cargo, git 2.55). No `extensions.partialclone` line appeared. Documented: a remote is a promisor remote if named by `extensions.partialClone` **or** `remote.<name>.promisor = true` (`Documentation/technical/partial-clone.adoc`). Jeff King confirms fetch writes the promisor config whenever it stores objects from a filtered fetch (public-inbox, 2025-09-18, "git fetch --dry-run --filter makes changes to .git/config"). So **no manual config is needed**.
- The commit is usable at once: `cat-file -t` gives `commit`; `shallow` lists the sha; `ls-tree -r` works (cargo tag 0.60.0: 685 entries, 4.75 KiB pack after one fetch).
- Re-fetching a sha already present exits 0 (0.64 s, a network round trip; `ensure_commit` already checks `object_type` first so it never does this). A second, different sha adds a second `shallow` line and a second small pack; both remain usable (experiment).
- `fetch --stdin` of blob ids into the shallow repo works (5 hosts, A1), and `worktree add --no-checkout` + `sparse-checkout set --cone` + `checkout` works from the shallow bare repo (SourceHut run: 32 files, status clean; the other hosts correctly failed only because my script prefetched 5 blobs, not the full set).
- The "broken pipe" on a whole-node `worktree add` is not reproduced here and, as the notes say, is an unsparse checkout; not checked.
- **Git version.** `GitSource` requires **2.36.0** (`src/source/git/remote.rs:82`; reason in docs/spec/spec.md section 7.5: `sparse-checkout` in a worktree of a bare repo). Every flag proposed (`init --bare`, `fetch --depth/--filter/--stdin/--no-write-fetch-head`) predates that; `fetch.negotiationAlgorithm=noop` is already used today. Not bisected on any git older than 2.55.
- Today's setup (`src/source/git/cache.rs:171-183`): `clone --bare --filter=blob:none -- <url> <tmp>` then rename; `fetch_commit` (`cache.rs:233-248`) is `fetch --no-tags --filter=blob:none origin <sha>` with no `--depth`. Replacing the clone with `init --bare` + `remote add` must keep the `<name>.tmp` then rename step for crash safety. ADR 0001 notes that a bare clone has no fetch refspec; `git remote add` would add `+refs/heads/*:refs/remotes/origin/*`. In the experiments `for-each-ref` stayed empty after fetching by sha, so it is harmless, but `git config --unset remote.origin.fetch` or `git remote add` replaced by `git config remote.origin.url` keeps the old shape. Both untested against `ls-remote`.

### A3. Audit: what a shallow Cache would break

Every git invocation in `src/` (grep for `Command::new`/`Cmd::new` and `.args([...])`; the only other process is `src/worktree.rs:22`, which runs on the user's project, not the Cache):

| Call site | Command | Uses history? |
|---|---|---|
| `src/source/git.rs:65,78` (`resolve`) | `ls-remote [--symref] <url> <patterns>` | No: remote refs, no Cache |
| `src/source/git.rs:89` (`tags`) | `ls-remote --tags --refs <url>` | No: remote refs, no Cache |
| `cache.rs:178` | `clone --bare --filter=blob:none` | Would be replaced |
| `cache.rs:201` | `cat-file -t <oid>` | No |
| `cache.rs:235` | `fetch --no-tags --filter=blob:none origin <sha>` | Add `--depth 1` |
| `cache.rs:255` | `ls-tree <sha> -- <path>` | No: one tree |
| `cache.rs:269` | `ls-tree -z [-r] <sha> [-- paths]` | No |
| `cache.rs:290` | `cat-file --batch-check` | No |
| `cache.rs:300` | `-c fetch.negotiationAlgorithm=noop fetch ... --stdin` | No: blobs by id |
| `command.rs:183` | `cat-file --batch-check` | No |
| `git.rs:255,303,306,320,354` | `worktree repair / remove / prune` | No |
| `git.rs:260` | `rev-parse HEAD` in the Checkout | No |
| `git.rs:274` | `status --porcelain=v1 -z` in the Checkout | No |
| `git.rs:323,330,337` | `worktree add --no-checkout --detach`, `sparse-checkout set --cone`, `checkout --detach` | No |

- No call to `describe`, `merge-base`, `log`, `rev-list`, `show`, `diff`, `for-each-ref`, `tag` or `cherry` exists in `src/`. Tag resolution is `ls-remote`, which never touches the Cache; annotated tags are peeled by the `^{}` pattern, and `ensure_commit` refuses a tag object (`cache.rs:222-224`).
- **Wording that would become false**: GLOSSARY.md:88 ("The machine-wide store of fetched Repo history"), docs/spec/spec.md:76, :260 ("Create it with `git clone --bare --filter=blob:none <url>`"), :263 ("Blobless is not shallow ... a shallow or `tree:0` mode is a later option") and :523. The spec already lists shallow as a deferred option, so this is the planned follow-up.
- Dangling-checkout recovery (spec :290) is unaffected: Checkouts hold no history.

### A4. Cost of the clone and the alternatives

- **Why `clone --bare --filter=blob:none` is slow.** Documented: a blobless clone "will download all reachable commits and trees" and fetches blobs on demand (GitHub blog, Derrick Stolee, "Get up to speed with partial clone and shallow clone", 2020-12-21; consistent with partial-clone.adoc "Non-Goals": partial clone is independent of DAG-level limits such as shallow). The Cache pays for every commit and tree in history; depth 1 pays for one tree snapshot. docs/spec/spec.md:263 already states this.
- **Experiment, cargo (one run each, one network):**

| Command | Time | Size | Notes |
|---|---|---|---|
| `clone --bare --filter=blob:none` | 5.5 s | 26 MB | 24,308 commits |
| `clone --bare --filter=tree:0` | 11.3 s | 12 MB | commits only; time is noise-dominated, size is real |
| `fetch --depth 1 --filter=blob:none origin <sha>` (tag 0.70.0 and an older sha) | 0.6 to 0.8 s | 195 KB for two commits | |
| `fetch --shallow-since=2023-01-01 --filter=blob:none origin 0.70.0` | 1.0 s | 500 KB | 415 commits; arbitrary cut-off, no use here |
| rust-lang/rust, `fetch --depth 1 --filter=blob:none origin <HEAD sha>` | 1.2 s | 2.2 MiB | 4,646 objects, 63,858 files |

  The notes' node figures (66 s vs 1.5 s) were not re-measured; the cargo numbers agree in direction (about 8x faster, 130x smaller).
- **`tree:0` does not suit `ls-tree -r`.** With `--depth 1 --filter=tree:0`, `GIT_NO_LAZY_FETCH=1 git ls-tree -r <sha>` fails with `fatal: not a tree object` (the root tree is absent; experiment). Without that variable git lazily fetches the trees (one request, 112 objects, 0.7 s total on cargo), which means the "no lazy fetch" invariant in `Repo::entry_kind`/`prefetch` could not hold. Doc: treeless clones "download all reachable commits while fetching trees and blobs on-demand" and are for throw-away builds (GitHub blog above). At depth 1 there is nothing for `tree:0` to save. **No intermediate option beats `--depth 1`**; `--shallow-since` and `--deepen` only matter if history is wanted.
- Azure DevOps supports `tree:0` as well as `blob:none` (release notes above); not needed.

## B. Picking directories and files in a terminal

### B1. `inquire` 0.9.4 `MultiSelect` (checkout at `C:\Luka\code\repos\inquire\inquire\src`)

- **Fuzzy** is the default feature (`inquire/Cargo.toml:23`) using `fuzzy-matcher`'s `SkimMatcherV2` with `ignore_case` (`prompts/multiselect/mod.rs:30-32, 182-185`). Custom scorer via `with_scorer`.
- **Cost per keystroke.** `run_scorer` (`prompts/multiselect/prompt.rs:208-239`) scores every option, collects, then `sort_unstable_by_key` (so equal scores come out in arbitrary order). `render` (`:322-347`) builds a `Vec<ListOption<&T>>` of **all** scored options on every redraw, including plain cursor moves, then paginates. So rendering is O(n) per redraw even though only `page_size` rows are drawn. `DEFAULT_PAGE_SIZE` is 7 (`config.rs:23`); `with_page_size` changes it.
- **Experiment** (release build, `fuzzy-matcher` 0.3.7, synthetic paths of 2 to 6 segments, score + sort only; this machine):

  | options | query `s` | query `solid` | query `packages/solid` |
  |---|---|---|---|
  | 5,000 | 3.0 ms | 1.1 ms | 0.7 ms |
  | 50,000 | 19.8 ms | 10.8 ms | 6.7 ms |
  | 500,000 | 204 ms | 110 ms | 66 ms |

  So 5k options (the notes' cap) is far below a frame; 50k is borderline-fine; 500k is not interactive. The per-redraw `Vec` is not in these numbers. Real repos: rust-lang/rust has 4,821 directories (`ls-tree -r -d`), which fits the cap.
- **Empty selection is allowed**: `submit` returns `get_final_answer()` when no validator rejects (`prompt.rs:269-279`); the type doc says "including none if applicable" (`mod.rs:33`). **Empty option list is an error**: `MultiSelectPrompt::new` returns `InvalidConfiguration("Available options can not be empty")` (`prompt.rs:39-43`), so a repo with no directories needs a guard before the prompt.
- **Help message / header.** Only a help line under the list: `with_help_message` / `without_help_message`; default text "up/down to move, space to select one, right to all, left to none, type to filter" (`mod.rs:218-219, 243-252`). There is no header above the options; the `message` is the prompt line. A "none selected = whole repo" hint therefore goes in the message or the help message.
- **Validator.** `with_validator` takes a `MultiOptionValidator<T>` called with the selected `ListOption`s at submit; an invalid result sets `self.error` and the error is shown one line above the prompt (`mod.rs:285-292`, `prompt.rs:172-189, 269-279`).
- Results come back in option order, not selection order (`get_final_answer`, `prompt.rs:191-205`).
- **Not a TTY** is `InquireError::NotTTY` (`error.rs:15, 69`).

### B2. Raw-mode lifecycle

- `raw_prompt` calls `get_default_terminal()` (`prompts/multiselect/mod.rs:416`), which builds `CrosstermTerminal::new()` (`terminal/mod.rs:76-81`); `new` calls `terminal::enable_raw_mode()` and enables bracketed paste (`terminal/crossterm.rs:89-95`). The terminal lives in the `backend` local of `raw_prompt`, so it drops when the call returns.
- `Drop` flushes, disables bracketed paste and calls `disable_raw_mode` (`crossterm.rs:232-242`).
- All drawing goes to **stderr** (`crossterm.rs:2, 97`), inline; `grep -i alternate` over `inquire/src` finds nothing. The only global state is `set_global_render_config` (`lib.rs:87`).
- Key events are filtered to `KeyEventKind::Press` (`crossterm.rs:73`), the Windows double-event pitfall.
- **Verdict:** each prompt owns raw mode only for its own duration, so a custom step between two prompts (own UI, `eprintln!`) runs in cooked mode. The notes' claim is correct.

### B3. `Prompt` is unreachable

- `pub trait Prompt<Backend>` is at `prompts/prompt.rs:44`, but `prompts/mod.rs:11` declares `mod prompt;` (private) and no `pub use prompt::...` exists; `lib.rs:79` has `mod prompts;` (private) with only `pub use crate::prompts::*` (`lib.rs:90`), which re-exports what `prompts/mod.rs` makes pub (`action`, `confirm`, `multiselect`, ... not `prompt`). `grep "pub use.*prompt::"` over the crate finds nothing, and `Prompt<` appears only in `prompts/**`. The loop that drives a prompt (`Prompt::prompt`, `prompt.rs:~105-140`) therefore cannot be called with a user-defined type. `inquire::ui` is `pub` (`lib.rs:82`) but only offers the render backend. A new prompt type = fork.

### B4. Alternatives (read from `Cargo.toml` and source; crates.io metadata fetched 2026-10-21)

| Crate | State | crossterm | Screen | Windows | Multi-select | Licence |
|---|---|---|---|---|---|---|
| **nucleo-picker** 0.12.2 (published 2026-09-04; 0.13.0-beta.6 exists; last commit 2026-10-19; about 59k downloads) | active | `crossterm = "0.29.0"` (`Cargo.toml`), same as inquire 0.29 | **alternate screen** (`src/terminal.rs:97-105`), on stderr; `PickError::NotInteractive` if stderr is not a terminal | `windows-latest` in `.github/workflows/rust.yml`; docs mention Windows key-event behaviour (`src/event.rs:454`) | yes: `pick_multi` in 0.12.2 (`src/lib.rs:1059`); 0.13 moves it to `Picker::multi()`/`MultiPicker` and deprecates `pick_multi` (CHANGELOG, unreleased) | MIT OR Apache-2.0 |
| **skim** 5.8.0 (updated 2026-10-20; about 2.1M downloads) | active | via ratatui 0.30 (resolves to crossterm 0.29, no duplicate) | full-screen or inline viewport; uses the alternate screen (`src/skim.rs:449-456`) | Windows job in `test.yml` | yes | MIT |
| **dialoguer** 0.12.0 (2025-08-23; about 90M downloads) | active | none: uses `console` 0.16, not crossterm | inline | yes | `MultiSelect` has **no** filtering; `FuzzySelect` is **single-select** only (`src/prompts/fuzzy_select.rs`, `multi_select.rs`) | MIT |
| **fuzzy-select** 0.1.3 (2024-08-05; about 7.7k downloads) | stale | **crossterm 0.27** (duplicate) | inline | untested | single-select API only | MIT OR Apache-2.0 |

- **Duplicate crossterm cost.** None for nucleo-picker or skim: `cargo tree -d` on a scratch crate with `inquire =0.9.4` and `nucleo-picker =0.12.2` printed "nothing to print"; the repo's `Cargo.lock` already has crossterm 0.29.0. fuzzy-select would add a second crossterm (0.27) and `nucleo` 0.5.
- **Dependency weight** (`cargo tree -e normal --prefix none | sort -u | wc -l`): inquire 28 crates, nucleo-picker 37, dialoguer 17, skim 268. nucleo-picker brings `rayon`, `rayon-core`, `crossbeam-deque/epoch/utils`, `either`, `ncp-engine`, `ncp-matcher`, `unicode-normalization`, which the repo's lockfile (281 packages) does not have; `parking_lot`, `memchr`, `smallvec` it already has.
- **The matcher is not Helix's `nucleo` itself**: `nucleo-picker` depends on `ncp-engine`, "an unstable fork of helix-editor/nucleo" by the same author (its README). Upstream `nucleo` 0.5.0 was last released 2024-04-02. The notes' "built on nucleo (the one in Helix)" is approximate.
- `nucleo-picker`'s default feature `unix-poll` enables `crossterm/use-dev-tty` (`Cargo.toml`); with Cargo feature unification that would also apply to inquire's crossterm on Unix. Behaviour effect untested.
- A picker with an alternate screen leaves the prior transcript intact when it exits, so it composes with inquire prompts either side of it; interaction untested.

## C. Package names and manifest shims

### C1. npm name rules (primary: `npm/validate-npm-package-name` 8.0.0, `lib/index.js`)

- A name passes if `encodeURIComponent(name) === name`, or it matches `^(?:@([^/]+?)[/])?([^/]+?)$` with both parts URL-safe. A slash anywhere else is `name can only contain URL-friendly characters`. Also errors: empty, leading `.`/`-`/`_`, leading/trailing space, `node_modules`, `favicon.ico`; warnings (not errors): >214 chars, capitals, `~'!()*`, core module names.
- **Experiment** running that file under node: `solid-js/h`, `solid-js/web/storage`, `a/b`, `@a/b/c` and `@scope` all return `validForOldPackages:false` with that error; `@solidjs/web` and `solid-js` pass. So `solid-js/h` is invalid. **No legitimate unscoped slash name exists** under the validator, which is also what the registry URL scheme needs. `validForOldPackages` (errors only) is the right check for inference, as capitals and the like are warnings that old published names trip.

### C2. The solid shims (experiment: blobless depth-1 clone of solidjs/solid, `**/package.json` only)

14 manifests. Root `package.json`: name `solid-js`, version `1.0.0`, `private: true`. Real packages: `babel-preset-solid`, `solid-element`, `solid-ssr`, `packages/solid` (`solid-js` 1.9.17). Eight shims, e.g. `packages/solid/h/package.json`:

```json
{ "name": "solid-js/h", "main": "./dist/h.cjs", "module": "./dist/h.js", "types": "./types/index.d.ts", "type": "module", "sideEffects": false }
```

`name` set with a slash, **no `version`, no `private`**, inside `packages/solid/` (a manifest with a name and a version). The other shims: `h/jsx-dev-runtime`, `h/jsx-runtime`, `html`, `store`, `universal`, `web`, `web/storage`. `packages/test-integration` is private. That gives the notes' 12 candidates (4 real + 8 shims) and confirms the diagnosis.

### C3. Is "nested, named, no version" reliable? Counter-examples found

Surveyed with a script (`nest.js`): for each non-root manifest, find the nearest ancestor manifest that is not the repo root; report versionless ones and name validity. Repos: solid, preact, react, vue core, svelte, rxjs, date-fns, redux-toolkit, emotion (depth-1 sparse clones, default branches on 2026-10-21).

| Repo | Shim pattern found | Matches "nested, named, versionless"? |
|---|---|---|
| solid | `solid-js/h` etc.: name with slash, no version, no `private` | yes (also caught by name validity) |
| vue core | `packages/vue/{compiler-sfc,jsx-runtime,server-renderer}`: **no name, no version**, `main`/`module`/`types` only | no name, so not "named": a nameless manifest is already dropped by `package_json` (`json["name"]` is `None`) |
| svelte | `packages/svelte/compiler/package.json`: only `{"type": ...}` | nameless, same |
| emotion | 9 nested shims (preconstruct): no name, no version | nameless, same |
| preact | `compat`, `debug`, `hooks`, `devtools`, `jsx-runtime`, `test-utils`: **valid names** (`preact-hooks`, ...), `private: true`, no version; they sit directly under the repo root (no non-root ancestor manifest) | no, but `private: true` already drops them |
| redux-toolkit | `packages/toolkit/query` = `@reduxjs/toolkit-query`, `packages/toolkit/react`, `query/react`: **valid scoped names, `version: "1.0.0"`, not private**, `main` pointing at `../dist/...`; the registry has no such packages (`GET /@reduxjs%2ftoolkit-query` returns 404) | **no**: they have a version and a valid name, so name validity and the versionless rule both miss them |
| react | `compiler/packages/*`: nested under `compiler/package.json` (private workspace root), valid names and versions, genuinely published (`babel-plugin-react-compiler`, ...) | no: they have versions, so they survive a "versionless" rule, which is right; but they are nested inside a manifest (`compiler/package.json`), so a rule of "nested in any manifest" alone would wrongly drop them |
| date-fns | `pkgs/core/examples/*`, `pkgs/tz/playground`: named, versionless, mostly `private`; `@date-fns/examples` and `@date-fns/tz-playground` are non-private and versionless | yes, correctly (not published); also under the skip list's `examples` |
| rxjs | `fixture-*` seeds: named, versionless, private | yes, correctly |

- So the rule needs care: "inside another manifest's directory" is **always true** for the root, so it has to mean "inside a **non-root** manifest's directory", and even then the React compiler packages sit inside the private `compiler/` workspace root yet are real. A versionless named manifest is never publishable (npm needs a version to publish), so "no version, not the repo root" as a drop rule fits all observed npm cases. What it misses: redux-toolkit's valid-named versioned shims. They are not detectable by name or version; the only signal I saw is a `main`/`module`/`types` path that leaves the directory (`../dist/...`). That is a heuristic I did not test further.
- **Preferred order**: valid npm name (drops solid), `private` (drops preact), nameless (drops vue, svelte, emotion; already so), then versionless-and-not-root. Redux-toolkit-style shims remain a known false positive.

### C4. Cargo and PyPI

- **Cargo**: `cargo-util-schemas` `restricted_names.rs:47-90`: first char Unicode XID start or `_` (not a digit), rest XID continue or `-`; `::` splits a name into parts. crates.io limits a crate name to 64 characters (`crates_io_validation`, `MAX_NAME_LENGTH`). A slash is never valid. `publish = false` is Cargo's analogue of `private: true`; `packages::cargo_toml` ignores it today (`src/packages.rs:70`). Version may be `version.workspace = true`, so **absence of a literal `version` does not mean unpublished**.
- **PyPI**: name must match `^([A-Z0-9]|[A-Z0-9][A-Z0-9._-]*[A-Z0-9])\Z` case-insensitively; normalised with `re.sub(r"[-_.]+", "-", name).lower()` (PyPA name-normalization spec, packaging.python.org, `name-normalization.rst`). `pyproject.toml` may declare `dynamic = ["version"]`, so version is again not a safe signal.
- **Go**: `go.mod` carries no version at all, and a nested `go.mod` is the official way to make a nested module; the module path legitimately contains slashes (`go_mod` in `src/packages.rs:85`). A versionless rule would drop every Go module.
- **Shims**: neither Cargo nor PyPI has subpath-export manifests; the nested manifests in cargo's own tree (10 of 27 `Cargo.toml` files are nested under another non-root manifest) are all under `tests/testsuite/mock-std/...` (skip-list `tests`). Not surveyed beyond cargo. **The shim rule is npm-only** and belongs in `package_json`, not the generic walk.

## D. Registry facts not already in registry-facts.md

- **`npm:solid --ref next`** (code reading, not run). `registry::expand` (`src/registry.rs:~270-310`): with a shorthand and no version, `repo.git_ref` is kept; it errors (`VersionAndRef`) only when **both** a version and a ref are given. `add::prepare` (`src/add.rs:~195-205`): `ref_settled = expanded.requested.is_some()` skips the ref question, and `pin(...)` runs only `if (None, Some(requested))`, i.e. when no ref was given. So an explicit ref is kept as is and never asked about. Coherent. `expand` also fills `paths` from the registry `directory` when empty; that still applies to `--ref`.
- **Empty `refs.toml`** (code reading + an existing test). `Edit::Add` guards the separator with `!out.is_empty()` (`src/edit.rs:~119`); `Config` is all-`#[serde(default)]` (`src/config.rs:12-24`), so an empty document deserialises; `tests/cli_add_packages.rs:25` already writes a zero-byte `refs.toml` and adds to it. This settles the open item in the notes' section 1 at the level of code; I did not run the test.
- **`gitHead`.** The npm registry docs (`npm/registry` `docs/responses/package-metadata.md`) do **not** list `gitHead` among full or abbreviated version fields. It is written by npm's publish-time normalisation: `@npmcli/package-json` `lib/normalize.js` populates `gitHead` when the step is on and `data.gitHead` is unset, using `git.find({cwd, root})` (npm/package-json, commit fda5722 shows the code). Earlier tooling only did so when the package directory itself is a git root; isaacs: "it's based on ... whether it's published from the root of a git repo or not" (npm/cli#393, 2021-02). Live registry sample (2026-10-21): solid-js 387 of 546 versions have `gitHead` (the latest does not), lodash 6 of 117, react 1,906 of 2,985, zod 898 of 1,013. So `gitHead` is common but not reliable and cannot stand in for tag mapping.

## E. Additional checks

1. **Locking and `outline`.** Every Cache operation runs under a per-entry advisory file lock (`Cache::locked`, `cache.rs:93-107`, `fd_lock::RwLock::write`, `<name>.lock`), taken in `with_repo` and `with_named`. An `outline` implemented through `with_repo` takes the same lock, so it serialises with a concurrent `sync` and sees whole fetches only. The cost: a long `outline` fetch blocks a `sync` of the same URL, and the reverse. Git also holds its own `shallow.lock` per fetch, so two raw git processes would not corrupt it (not tested).
2. **Idempotence** (A2): re-fetch of a present sha is safe and exits 0; skipping it via `object_type` saves a round trip.
3. **Garbage collection.** After `git gc --prune=now` in a repo whose pins were fetched by sha, with no refs, **both pinned commits remained** (`cat-file -t` returned `commit`), because shallow boundary commits are kept as roots; a commit with a linked worktree also remained. (experiment, cargo). The `shallow` file gets one line per pin; a v2 fetch sends a `shallow <oid>` line for each existing shallow commit (`gitprotocol-v2.adoc`, `fetch` arguments), so the request grows with the pin count (hundreds is nothing; not measured). Whether `gc.auto`/`maintenance --auto` after many small packs can disturb the Windows file lock or a concurrent checkout: could not verify.
4. **`ls-tree -r` size** (experiment, rust-lang/rust depth 1): 63,858 files, output 6,805,552 bytes, 0.18 s; `ls-tree -r -d` (directories only) 4,821 entries, 191,749 bytes with `--name-only`, 0.09 s, a 35x reduction. So the directory picker should use `-r -d --name-only` (or `-z`) instead of listing files and deriving directories; files for `start` need `-r --name-only` plus a filter in core (`ls-tree` takes paths, not globs). Chromium and linux sizes: could not verify; scale the rust figures by file count.
5. **Missing blobs and lazy fetch.** `cat-file --batch-check` with `GIT_NO_LAZY_FETCH=1` reports `missing` for absent blobs (5 of 5 before prefetch, 0 after; experiment), which is how `prefetch` decides. `Cmd::no_lazy_fetch` (`command.rs:84`) is already applied to the read-only calls. A `checkout` with the variable set and blobs absent fails with `could not fetch <oid> from promisor remote`, which `remote::missing_object` parses.
6. **Windows paths.** `Cmd::new` already passes `-c core.longpaths=true` (`src/source/git/command.rs`, in `Cmd::new`). The new `init --bare` flow adds no new long paths (pack names are the same). Not run on a deep tree.
7. **Prior art for add-time validation.** `cargo add` documents that, without an explicit source, it picks "Latest release in the registry" and lists enabled features on success (doc.rust-lang.org/cargo/commands/cargo-add.html), so it has already talked to the registry before it writes `Cargo.toml`; `--offline` is the escape. `uv add` resolves first and refuses an unusable requirement with an error and no edit: `uv add "httpx>9999"` prints `No solution found when resolving dependencies` (uv `docs/concepts/projects/dependencies.md`); `--no-sync` skips the environment sync, not the resolve (`docs/concepts/projects/sync.md`, automatic lock and sync). `gh repo clone` has no validation step beyond the clone itself (not checked in docs). So "validate before writing" has precedent; ADR 0005's `--no-sync` parity maps to "skip the checkout", and `outline` is the analogue of uv's resolve.
8. **`git clone --revision`** exists since git 2.49 (Stack Overflow answer quoting the release; not a primary source). It does not help: the repo's minimum is 2.36 and the `init` + `fetch` form is identical in effect.
9. **Per-host `filter` absent** (SourceHut): promisor config is still written, blobs arrive with the commit; `prefetch`'s `cat-file --batch-check` then reports nothing missing and skips the blob fetch. Works, but the "blobless" guarantee and the test in spec :265 ("assert the clone is partial") do not hold there.

## Deviations the notes should make

1. Say "works on GitHub, GitLab.com, Codeberg, Gitea.com and Bitbucket Cloud, protocol v2" instead of "GitHub only"; list Azure DevOps, Bitbucket Data Center and self-managed Forgejo/GitLab as untested, and keep the fallback in the design (ref-based fetch, then today's blobless fetch). Reuse `commit_unavailable` unchanged.
2. Replace "check `describe`/`merge-base`/`log`/`rev-list`" with the audit result: none are used; the only edits are `cache.rs:171-183` (clone), `:233-248` (`--depth 1`), GLOSSARY.md:88 and spec 7.2/7.3 wording.
3. Do not offer `tree:0` or `--shallow-since` as intermediates; depth 1 is strictly cheaper and keeps the no-lazy-fetch invariant.
4. Use `git ls-tree -r -d --name-only` for the directory list (35x smaller than `-r`); keep `-r --name-only` only for `start` files, with the filter in core.
5. Guard an empty option list before `MultiSelect` (it is an error), put the "none = whole repo" hint in the message or help message (there is no header), and expect O(n) work per redraw, fine up to about 50k options.
6. Fix the `nucleo-picker` description: it uses the alternate screen, brings about 10 new crates (rayon family), no duplicate crossterm, and its matcher is the author's `ncp-engine` fork of Helix's `nucleo`. `pick_multi` is in the published 0.12.2 and deprecated for `Picker::multi` in the unreleased branch.
7. The package rule: valid npm name first (it fixes solid), then `private`, then "nameless", then "versionless and not the repo root"; the last two must be npm-only (`package.json`), because Cargo `version.workspace`, PyPI `dynamic = ["version"]` and Go modules all lack a literal version. State that redux-toolkit-style shims (valid scoped name, version 1.0.0, `main` pointing at `../dist`) are a known false positive.
8. Replace "nested manifest inside another manifest's directory" by "inside a non-root manifest's directory"; the repo root contains everything.
9. Add Cargo `publish = false` to the "never imported" filter (npm `private` is handled; Cargo's equivalent is not).
10. Drop `gitHead` from any design that depends on it (not in the registry docs; absent from solid-js latest).
11. The open item "does `npm:solid --ref next` work" and "does `Edit::Add` work on an empty file" can be closed at the code level; a run is still cheap.
12. Note that a SourceHut-style host ignores `--filter` with exit 0 and a warning; the partial-clone assertion in the tests (spec :265) must not be applied to a real remote of that kind.

## Still unverified

- Azure DevOps (Services and Server) fetch-by-sha, `--depth` plus `--filter`, and blob-by-id fetch (anonymous access was not available; the Git Credential Manager prompted and I stopped).
- Bitbucket Data Center and self-managed GitLab/Forgejo/Gitea fetch-by-sha; self-managed GitLab behind a load balancer for the blob step.
- The blob-by-id wants for a commit not reachable from the default branch (the cockroachdb commit message suggests a failure mode; not reproduced with the repos tried).
- The fallback sequence in A1 (ref-based depth 1, then blobless) was reasoned, not run.
- Any git older than 2.55 (the repo's minimum is 2.36).
- node and chromium figures (clone time, tree size, `ls-tree -r` size, `ls-tree -r -d` size); the rust-lang/rust figures are the largest tree measured.
- `nucleo-picker`: runtime behaviour on Windows terminals, the effect of the `use-dev-tty` feature on inquire, and composing its alternate screen with inquire prompts. Only source and CI configuration were read.
- `inquire` redraw cost beyond the matcher (the per-redraw `Vec` of all options) at 50k+ options; only the scoring was timed.
- `npm:solid --ref next` and an add onto an empty `refs.toml` end to end (code and an existing test read; not run).
- `gc.auto`/`maintenance --auto` after many small packs on Windows, and growth of the `shallow` file over hundreds of pins.
- The `main` pointing outside the directory heuristic for redux-toolkit-style shims.
- `gh repo clone` behaviour (no doc read).
