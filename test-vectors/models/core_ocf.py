"""Independent model of the Operational Control Field classification for the vector file
test-vectors/core/ocf.json (CMN-010, CMN-011; M2 WP-1).

Sources: CCSDS 132.0-B-3 §4.1.5.4-5, 732.0-B-5 §4.1.5.4-5, and 732.1-B-3 §4.1.5.2.1-2 (the
same classification for TM, AOS, and USLP); the CLCW layout of 232.0-B-4 §4.2.1 for one
assembled input. Parameters, not text: bit 0 (the most significant bit of the first octet) is
the Type Flag; 0 is a Type-1 Report; 1 is a Type-2 Report, whose bit 1 is 0 for a
project-specific report and 1 for an SDLS FSR. Error cases name the field codes of
docs/03-api-model.md §9.1: 65 `ocf_type_flag`, 66 `ocf_report_use`.

The model was written from the Blue Book clauses, not from the Rust code
(crates/aldis-core/src/ocf.rs), and must stay independent of it: never port code from it, and
never fix the model to agree with it.

Usage, from the repository root:

    python3 -I test-vectors/models/core_ocf.py --check test-vectors/core/ocf.json
    python3 -I test-vectors/models/core_ocf.py --write test-vectors/core/ocf.json

--check recomputes the expected outcome of every live case from its operation, parameters,
and input, compares the cases the generator writes with the file's, and checks the two field
codes against test-vectors/core/fields.json; it exits 1 on any difference. --write
regenerates the file (cases and provenance note).
"""
import json
import os
import sys

FIELD_TYPE_FLAG = 65  # ocf_type_flag, 03 §9.1
FIELD_REPORT_USE = 66  # ocf_report_use, 03 §9.1
FIELDS = {FIELD_TYPE_FLAG: "ocf_type_flag", FIELD_REPORT_USE: "ocf_report_use"}


def bits(octets):
    word = int.from_bytes(octets, "big")
    # Bit 0 is the most significant bit of the 32-bit field (CCSDS bit numbering).
    return [(word >> (31 - i)) & 1 for i in range(32)]


def classify(octets):
    b = bits(octets)
    if b[0] == 0:
        return "clcw"  # Type-1 Report
    if b[1] == 0:
        return "project_specific"  # Type-2, bit 1 = 0
    return "sdls_fsr"  # Type-2, bit 1 = 1


def construct(report, octets):
    """The checked constructor of one report type: None, or (field, expected, found)."""
    b = bits(octets)
    if report == "clcw":
        return None if b[0] == 0 else (FIELD_TYPE_FLAG, 0, 1)
    if b[0] != 1:
        return (FIELD_TYPE_FLAG, 1, 0)
    want = 0 if report == "project_specific" else 1
    return None if b[1] == want else (FIELD_REPORT_USE, want, b[1])


def clcw(cop, vcid, flags, farm_b, report_value, status=0, version=0, spare14=0, spare23=0):
    """A CLCW from the field widths of 232.0-B-4 §4.2.1, bit 0 first."""
    fields = [(0, 1), (version, 2), (status, 3), (cop, 2), (vcid, 6), (spare14, 2),
              (flags, 5), (farm_b, 2), (spare23, 1), (report_value, 8)]
    word = 0
    for value, width in fields:
        assert 0 <= value < (1 << width)
        word = (word << width) | value
    return word.to_bytes(4, "big")


def expect(case):
    """The expected outcome of a case, from the model alone."""
    octets = bytes.fromhex(case.get("input_hex", ""))
    if len(octets) != 4:
        raise ValueError(f"{case['id']}: an OCF has four octets")
    if case["operation"] == "ocf_classify":
        return {"ok": True, "report": classify(octets), "octets_hex": octets.hex().upper()}
    if case["operation"] == "ocf_report":
        err = construct(case["params"]["report"], octets)
        if err is None:
            return {"ok": True, "octets_hex": octets.hex().upper()}
        return {"ok": False, "error": "InvalidField", "field": err[0]}
    raise ValueError(f"{case['id']}: the model has no operation {case['operation']!r}")


# ---- the generator: how the inputs of the vector file were chosen

CLCW = clcw(cop=0b01, vcid=5, flags=0b00001, farm_b=0b10, report_value=42)

CLASSIFY = [
    ("ocf-classify-001", "CLCW of 232.0 figure 4-6: COP-1, VCID 5, Retransmit, FARM-B 2, N(R) 42",
     CLCW),
    ("ocf-classify-002", "All zeros: Type Flag 0, a Type-1 Report", bytes(4)),
    ("ocf-classify-003",
     "Type Flag 0 and every other bit 1: still a Type-1 Report; core does not check the CLCW "
     "(CMN-011)", bytes.fromhex("7FFFFFFF")),
    ("ocf-classify-004", "Type Flag 0 and bit 1 set: bit 1 belongs to the Type-1 Report",
     bytes.fromhex("40000000")),
    ("ocf-classify-005", "Bits 0-1 = 10, the rest zero: project-specific Type-2 Report",
     bytes.fromhex("80000000")),
    ("ocf-classify-006", "Bits 0-1 = 10, the rest ones: project-specific Type-2 Report",
     bytes.fromhex("BFFFFFFF")),
    ("ocf-classify-007", "Bits 0-1 = 11, the rest zero: SDLS FSR Type-2 Report",
     bytes.fromhex("C0000000")),
    ("ocf-classify-008", "All ones: SDLS FSR Type-2 Report", bytes.fromhex("FFFFFFFF")),
    ("ocf-classify-009", "Bits 0-1 = 10 with a mixed project payload",
     bytes.fromhex("A55A00FF")),
    ("ocf-classify-010", "Bits 0-1 = 11 with a mixed FSR payload", bytes.fromhex("E0010203")),
]

REPORTS = [
    ("ocf-report-001", "clcw", "A CLCW is a Type-1 Report", CLCW),
    ("ocf-report-002", "clcw", "Type Flag 1 is not a Type-1 Report", bytes.fromhex("80000000")),
    ("ocf-report-003", "clcw", "An SDLS FSR is not a Type-1 Report", bytes.fromhex("C1234567")),
    ("ocf-report-004", "project_specific", "Bits 0-1 = 10: project-specific",
     bytes.fromhex("80123456")),
    ("ocf-report-005", "project_specific", "Type Flag 0 is not a Type-2 Report", CLCW),
    ("ocf-report-006", "project_specific",
     "Type Flag 0 with bit 1 set: the Type Flag is reported", bytes.fromhex("40000000")),
    ("ocf-report-007", "project_specific", "Bit 1 = 1 is an SDLS FSR, not project-specific",
     bytes.fromhex("C1234567")),
    ("ocf-report-008", "sdls_fsr", "Bits 0-1 = 11: SDLS FSR", bytes.fromhex("C1234567")),
    ("ocf-report-009", "sdls_fsr", "Type Flag 0 is not a Type-2 Report",
     bytes.fromhex("40000000")),
    ("ocf-report-010", "sdls_fsr", "Bit 1 = 0 is project-specific, not an SDLS FSR",
     bytes.fromhex("80123456")),
]

