# Version to tag mapping: what real tag lists showed

Evidence for #58 and spec §7.2. The script is `prototypes/tag-mapping/match.py` on branch `prototype/tag-mapping`; it runs candidate matching over `git ls-remote --tags --refs` lists.

## Repos tried

tokio, log, serde, TanStack/query, changesets, solid, babel, zod, flask and requests, plus a repo with no tags. They cover a monorepo with per-package tags (tokio, changesets, babel), plain `v` tags (log, zod), bare version tags (log's early releases, flask) and none.

## What decided the order

- **A bare `v{ver}` in a monorepo can be another package's release.** babel has `v7.23.2` and `@babel/core@7.23.2` on different commits, so package-qualified candidates go first.
- **Several naming styles are in use**: `{name}@{ver}` (changesets, babel), `{name}-v{ver}` and `{name}-{ver}` (tokio), `{name}_v{ver}` and `{name}_{ver}`, `v{ver}`, `{ver}`.
- **A scoped npm name** is tried by its full name and by the part after the slash.
- **Exact match only.** "Any tag ending in the version" matches too much (`2.31.0.post1` against `2.31.0`, a tag of another package in the same repo).
- **Annotated tags need nothing extra**: `ls-remote --tags --refs` lists each tag once, by its own name, and `resolve` peels `^{}`.

The decision and its failure policy are in #58 and spec §7.2.
