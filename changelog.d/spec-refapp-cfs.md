### Added
- Reference application: a cFS flight side, `docs/reference-application.md` §10, with
  APP-110…APP-125: cFS apps on cFE, OSAL, and the pc-linux PSP that call Aldis through
  `include/aldis/aldis.h`, run in lockstep with the harness, and reproduce the Rust flight side's
  golden traces in four phases tied to the C ABI (packets, frames, COP-1, engines).
- Plan `docs/milestones/acfs-flight-side.md` (milestone ACFS): the cFS facts read at
  nasa/cFS `v7.0.1`, decisions D1–D19 (accepted), work packages, exit criteria, and risks;
  the ACFS row and phases in `docs/08-roadmap.md` §3.
- Open issues OI-36 (cFE TIME from simulated time), OI-37 (trace timing and cFS time), and OI-38
  (node protocol transport and timing): OI-37 and OI-38 resolved, OI-36 deferred.
- Glossary: cFS, cFE, OSAL, PSP, SB, SCH and SCH_LAB, CI_LAB, TO_LAB, MET, and STCF.
