## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues (lukaprsina/refs-cli) via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Checks

Run `scripts/check` before committing; CI (on pushes to `prod`) and `.githooks/pre-commit` run it too. Enable the hook with `git config core.hooksPath .githooks`.
