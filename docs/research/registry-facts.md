# Registry facts for `npm:`, `cargo:` and `pypi:` lookups

Research for issue #57 (map #56), feeding ADR 0009. Dates: live API checks were run on 2026-10-08. "Live" means I called the real registry with `curl`; those results can drift, the documented ones cannot be expected to.

## 1. Can the `reqwest` in the tree do blocking HTTPS?

**Short answer: not as configured. The `blocking` feature is off; turning it on needs a Cargo.toml change but is almost free.**

Sources: `Cargo.lock`, `cargo tree -e features -i reqwest`, `reqwest-0.13.5/Cargo.toml`, `axoasset-2.0.1/Cargo.toml`.

- The only `reqwest` in `Cargo.lock` is 0.13.5, reached via `axoupdater` -> `axoasset` (feature `remote`). axoasset declares it as `reqwest = { version = ">=0.13.0", default-features = false, features = ["json", "default-tls"], optional = true }`.
- Features enabled on it today (from `cargo tree -e features -i reqwest`): `default-tls`, `rustls`, `__rustls`, `__rustls-aws-lc-rs`, `__tls`, `json`. **Not** enabled: `blocking`, `http2`, `charset`, `system-proxy`, `gzip`/`brotli`/`deflate`.
- HTTPS already works: reqwest's `rustls` feature is `["__rustls-aws-lc-rs", "dep:rustls-platform-verifier", "__rustls"]`, and `default-tls = ["rustls"]`. Roots come from the platform verifier. (axoupdater itself hits GitHub over this.)
- `reqwest::blocking` is gated by the `blocking` feature, defined in reqwest's Cargo.toml as `["dep:futures-channel", "futures-channel?/sink", "dep:futures-util", "futures-util?/io", "futures-util?/sink", "tokio/sync"]`. Without it `reqwest::blocking` does not exist. (axoupdater's own `blocking` feature is unrelated: it wraps its own tokio runtime and does not enable reqwest's.)
- Other relevant API (`reqwest-0.13.5/src/blocking/client.rs`): `Client::builder()`, `.user_agent(..)`, `.timeout(..)`, `.default_headers(..)`, `.build()`, `Client::get(..)`. `json` is already on, so `Response::json()` works.
- `serde_json` 1.0.151 and `tokio` (features `full` via other deps) are already in the tree, so `blocking`'s `tokio/sync` adds nothing.

### Cost of adding the feature

Cargo feature unification means one line in `[dependencies]` is enough:

```toml
reqwest = { version = "0.13", default-features = false, features = ["blocking", "rustls", "json"] }
```

(`rustls` rather than `default-tls` is equivalent; either selects the same TLS stack. Keep `default-features = false` so `http2`, `charset`, `system-proxy` are not newly pulled in.)

Measured in a scratch copy of the repo outside this worktree (Windows, `cargo build --release`, same target dir for both runs):

| | Crates in `Cargo.lock` | Release `refs.exe` |
|---|---|---|
| Baseline | 276 | 7,115,264 bytes |
| With the line above, plus a dead `blocking::Client::new()` call | 278 | 7,093,248 bytes |

- New crates: only `futures-io` and `futures-sink` (tiny, no transitive deps). `futures-channel` and `futures-util` were already present.
- Binary size: no measurable increase. Caveat: my probe function was unused, so the linker discarded it; real use will add the `blocking` module's code (a thread plus a tokio current-thread runtime per client, a few tens of KB, not measured). Treat size cost as "small, unmeasured".
- Compile time: rebuilding `reqwest`, `axoasset`, `axoupdater` and the crate took 27.6 s after the change (incremental over a warm dep cache). No new heavy crates are compiled, so a cold build does not grow materially. The baseline cold build took 3 m 21 s on this machine.
- Runtime note: `reqwest::blocking` spawns its own thread and runtime; calling it from inside an async context panics. refs-cli is synchronous today, so this is fine.
- The existing `Cargo.lock` still resolves (reqwest stays at 0.13.5).

Alternative not needed: adding `ureq` would be a new dependency tree; there is no reason to prefer it.

## 2. Repository URL and subpath fields

### npm

Sources: npm docs `package.json` page (docs.npmjs.com/cli/v11/configuring-npm/package-json, "repository"); `npm/registry` `docs/REGISTRY-API.md` and `docs/responses/package-metadata.md`; live calls to `registry.npmjs.org`.

