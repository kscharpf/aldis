# Human review: four time-code readings (CCSDS 301.0-B-4)

**Status:** in progress (all four readings recorded; Q4 differs from Aldis, OI-40; Q3's question is deferred, OI-39; the comparisons with the vectors and with §3 D1–D4 are open) · **Reviewer:** the maintainer · **Prepared:** 2026-10-21

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
| `time-cds-004` | strict | reject |
| `time-cds-005` | lenient | reject |

**Record**

- Your reading, with clauses: reject code `11`. §3.3.2 reserves it for future use, so a sender using it means either the specification has been updated or the application must be, and a decoder should not guess its meaning. Reject it in lenient mode as well: the code sets the number of sub-millisecond octets, so without knowing it the decoder cannot tell where the T-field ends, and its handling of every later octet could be off. (2026-10-21)
- Fixed by 301.0, or left open: the code is reserved by §3.3.2; what a decoder does with it is the reading above.
- Vector cases agree with your results (yes / no, which): results above, from the reading (2026-10-21); not yet compared with the vector file.
- After reading cross-checks.md §3 D2 — agree with Aldis / `spacepackets` / neither / unsure: not yet compared.
- Notes: the coordinator (an AI) did not discuss Q2 before this reading was given. The reviewer did not read the automated review of PR #80, which answers Q2.

## Q3. The P-field extension flag

**Read:** 301.0 §3.1.1 (bit 0 of the P-field) and §3.2.2 (CUC octet 2). Read §3.1.1 in full, including what it says about the present value of the flag.

**Question:** When bit 0 of P-field octet 1 is `1`, what follows? For CUC, what does octet 2 carry, and how do its counts combine with those of octet 1? Should a decoder accept a two-octet P-field, given what §3.1.1 says about the flag's present value? Does a decoder that reads only octet 1 and treats the next octet as the T-field conform?

**Exercises:** decode each P-field (time code, epoch, coarse octets, fine octets, mission-defined bits, P-field length), then split the rest into coarse and fine T-field octets.

| Case | Input | Your result |
|---|---|---|
| `time-cuc-003` (in `cuc.json`) | `9C 20 01 02 03 04 05` | first reading: reject, P-field bit 0 is set; revised: as Aldis decodes it |
| `time-pf-003` (in `pfield.json`) | `9F 7E 01 02 … 11` (19 octets in all, `01` to `11` hex) | first reading: reject, P-field bit 0 is set; revised: as Aldis decodes it |

**Record**

- Your reading, with clauses: **revised** (2026-10-21): accept Aldis's current behaviour, which reads CUC octet 2 (§3.2.2), because the first reading below would change requirements and test vectors on an ambiguous text. The question is tracked in [#84](https://github.com/kscharpf/aldis/issues/84), for a real system that needs one reading or the other to weigh in. **First reading:** bit 0 of the P-field is the extension flag (§3.1.1), and §3.1.1 says its value is at present `0`. Take §3.1.1 as authoritative and §3.2.2's octet 2 as an extension that has not been formalized. A P-field with bit 0 set is therefore ambiguous, and the safe behaviour in the face of ambiguity is to reject the time code. Only the one P-field octet is accepted, and the time units are read from it as specified. (2026-10-21)
- Fixed by 301.0, or left open: left open; the two clauses read against each other.
- Vector cases agree with your results (yes / no, which): results above, from the reading; not yet compared with the vector files.
- After reading cross-checks.md §3 D3 — agree with Aldis / `spacepackets` / neither / unsure: not yet compared.
- Notes:
  - **The first reading disagreed with Aldis**, which accepts CUC octet 2 (TIME-010) and decodes both exercise inputs. Raised as [OI-39](../../09-open-issues.md), now deferred with Aldis's behaviour kept; the revised reading agrees with Aldis.
  - **The revision was primed** by the coordinator's write-up of OI-39, which recommended keeping octet 2 and listed what the first reading would change.
  - **Partly primed:** the first answer ("accept only the one P-field octet") left open whether a set flag means reject or ignore the flag. The coordinator (an AI) asked which, and asked how the reading fits §3.2.2. The reading above came after those questions and kept the first answer's direction.
  - **For an expert:** whether §3.1.1's "at present `0`" or §3.2.2's definition of octet 2 governs. This is the question of this review best suited to a CCSDS expert.

## Q4. Converting nanoseconds to CUC fine time

**Read:** 301.0 §3.2.1 again, and anything in 301.0 on resolution or rounding.

**Question:** An encoder is given a time in whole seconds and nanoseconds and must write it with n fine octets. When the time is not a multiple of 2^(−8n) s, does 301.0 say how to round? If it does, how? If it does not, the choice is a project rule, so judge it as one: which rule would you choose (down, to nearest, up), and why? What should happen when rounding carries into the coarse time, and when the carry overflows the coarse octets?

**Exercises** (operation `cuc_from_ns`, 1 coarse octet and 1 fine octet in each):

| Case | Input | Your result |
|---|---|---|
| `time-cuc-020` | 0 s, 1 ns | coarse 0, fine `00` |
| `time-cuc-022` | 5 s, 999 999 999 ns | coarse 5, fine `FF` |
| `time-cuc-023` | 255 s, 999 999 999 ns | coarse 255, fine `FF` (fits; no carry) |

**Record**

- Your reading, with clauses: truncate (round down) the nanoseconds. A hardware provider with an explicit clock at 2^N Hz provides an exact binary fraction, so the conversion from nanoseconds is not the source of the time code's precision. (2026-10-21)
- Fixed by 301.0, or left open: left open (no rounding rule is given); the choice is a project rule (TIME-011).
- If left open, the rule you would choose, and whether Aldis's (after you read it) is reasonable: round down. Aldis's rule not yet read.
- Vector cases agree with your results (yes / no, which): the results above follow from round-down and were computed by the coordinator (an AI) from the reviewer's rule, not worked by hand; not yet compared with the vector file.
- After reading cross-checks.md §3 D4 — agree with Aldis / `spacepackets` / neither / unsure: not yet compared.
- Notes:
  - **Differs from Aldis**, which rounds up (TIME-011). Under round-down the carry into the coarse time (`time-cuc-022`) and the overflow (`time-cuc-023`) cannot happen. Raised as [OI-40](../../09-open-issues.md); Aldis keeps its behaviour until OI-40 is decided.
  - Not primed: the coordinator did not discuss Q4 before this reading was given, and the reviewer did not read the automated review of PR #80, which answers it.

## Sign-off

- Reviewer:
- Date(s):
- Document read (edition, and corrigenda checked on the CCSDS site):
- Time spent:
- Had you seen any AI's answers to these questions before writing yours (an automated review of the pull request that added this packet, PR #80, works all four)? Which questions: no automated review was read. The coordinator's discussion primed Q1's final reading and, in part, Q3's (see their notes); Q2 and Q4 were not primed. (2026-10-21)
- Findings that need a change (none, or the questions):
- Questions to pass to a CCSDS expert: Q3 (OI-39, #84). Q4 (OI-40) is a project rule, so it needs no expert.
