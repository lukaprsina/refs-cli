# TODO

1. #61, infer packages on add from the new Checkout's Manifests. It's independent of the rest and the smallest. It also fixes the stale "npm-style" wording in the spec. I'd start here with tdd.
2. #62, npm:/cargo:/pypi: shorthand through a Registry seam. This is the biggest piece. It adds reqwest, the Registry glossary term, and the opensrc credit in the README and NOTICE.
3. #63, then #64. Pin the tag for a known version, then pick the Used version from the Package lockfile. Both depend on #62, in that order.
4. #65, the Tooling gap advisory on sync. It's independent and can run in parallel with the others.

