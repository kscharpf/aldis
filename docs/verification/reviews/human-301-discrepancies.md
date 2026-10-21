# Human review: four time-code readings (CCSDS 301.0-B-4)

**Status:** in progress (Q1 reading recorded; exercises and Q2–Q4 open) · **Reviewer:** the maintainer · **Prepared:** 2026-10-21

## Purpose

The `spacepackets` cross-check ([cross-checks.md](../cross-checks.md)) disagrees with Aldis on four points of CCSDS 301.0-B-4. Each was decided by an agent reading the Blue Book. The same family of models wrote Aldis's requirements, its vectors, and its code, so a misreading would look consistent across all three. This review asks a person to read the Blue Book and answer the same four questions independently.

## How to do this review

1. Get the Blue Book: `specs/fetch.sh` downloads CCSDS 301.0-B-4 to `specs/pdf/301x0b4e1.pdf` ([specs/README.md](../../../specs/README.md)). Note any corrigendum or later edition on the CCSDS site in the sign-off.
2. **Read and answer each question from the Blue Book alone, before you open any Aldis document about it.** Don't read [cross-checks.md](../cross-checks.md) §3 or [time-codes.md](../../standards/time-codes.md), which holds the TIME requirements, until all four questions are answered. Their reasoning would steer yours.
3. Work each exercise by hand, then compare your result with the named vector cases in [`test-vectors/time/`](../../../test-vectors/time/). Each vector case has an `id`, its input (`input_hex` or `params`), and an `expect` object.
4. Only then read the matching entry of cross-checks.md §3 (D1–D4), and record whether you agree with Aldis, with `spacepackets`, with neither, or are unsure.
5. Write in your own words, citing clauses by number. Don't paste Blue Book text into this file (AGENTS.md: the CCSDS documents are not redistributed here).

Record every reading, including "agrees". A reading you would have made differently is worth noting even if Aldis's reading is defensible, because those are the points for a later expert to check. If a finding disagrees with Aldis, say so in the record; the coordinator then opens an issue and a fix, and the vectors change only by a new case with the old one marked `superseded` ([07-test-vectors.md](../../07-test-vectors.md)).

## Q1. What is a CUC fine time worth?

**Read:** 301.0 §3.2.1 and §3.2.2, and anything else in 301.0 you find relevant.

**Question:** A CUC T-field has n octets of fractional time holding the unsigned integer F. What fraction of the basic time unit (the second) does F represent? Is that fixed by the Blue Book, or open to interpretation?

**Exercises** (operation `cuc_to_ns` converts to whole seconds and nanoseconds, dropping any fraction of a nanosecond):

| Case | Input | Your result |
|---|---|---|
| `time-cuc-015` | coarse 7, fine octets `80` (n = 1) | |
| `time-cuc-016` | coarse 0, fine octets `FF FF FF` (n = 3) | |

**Record**

- Your reading, with clauses: F is a binary fraction of a unit that the metadata defines (§3.2.1, §3.2.2). (2026-10-21)
- Fixed by 301.0, or left open: the form of the fraction is fixed (binary, octet by octet); the basic time unit is left to the metadata.
- Vector cases agree with your results (yes / no, which): exercises not yet done.
- After reading cross-checks.md §3 D1 — agree with Aldis / `spacepackets` / neither / unsure: not yet compared.
- Notes:
  - **First reading** (2026-10-21): fine time as segments of milliseconds of the second, microseconds of the millisecond, and nanoseconds of the microsecond, each finer one requiring the one before. This came from Annex E1, which is PB-5J, an informative example of an agency-defined segmented code (time code ID `110`), not from §3.2. A reader looking up "fine time" can land there.
  - **A reading considered and set aside:** that 301.0 is agnostic about the fractional unit, which might be defined by the mission-definition bits 6–7 of CUC P-field octet 2. §3.2.2 only reserves those bits for mission definition.
  - **Primed:** the reading above was settled after a discussion with the coordinator (an AI), which pointed to the "binary fraction" wording of §3.2.1 and §1.2. The first reading was not primed.
  - **Finding:** Aldis's conversion of a CUC time to seconds and nanoseconds (TIME-011) assumes that the basic time unit is the second. For a Level 2 code whose agency defines another unit, that conversion does not apply. Raised as #82.

