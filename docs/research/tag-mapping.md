# Version to tag mapping: what real tag lists showed

Evidence for #58 and spec §7.2. The script is `prototypes/tag-mapping/match.py` on branch `prototype/tag-mapping`; it runs candidate matching over `git ls-remote --tags --refs` lists.

## Repos tried

tokio, log, serde, TanStack/query, changesets, solid, babel, zod, flask and requests, plus a repo with no tags: monorepos with per-package tags, repos with plain `v` tags and some with none (#58).

## What decided the order

- **A bare `v{ver}` in a monorepo can be another package's release.** babel has `v7.23.2` and `@babel/core@7.23.2` on different commits, so package-qualified candidates go first.
- **Several naming styles are in use**, so the candidates are `{name}@{ver}`, `{name}-v{ver}`, `{name}-{ver}`, `{name}_v{ver}`, `{name}_{ver}`, `v{ver}` and `{ver}`.
- **A scoped npm name** is tried by its full name and by the part after the slash.
- **Exact match only.** The script prints "any tag ending in the version" beside the exact hits to show what a loose match would add (its cases include `requests` `2.31.0.post1` beside `2.31.0`).
- **Annotated tags need nothing extra**: `ls-remote --tags --refs` lists each tag once, by its own name, and `resolve` peels `^{}`.

The decision and its failure policy are in #58 and spec §7.2.
