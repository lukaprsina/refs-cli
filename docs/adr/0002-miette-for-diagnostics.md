# Diagnostics use miette, with stable codes

Library errors are `thiserror` enums that also derive `miette::Diagnostic`, so `sync`, `lock` and `doctor` share one diagnostic contract (severity, help, stable code, optional labelled spans) instead of a bespoke type. Only `refs.toml` validation errors carry source spans; git and I/O failures are plain messages with help text. Tests assert on diagnostic codes, not message strings. We chose miette over anyhow (no spans, no severity or help) and color-eyre (aimed at developers reading crash dumps, not end users).

## Consequences

- The `fancy` feature (graphical output) is the heavy part; base miette is light. The library must not depend on rendering. If `fancy` compile time hurts, split the binary into its own crate.
- Config types wrap the fields validation points at in `toml::Spanned`.