- In a manifest, `repository` is an object `{type, url, directory?}` or a string shorthand (`"npm/example"`, `"github:npm/example"`, `"gist:..."`, `"bitbucket:user/repo"`, `"gitlab:user/repo"`). The docs say npm "normalizes the `repository` field to the full object format with a `url` property" at publish, so registry data should be an object; I still recommend tolerating a string (`#[serde(untagged)]`) defensively, since old publishes predate normalization and I could not confirm none exist.
- `directory` is documented as the monorepo subpath: `"directory": "workspaces/libnpmpublish"`. It is optional and relative to the repo root.
- URL spellings are not normalized to https. Live `latest` documents showed: `git+https://github.com/lodash/lodash.git`, `git://github.com/jashkenas/underscore.git`, `git+ssh://git@github.com/Marak/colors.js.git`, `git://github.com/retrofox/is-array` (no `.git`), `https://github.com/babel/babel.git`. The caller must normalize `git+`, `git://`, `git+ssh://git@`, `.git` suffix, and shorthand forms to an https URL.
- Top-level vs per-version: the metadata doc says "Several top-level fields are copied from the most recently published version. These include `author`, `description`, `license`, `maintainers`, and `repository`." (so the top-level one reflects the latest version at publish time of the doc.) Each entry in `versions[v]` carries its own `repository`. Live: `@babel/core` top-level and `versions["8.0.7"]` both gave `{url: https://github.com/babel/babel.git, type: git, directory: packages/babel-core}`. If a version is requested, read the per-version object; for "latest", top-level is equivalent.
- The abbreviated document (`Accept: application/vnd.npm.install-v1+json`) has keys `name, dist-tags, versions, modified` and **no `repository`** (live, and its documented version objects only guarantee `name`, `version`, `dist`). Do not request it.
- Cheapest request: `GET https://registry.npmjs.org/{name}/latest` (REGISTRY-API.md: `GET /{package}/{version}` where version is "a version number or `latest`"). Live size for `@babel/core`: 4.4 KB vs 840 KB for the full packument. It returns a single version object with `repository`, `homepage`, `version`. For a specific version use `/{name}/{version}`. An unknown version/tag gives HTTP 404 with body `"version not found: <x>"` (live).
- `homepage` also exists on the version object (not a repo URL in general; e.g. `https://babel.dev/...`). It is a poor fallback; I would not use it for npm.

### crates.io

Sources: crates.io OpenAPI (`https://crates.io/api/openapi.json`, operation `find_crate`, schema `Crate`); live calls.

- `GET https://crates.io/api/v1/crates/{name}` returns `{crate, versions, keywords, categories}`. `crate.repository` ("The URL to the crate's repository, if set.") and `crate.homepage` ("The URL to the crate's homepage, if set.") are **inside the `crate` object**, not at top level. Both are nullable strings. Live: serde gives `repository = https://github.com/serde-rs/serde`, `homepage = https://serde.rs`.
- Per version, `GET /api/v1/crates/{name}/{version}` returns `{version: {...}}` and `version.repository` / `version.homepage` exist too (live; the crate-level value reflects the crate's current data and may differ from older versions).
- Default response is `include=full` ("Defaults to `full` for backwards compatibility", per OpenAPI) and is large: 441 KB for serde. The documented `include` query parameter accepts a comma list (`versions`, `keywords`, `categories`, `badges`, `downloads`, `default_version`, `full`). `?include=` (empty) returned 952 bytes with `repository` and `homepage` still present and `versions: null` (live). Use that to keep the request small. This is documented only as "comma-separated list"; the empty value relying on it is an observed behaviour, so a fallback to the default request if `crate.repository` were ever absent is cheap insurance.
- Cargo's `repository` is free text set by the publisher. It often points at a monorepo root with no subpath information; crates.io has no `directory` equivalent in this response. A subpath would have to come from the Checkout (ADR 0009's inference), not the registry. (I did not find a crates.io field for it in the `Crate` or `Version` schemas.)
- Alternative: the sparse index (`index.crates.io`) has no rate limit but, per the Cargo index format, does not carry `repository`/`homepage`, so it is not usable here. (Format reference: doc.rust-lang.org/cargo/reference/registry-index.html; live entry for serde had only `cksum, deps, features, name, pubtime, rust_version, vers, yanked`.)

### PyPI

Sources: docs.pypi.org/api/json/; packaging.python.org core-metadata spec; live calls.

- `GET https://pypi.org/pypi/{project}/json` (latest) and `GET https://pypi.org/pypi/{project}/{version}/json` (release; same minus `releases`). Fields are under `info`.
- `info.project_urls`: object of free-form label -> URL (docs example keys: "Bug Reports", "Homepage", "Source", "Funding"). Core metadata defines `Project-URL` as "label, URL" with a free-text label limited to 32 characters. Live label spellings differ per project and case: requests `{"Documentation", "Source"}`, Pillow `{"Source", "Homepage", ...}`, numpy lowercase `{"source", "homepage", "tracker", ...}`, Flask `{"Source", ...}`. Match labels **case-insensitively**. It can be `null` when a project declares none (the field is optional; I did not hit a null live but the core metadata makes it optional).
- `info.home_page`: core metadata `Home-page`, "Deprecated since metadata version 1.2. Per PEP 753, the field is superseded by Project-URL." Live: it was `null` for requests, Pillow, numpy and Flask. Treat as legacy fallback only; it can also be an empty string.
- `info.project_url` (singular) is the PyPI page URL, **not** the repository; do not use it.
- There is no subpath field. Same remark as crates.io.
- Name normalization: `Pillow` and `pillow` both returned 200 with no redirect (live), and `python_dateutil` resolves too; so the user's casing/underscore choice works as typed.

### Prior art check (opensrc, `core/registries`)

