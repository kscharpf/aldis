# CCSDS reference documents

Aldis implements standards published by the **Consultative Committee for Space Data Systems (CCSDS)**. The CCSDS documents are © CCSDS and are distributed by CCSDS at <https://ccsds.org/publications/>. **They are not included in this repository.** Download them locally with the steps below. The `specs/pdf/` folder is git-ignored.

## Download

```sh
specs/fetch.sh                 # standards implemented in v1 (default)
specs/fetch.sh --group ref     # other standards cited by the specification
specs/fetch.sh --all           # everything in the manifest
specs/fetch.sh --verify-only   # check existing files without downloading
```

The script reads [`manifest.tsv`](manifest.tsv), downloads each document into `specs/pdf/`, and verifies its SHA-256 checksum. It needs `bash`, `curl`, and `sha256sum` or `shasum`.

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

Not in the manifest: **CCSDS 732.0-B-4** (superseded AOS Issue 4), which is needed for open issue OI-04. Get it from the CCSDS Silver Books (historical documents): <https://ccsds.org/publications/silverbooks/>.

## Rules

- Never commit CCSDS PDFs or extracted text from them. CI SHALL NOT require them: tests use the [test vectors](../test-vectors/README.md).
- Quote the standards by document number and clause (e.g. `133.0 §4.1.3.5.2`), not by copying text.
