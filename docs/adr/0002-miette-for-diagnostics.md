# Diagnostics use miette, with stable codes

Library errors are `thiserror` enums that also derive `miette::Diagnostic`, so `sync`, `lock` and `doctor` share one diagnostic contract (severity, help, stable code, optional labelled spans) instead of a bespoke type. Only `refs.toml` validation errors carry source spans; git and I/O failures are plain messages with help text. Tests assert on diagnostic codes, not message strings. In miette 7.6 `Diagnostic::code()` returns `Option<Box<dyn Display>>`, so the assertion is `d.code().unwrap().to_string()`. Codes are named `refs::<area>::<name>`. We chose miette over anyhow (no spans, no severity or help) and color-eyre (aimed at developers reading crash dumps, not end users).

## Consequences

- The `fancy` feature (graphical output) is the heavy part; base miette is light. Measured on a clean build: 3.3s and 69 crates with `fancy`, 1.5s and 14 without. Not enough to split crates now. The library must not depend on rendering; if `fancy` compile time starts to hurt, split the binary into its own crate.
- Config validation reports every error at once: a wrapper error with `#[related]` children, each with its own span, code and help. Each child needs its own `NamedSource`.
- `doctor` findings map onto miette severity: `warn` is `Severity::Warning`, `error` is the default.
- Config types wrap the fields validation points at in `toml::Spanned`.
