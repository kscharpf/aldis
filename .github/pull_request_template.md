## Summary

<!-- What changes and why. -->

## Requirements

<!-- Requirement IDs implemented, verified, added, or changed (e.g. SPP-002, CMN-020). -->

## Decisions

<!-- Maintainer decisions taken on this PR, each also a line of docs/process/decisions.md;
     questions for the maintainer, each with a recommended option; coordinator decisions and
     judgement calls made under docs/process/decision-rights.md. Delete what does not apply. -->

## Checklist

- [ ] Wire behavior checked against the Blue Book clause(s) cited (never from memory)
- [ ] Requirement tables in `docs/` updated in the same change (new IDs never reuse old ones)
- [ ] Tests carry `// req: <ID>` annotations; test vectors added or extended where marked V
- [ ] No released test vector edited (new case + `superseded` instead)
- [ ] No CCSDS documents or text copied from them committed
- [ ] Any tool or document this change depends on is free for this project (VER-055)
- [ ] `cargo xtask verify --publish` passed on the head commit, which contains the tip of `main`, locally or through the `verify` workflow (VER-058, VER-060; see AGENTS.md)
- [ ] A fragment added in `changelog.d/` if behavior, public API, or the spec changed (see `changelog.d/README.md`)
- [ ] Each maintainer decision taken here is a line of `docs/process/decisions.md` (VER-062)
