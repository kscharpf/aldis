# Analysis and inspection records

Requirements whose evidence is a person's judgement are traced by a **record**: method **I** (inspection), or method **A** (analysis) that no check of the verification run makes. A requirement verified by test, by vector, or by a check of `cargo xtask verify` has **no record**: `cargo xtask trace` reads the check from `CHECK_REQUIREMENTS` in `xtask/src/trace.rs`, and the verify record (VER-057) is the result (OI-63). Each record is one file in [`records/`](records/), named after the first requirement ID it covers, for example [`records/VER-011.md`](records/VER-011.md). Tests and vectors carry their own annotations. `cargo xtask trace` reads every file in `records/` (VER-020). There is no index to update: the directory is the list.

List a requirement only when the artefact actually checks it. A record names its tests and checks and does not describe what they do; the code and the check are the description. Delete the record if the review is removed, or when a check takes over.

## Format

```markdown
# Record: GEN-001, GEN-002, GEN-003, GEN-004

| | |
|---|---|
| Requirements | GEN-001, GEN-002, GEN-003, GEN-004 |
| Method | A |
| Check | Verify check `layers`: `cargo xtask layers` |

Free text: what the check or review establishes, its files, and when it was last done.
```

- The first table has exactly the rows `Requirements`, `Method`, and `Check`, in this order. `Requirements` lists IDs or ranges (`RS-016…RS-019`); `Method` is `A`, `I`, or `A, I`; `Check` names the check or review in a few words. The trace matrix shows `Method: Check (path)` as the artefact.
- The file is named after the first ID of `Requirements`, and a requirement has at most one record. To add a requirement to an existing check, extend that record; to give it its own, move it to a new file.
- `cargo xtask trace` fails on a record that breaks these rules; `cargo xtask ci` and the verify check `spec` run it.

GEN-022 and GEN-023 are traced by [`records/GEN-022.md`](records/GEN-022.md).