NOTE = (
    "No frame Blue Book prints an example OCF. Classification by bits 0-1 (bit 0 the most "
    "significant bit of the first octet): Type Flag 0 is a Type-1 Report (CLCW, or CLCW/PLCW "
    "on USLP); Type Flag 1 with bit 1 = 0 is project-specific; with bit 1 = 1 an SDLS FSR. "
    "TM, AOS Issues 4 and 5, and USLP give the same table and no reserved value. The CLCW "
    "of ocf-classify-001 is assembled field by field from 232.0-B-4 figure 4-6 (Control "
    "Word Type 0, Version 00, Status 000, COP in Effect 01, VCID 000101, spare 00, Flags "
    "00001, FARM-B Counter 10, spare 0, Report Value 0x2A). Expected values computed by an "
    "independent Python model of those clauses, which shares no code with the Rust "
    "implementation. Error cases "
    "name the field code (03 §9.1): 65 ocf_type_flag, 66 ocf_report_use. The model is "
    "committed as test-vectors/models/core_ocf.py (TV-023); "
    "`python3 -I test-vectors/models/core_ocf.py --check test-vectors/core/ocf.json` "
    "recomputes every expected value. Author of the model: an AI agent, Claude Opus 5.5, "
    "recorded as co-author of commit 222232e (2026-10-21), which committed it, and of commit "
    "7f8ecda, which added the vectors; the history records no briefing of the agent and does "
    "not show whether it read any implementation (TV-023)."
)


def generate():
    cases = []
    for cid, desc, octets in CLASSIFY:
        c = {"id": cid,
             "requirements": ["CMN-010", "CMN-011"] if cid == "ocf-classify-003" else ["CMN-010"],
             "description": desc, "operation": "ocf_classify", "input_hex": octets.hex().upper()}
        c["expect"] = expect(c)
        cases.append(c)
    for cid, report, desc, octets in REPORTS:
        c = {"id": cid, "requirements": ["CMN-010"], "description": desc,
             "operation": "ocf_report", "params": {"report": report},
             "input_hex": octets.hex().upper()}
        c["expect"] = expect(c)
        cases.append(c)
    return {
        "kind": "codec",
        "standard": "132.0-B-3 §4.1.5.4-5 / 732.0-B-5 §4.1.5.4-5 / 732.1-B-3 §4.1.5.2.1-2; "
                    "CLCW layout 232.0-B-4 §4.2.1",
        "provenance": "hand-derived",
        "provenance_note": NOTE,
        "cases": cases,
    }


def check_field_codes(path):
    """The field codes the model names, against the generated catalogue next to the file."""
    catalogue = os.path.join(os.path.dirname(path), "fields.json")
    if not os.path.exists(catalogue):
        return 0
    with open(catalogue, encoding="utf-8") as f:
        names = {e["code"]: e["name"] for e in json.load(f)["fields"]}
    bad = 0
    for code, name in FIELDS.items():
        if names.get(code) != name:
            print(f"{catalogue}: code {code} is {names.get(code)!r}, the model expects {name!r}")
            bad += 1
    return bad


def check(paths):
    generated = {c["id"]: c for c in generate()["cases"]}
    bad = 0
    for path in paths:
        with open(path, encoding="utf-8") as f:
            document = json.load(f)
        bad += check_field_codes(path)
        live = [c for c in document["cases"] if "superseded" not in c]
        for c in live:
            try:
                want = expect(c)
            except (KeyError, ValueError) as e:
                print(f"{path}: {c['id']}: {e}")
                bad += 1
                continue
            if c["expect"] != want:
                print(f"{path}: {c['id']}: expect {c['expect']}, the model gives {want}")
                bad += 1
            if c["id"] in generated and generated[c["id"]] != c:
                print(f"{path}: {c['id']} differs from the generator (--write)")
                bad += 1
        ids = {c["id"] for c in document["cases"]}
        for cid in generated:
            if cid not in ids:
                print(f"{path}: {cid}, written by the generator, is missing")
                bad += 1
        print(f"{path}: {len(live)} case(s) checked against the model")
    return bad


def main(argv):
    if len(argv) >= 2 and argv[0] == "--check":
        bad = check(argv[1:])
        if bad:
            print(f"core_ocf: {bad} difference(s)")
            return 1
        return 0
    if len(argv) == 2 and argv[0] == "--write":
        with open(argv[1], "w", encoding="utf-8") as f:
            json.dump(generate(), f, indent=2, ensure_ascii=False)
            f.write("\n")
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
