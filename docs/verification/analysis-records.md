# Analysis and inspection records

Requirements whose verification method is **A** (analysis, usually a check of the verification run, `cargo xtask verify`) or **I** (inspection) are traced by a **record**: one file per record in [`records/`](records/), named after the first requirement ID it covers, for example [`records/VER-024.md`](records/VER-024.md). Tests and vectors carry their own annotations. `cargo xtask trace` reads every file in `records/` (VER-020). There is no index to update: the directory is the list.

List a requirement only when the artefact actually checks it. Delete the record if the check is removed.

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
