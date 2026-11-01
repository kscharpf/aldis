# CCSDS reference documents

Aldis implements standards published by the **Consultative Committee for Space Data Systems (CCSDS)**. The CCSDS documents are © CCSDS and are distributed by CCSDS at <https://ccsds.org/publications/>. **They are not included in this repository.** Download them locally with the steps below. The `specs/pdf/` folder is git-ignored.

## Download

```sh
specs/fetch.sh                 # standards implemented in v1 (default)
specs/fetch.sh --group ref     # other standards cited by the specification
specs/fetch.sh --all           # everything in the manifest
specs/fetch.sh --verify-only   # check existing files without downloading
```

In a git worktree, the script first links `specs/pdf` to the main checkout's `specs/pdf` when that holds PDFs (a copy if linking fails), then verifies them. The script reads [`manifest.tsv`](manifest.tsv), downloads each document into `specs/pdf/`, and verifies its SHA-256 checksum. It needs `bash`, `curl`, and `sha256sum` or `shasum`.

## If a download fails or a checksum changes

CCSDS re-uploads documents when it issues editorial changes or corrigenda, so the URLs and checksums in the manifest can go stale.

1. Open the CCSDS Blue Books page: <https://ccsds.org/publications/bluebooks/>.
2. Find the document by number (e.g. `CCSDS 133.0-B-2`) and download the PDF.
3. Save it as `specs/pdf/<file>` using the file name from the manifest, then run `specs/fetch.sh --verify-only`.
4. If the checksum still differs, CCSDS has published a different edition. Check the document's change history. If the change affects wire behavior, record it in the specification ([`docs/`](../docs/README.md)). Then update the URL and checksum in `manifest.tsv` in the same change.

## Documents

| Group | Document | Title | Local file |
|---|---|---|---|
| v1 | CCSDS 133.0-B-2 | Space Packet Protocol | `specs/pdf/133x0b2e2.pdf` |
| v1 | CCSDS 133.1-B-3 | Encapsulation Packet Protocol | `specs/pdf/133x1b3e1.pdf` |
| v1 | CCSDS 132.0-B-3 | TM Space Data Link Protocol | `specs/pdf/132x0b3.pdf` |
| v1 | CCSDS 232.0-B-4 (+ Cor. 1) | TC Space Data Link Protocol | `specs/pdf/232x0b4e1c1.pdf`, `specs/pdf/232x0b4e1c1_tc2325.pdf` |
| v1 | CCSDS 232.1-B-2 (+ Cor. 1) | Communications Operation Procedure-1 | `specs/pdf/232x1b2e2c1.pdf`, `specs/pdf/232x1b2e2c1_tc1911.pdf` |
| v1 | CCSDS 732.0-B-5 | AOS Space Data Link Protocol | `specs/pdf/732x0b5ec1.pdf` |
| v1 | CCSDS 732.1-B-3 | Unified Space Data Link Protocol | `specs/pdf/732x1b3e1.pdf` |
| v1 | CCSDS 301.0-B-4 | Time Code Formats | `specs/pdf/301x0b4e1.pdf` |
| ref | CCSDS 355.0-B-2 | Space Data Link Security Protocol (SDLS hooks) | `specs/pdf/355x0b2.pdf` |
| ref | CCSDS 131.0-B-6 | TM Synchronization and Channel Coding | `specs/pdf/131x0b6ec1.pdf` |
| ref | CCSDS 231.0-B-4 | TC Synchronization and Channel Coding | `specs/pdf/231x0b4e1c1.pdf` |
| ref | CCSDS 732.0-B-4 (Silver Book) | AOS Space Data Link Protocol, superseded Issue 4, for `Issue4Compatible` (OI-04, AOS-002, AOS-003) | `specs/pdf/732x0b4s.pdf` |
| ref | CCSDS 700.1-G-1 (Green Book) | Overview of the Unified Space Data Link Protocol, informational, confirming the TFDF header length (OI-05, USLP-005) | `specs/pdf/700x1g1.pdf` |

CCSDS 732.0-B-4 is a superseded issue, published with the other historical documents in the CCSDS Silver Books (<https://ccsds.org/publications/silverbooks/>). CCSDS 700.1-G-1 is an informational report (<https://ccsds.org/publications/greenbooks/>), written for 732.1-B-1: it explains the Blue Book and never overrides it.

## Rules

- To read a clause as text, run `pdftotext -layout -f <page> -l <page> specs/pdf/<file> -` (poppler; `brew install poppler`).
- Never commit CCSDS PDFs or extracted text from them. CI SHALL NOT require them: tests use the [test vectors](../test-vectors/README.md).
- Quote the standards by document number and clause (e.g. `133.0 §4.1.3.5.2`), not by copying text.

## SANA registry snapshots

Protocol values that a registry of the Space Assigned Numbers Authority (SANA) assigns are never written from memory (OI-06). [`sana/`](sana/) holds a committed snapshot of each registry the code needs: the rows of the registry's public table, the URL they were read from, the retrieval date, and the SHA-256 of the rows (M2 plan D13). Unlike the PDFs, the snapshots are committed: they are registry data, not CCSDS text.

| Registry | Snapshot | Generated from it |
|---|---|---|
| `uslp_protocol_id`, USLP Protocol Identifier (732.1 reference [14]) | [`sana/uslp_protocol_id.tsv`](sana/uslp_protocol_id.tsv) | `crates/aldis-uslp/src/upid/table.rs` (USLP-007) |

```sh
cargo xtask sana import uslp_protocol_id   # download, write the snapshot and the table
cargo xtask sana                           # offline: rows match the SHA-256, table matches (cargo xtask ci)
cargo xtask sana check-live                # the live registry still matches (nightly job sana-registries)
```

SANA publishes no machine-readable export without an account (checked 2026-10-23: `/api/` asks for a login, and the registry page links no CSV, JSON, or XML), so the import reads the HTML table of the public registry page and checks the row count it states. The registry pages state no terms of use.
