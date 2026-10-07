## Agent skills

### Issue tracker

Issues are tracked in GitHub Issues (lukaprsina/refs-cli) via the `gh` CLI. See `docs/agents/issue-tracker.md`.

### Triage labels

Default vocabulary: `needs-triage`, `needs-info`, `ready-for-agent`, `ready-for-human`, `wontfix`. See `docs/agents/triage-labels.md`.

### Domain docs

Single-context: one `GLOSSARY.md` and `docs/adr/` at the repo root. See `docs/agents/domain.md`.

### Checks

Run `scripts/check` before committing; `.githooks/pre-commit` and CI run it too. CI runs on release tags (as the gate before the release is created) and by manual dispatch, not on every push. Enable the hooks with `git config core.hooksPath .githooks`. `scripts/check` covers docs, fmt and clippy but not tests: `.githooks/pre-push` and CI run `cargo test`; run it yourself when needed. On Windows, `scripts/check.ps1` runs it through Git Bash (plain `bash` is WSL there).