Not trusted, compared against the sources above:

- npm: deserializes `repository` as a struct only, so a string shorthand would fail to parse; strips `git+`, replaces `git://`, `git+ssh://git@`, `.git`, `github:`. Matches the live spelling zoo above, minus string shorthand. Uses per-version repo falling back to top-level, fine.
- crates: reads `repository`, then `homepage` (consistent with the OpenAPI schema).
- PyPI: looks up labels `Source`, `Source Code`, `Repository`, `GitHub`, `Code`, `Homepage` with **case-sensitive** map lookup, so numpy's lowercase `source` is only found by its later "any value that looks like a git URL" scan. Then tries `home_page`. A case-insensitive pass is better.

## 3. Request rules

### crates.io

Source: crates.io data-access policy (`svelte/src/routes/data-access/+page.svelte` in `rust-lang/crates.io`, served at crates.io/data-access); middleware `src/middleware/require_user_agent.rs`; live.

- Policy for the API: "A maximum of 1 request per second, and a `user-agent` header that identifies your application. We strongly suggest providing a way for us to contact you (whether through a repository, or an e-mail address...)". The page tells users to prefer the sparse index, then the DB dump, and use the API only "should you be unable to use one of the previous options" (the index does not carry the repo URL, so the API is the right option for us).
- Enforcement: requests with an empty `User-Agent` (or the CloudFront-synthesized `Amazon CloudFront`) get `403 Forbidden` (middleware source, and confirmed live: no `-A` gave 403). Suggested value: `refs-cli/<version> (https://github.com/lukaprsina/refs-cli)`, built with `ClientBuilder::user_agent`.
- Rate: one lookup per `add` is far under 1 rps. No explicit retry-after contract is documented in the sources I read; treat 429 as an error to surface, not retry in a loop.
- `static.crates.io` (downloads) has no limit but we do not use it (ADR 0009: no tarballs).

### npm

- `registry.npmjs.org` has no documented User-Agent or rate-limit requirement in `npm/registry` docs (REGISTRY-API.md says nothing on either). Send a UA anyway; use one shared client UA string.
- Scoped names: REGISTRY-API.md does not document encoding. Live, all of `/@babel/core`, `/@babel%2fcore` and `/%40babel%2fcore` returned 200. The common, safest form is `@scope%2fname` (keep `@`, encode only `/`). The npm client itself does this. For the `/latest` form: `https://registry.npmjs.org/@babel%2fcore/latest`, and the unencoded `/@babel/core/latest` also worked live. Recommend percent-encoding the whole name (`%40babel%2fcore`) with a URL-component encoder; it is the form that is valid under any path-segment rule.
- Unknown package: HTTP 404, body `{"error":"Not found"}` (live).

### PyPI

- docs.pypi.org/api/json/ documents only `200 OK`; it says nothing about 404, rate limits or User-Agent. Live: unknown project gives 404 `{"message": "Not Found"}`. Send a UA anyway.

## 4. Error cases to handle

| Case | npm | crates.io | PyPI |
|---|---|---|---|
| Package not found | 404 `{"error":"Not found"}` | 404 `{"errors":[{"detail":"crate `x` does not exist"}]}` | 404 `{"message": "Not Found"}` |
| Unknown version (only if a version is asked for) | 404 on `/{name}/{version}`, body `"version not found: x"` | 404 on `/{name}/{version}` | 404 on `/{project}/{version}/json` |
| Missing/blank User-Agent | no failure observed | **403** | no failure observed |
| Other transport errors | map any non-2xx to a registry error carrying the status; timeouts via `ClientBuilder::timeout` | same | same |
| No repository URL | `repository` absent (or no `url`) | `crate.repository` null/absent, `homepage` null | `project_urls` null/lacks a source-like label and `home_page` null/empty |
| Non-GitHub host | any host appears (`git+ssh`, gitlab, bitbucket, self-hosted, shorthand) | free text, often a docs or project page | `Homepage` is often a docs site, not a repo |

Notes for the design:

- "No repository URL" and "non-GitHub host" are distinct outcomes that need distinct messages. Whether non-GitHub hosts are accepted depends on what the existing `add` URL handling already allows for plain git URLs; the registry URL should go through the same validation as a hand-typed URL rather than a registry-specific host check. (Not researched here: that is an in-repo question.)
- crates.io `homepage` and PyPI `Homepage` are frequently not repositories, so using them as a fallback needs a host/shape check (looks like a git forge URL) before trusting it; otherwise prefer failing with "no repository URL".
- Normalize before storing: strip `git+`, convert `git://` and `git+ssh://git@host/` to `https://host/`, remove a trailing `.git` and any `#readme`/fragment, expand `github:` shorthand, trim a trailing `/`. Observed PyPI `Source` values include a trailing slash (`https://github.com/pallets/flask/`) and crates/PyPI values can include deep paths (`/tree/main/...`) or `/issues`; that path-handling is a separate parsing question.
- Subpath: only npm provides one (`repository.directory`). crates.io and PyPI give no subpath; `paths` stays empty for them, as ADR 0009 already assumes.
