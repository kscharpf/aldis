## Summary

<!-- What changes and why. -->

## Requirements

<!-- Requirement IDs implemented, verified, added, or changed (e.g. SPP-002, CMN-020). -->

## Checklist

- [ ] Wire behavior checked against the Blue Book clause(s) cited (never from memory)
- [ ] Requirement tables in `docs/` updated in the same change (new IDs never reuse old ones)
- [ ] Tests carry `// req: <ID>` annotations; test vectors added or extended where marked V
- [ ] No released test vector edited (new case + `superseded` instead)
- [ ] No CCSDS documents or text copied from them committed
- [ ] Any tool or document this change depends on is free for this project (VER-055)
- [ ] `cargo fmt`, `cargo clippy`, `cargo test` and the bare-metal build pass locally (see AGENTS.md)
- [ ] A fragment added in `changelog.d/` if behavior, public API, or the spec changed (see `changelog.d/README.md`)