## Q2. CDS sub-millisecond code `11`

**Read:** 301.0 §3.3.1 and §3.3.2 (bits 6–7 of the CDS P-field).

**Question:** What does the Blue Book assign to code `11`? Does it give a length for the sub-millisecond segment with that code? On receiving it, what should a decoder do: reject the time code, decode it as if there were no sub-millisecond segment, or something else? Is that the Blue Book's answer or a decoder's choice? Aldis has a strict and a lenient decoding mode: should the two differ here?

**Exercise:** decode the P-field octet of input `43 00 00 00 00 00 00` bit by bit (time code, epoch, day segment length, sub-millisecond code), and say what you would return.

| Case | Mode | Your result |
|---|---|---|
| `time-cds-004` | strict | |
| `time-cds-005` | lenient | |

**Record**

- Your reading, with clauses:
- Fixed by 301.0, or left open:
- Vector cases agree with your results (yes / no, which):
- After reading cross-checks.md §3 D2 — agree with Aldis / `spacepackets` / neither / unsure:
- Notes:

## Q3. The P-field extension flag

**Read:** 301.0 §3.1.1 (bit 0 of the P-field) and §3.2.2 (CUC octet 2). Read §3.1.1 in full, including what it says about the present value of the flag.

**Question:** When bit 0 of P-field octet 1 is `1`, what follows? For CUC, what does octet 2 carry, and how do its counts combine with those of octet 1? Should a decoder accept a two-octet P-field, given what §3.1.1 says about the flag's present value? Does a decoder that reads only octet 1 and treats the next octet as the T-field conform?

**Exercises:** decode each P-field (time code, epoch, coarse octets, fine octets, mission-defined bits, P-field length), then split the rest into coarse and fine T-field octets.

| Case | Input | Your result |
|---|---|---|
| `time-cuc-003` (in `cuc.json`) | `9C 20 01 02 03 04 05` | |
| `time-pf-003` (in `pfield.json`) | `9F 7E 01 02 … 11` (19 octets in all, `01` to `11` hex) | |

**Record**

- Your reading, with clauses:
- Fixed by 301.0, or left open:
- Vector cases agree with your results (yes / no, which):
- After reading cross-checks.md §3 D3 — agree with Aldis / `spacepackets` / neither / unsure:
- Notes:

## Q4. Converting nanoseconds to CUC fine time

**Read:** 301.0 §3.2.1 again, and anything in 301.0 on resolution or rounding.

**Question:** An encoder is given a time in whole seconds and nanoseconds and must write it with n fine octets. When the time is not a multiple of 2^(−8n) s, does 301.0 say how to round? If it does, how? If it does not, the choice is a project rule, so judge it as one: which rule would you choose (down, to nearest, up), and why? What should happen when rounding carries into the coarse time, and when the carry overflows the coarse octets?

**Exercises** (operation `cuc_from_ns`, 1 coarse octet and 1 fine octet in each):

| Case | Input | Your result |
|---|---|---|
| `time-cuc-020` | 0 s, 1 ns | |
| `time-cuc-022` | 5 s, 999 999 999 ns | |
| `time-cuc-023` | 255 s, 999 999 999 ns | |

**Record**

- Your reading, with clauses:
- Fixed by 301.0, or left open:
- If left open, the rule you would choose, and whether Aldis's (after you read it) is reasonable:
- Vector cases agree with your results (yes / no, which):
- After reading cross-checks.md §3 D4 — agree with Aldis / `spacepackets` / neither / unsure:
- Notes:

## Sign-off

- Reviewer:
- Date(s):
- Document read (edition, and corrigenda checked on the CCSDS site):
- Time spent:
- Had you seen any AI's answers to these questions before writing yours (an automated review of the pull request that added this packet, PR #80, works all four)? Which questions:
- Findings that need a change (none, or the questions):
- Questions to pass to a CCSDS expert:
