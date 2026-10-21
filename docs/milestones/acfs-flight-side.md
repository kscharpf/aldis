# Milestone ACFS — cFS flight side

**Goal:** a second flight node for the reference application, built from NASA Core Flight System (cFS) applications that call Aldis through the C ABI (`include/aldis/aldis.h`), and that reproduces the golden traces of the Rust flight node byte for byte, phase by phase as the C ABI grows: packets (A1), frames (A2), COP-1 (A3), and the link engines (A4). It shows Aldis integrated the way most integrators will use it, inside a flight framework with heritage, and gives interoperability evidence for cFS.
**Roadmap entry:** [08-roadmap.md](../08-roadmap.md) §3 (track A) · **Builds on:** A1–A4 and the C ABI of M2–M4 (§4) · **Specification:** [reference-application.md](../reference-application.md) §10 (APP-110…APP-125) · **Track:** A, owns `refapp/`; the code goes in `refapp/cfs/` (D2)

**Status: plan approved (2026-10-21).** Decisions D1–D19 accepted by the maintainer 2026-10-21 (#78), as recommended. Entry gate of P1, the first phase: A1 exited (not yet) and the M1 codecs' C ABI (M2 WP-0c, merged in #72). How many of the §4 requirements are traced is reported by `cargo xtask trace --scope acfs`, not quoted here.

| WP | State | PRs | Notes |
|---|---|---|---|
| WP-0 Plan and specification | in review | — | Branch `spec/refapp-cfs`: this plan, [reference-application.md](../reference-application.md) §10, APP-110…APP-125, OI-36…OI-38, roadmap §3 row, glossary |
| WP-1 cFS build skeleton | planned | — | Decisions D2–D7, D11, D12 accepted |
| WP-2 Harness: cFS node and TCP transport | planned | — | Decisions D9, D18 accepted |
| WP-3 Tooling: C header, trace scan, verify inputs | planned | — | Decisions D12, D15, D17 accepted |
| WP-4 P1 Packets | planned | — | Entry: A1 exited; WP-1 to WP-3 |
| WP-5 P2 Frames | planned | — | Entry: A2 exited; the M2 frame C ABI |
| WP-6 P3 COP-1 | planned | — | Entry: A3 exited; the M3 FARM-1 handle |
| WP-7 P4 Engines | planned | — | Entry: A4 exited; the M4 engine handles |
| WP-8 Review and exit | planned | — | |

A pull request that starts or merges a work package changes only its row ([roadmap](../08-roadmap.md) §6 rule 1).

History:
- 2026-10-21: plan drafted on branch `spec/refapp-cfs`. The cFS facts of §1.1 were read from the cFS repositories at `v7.0.1`. Opened OI-36 (cFE TIME from simulated time), OI-37 (trace timing and cFS time), and OI-38 (node protocol transport and timing).

## 1. Starting point

| In place | Missing for ACFS |
|---|---|
| The scenario, cases, golden-trace format, and node protocol ([reference-application.md](../reference-application.md) §2–§8) | The cFS flight node, its build, and its nightly job |
| The M1 codecs' C ABI: `aldis_time_*`, `aldis_spp_*` (with the tracker, segmenter, and reassembler in caller storage), `aldis_epp_*`; the generated `include/aldis/aldis.h`, valid C99 (CABI-003); `libaldis_ffi.a` for every target (CABI-001) (M2 WP-0c, #72) | The frame C ABI (M2 WP-1 to WP-3), the FARM-1 handle (M3), and the engine handles (M4) |
| `refapp-harness`: scheduler, link model, trace writer, comparator; `a0-empty` reproduced | The node protocol (§8.2), planned for A5; a TCP transport; a launcher for `core-cpu1` |
| `refapp/scenario/scenario.json` with generated Rust constants (APP-107) | A generated C header (planned for A5 as a C++ header) |
| `cargo xtask trace` scans `.rs` files in `refapp/` and golden-trace headers | A scan of the C sources in `refapp/cfs/` (APP-125) |
| The cFE `MSG` cross-check (track R), being written on branch `m2/crosscheck-cfe`; not pushed when this plan was written | One cFS pin shared by both (D4) |

### 1.1 cFS facts

Read on 2026-10-21 from the repositories, through the GitHub API and the source trees at the tags below, not from memory. Paths are relative to each repository's root. Line numbers are those at the pinned commit.

| # | Fact | Repository, tag (commit), path |
|---|---|---|
| F1 | The latest cFS bundle release is `v7.0.1` (published 2026-05-14; the one before is `v7.0.0`, "Draco", 2026-01-26). Its submodules pin cFE, OSAL, PSP, CI_LAB, TO_LAB, and SCH_LAB at the commits of each component's own `v7.0.1` tag: cFE `c5fb2b4d540bd55eb6c3707da7dd13eee679d4dd`, OSAL `d2d877a69cff47452bcca274b309147d48e6c16f`, PSP `c4b3b0b65b119e106481ad8e20976ae4d7f554e3`, ci_lab `f5d36625336249312ee9d5815bc875e231815bb4`, to_lab `38f7312ec4c1109b8f1c0738730b6e5ac5860f05`, sch_lab `bc848cd8a6c0407023b959191e8aa39a01928444` | nasa/cFS `v7.0.1` (`088b2fa828db9ff7e00733f1908e0eeb59f66ce3`): `.gitmodules` and the submodule entries of the tree; the releases and tags of nasa/cFS, nasa/cFE, nasa/osal, nasa/PSP, nasa/ci_lab, nasa/to_lab, nasa/sch_lab |
| F2 | Licence: Apache-2.0 for nasa/cFS, cFE, osal, PSP, ci_lab, to_lab, and sch_lab (the GitHub licence field of each repository; the `LICENSE` file of each at the pin is the Apache License 2.0 text). The `v7.0.1` release notes call it an open source release under the Apache 2.0 licence | each repository's `LICENSE` at the F1 commits; nasa/cFS release `v7.0.1` |
| F3 | Build: CMake and Make; `make native_std.prep` and `make native_std.install`, then `./core-cpu1` run from `build-native_std/exe/cpu1/`, the staging directory, where the PSP finds its startup script and modules. `native_std` is `ARCH=native` with `-DSIMULATION=native` and unit tests enabled. C is compiled as C99 (`-std=c99`) | nasa/cFS `v7.0.1`: `README.md` (Quick-start), `target-configs.mk:39`, `:52–53`, `sample_defs/arch_build_custom.cmake:29` |
| F4 | Mission configuration lives outside the cFE tree: the defs directory is chosen by `MISSIONCONFIG` (`MISSION_DEFS` = `<mission source>/<MISSIONCONFIG>_defs`); extra app directories come from the environment variable `CFS_APP_PATH`; each CPU's apps are `<cpu>_APPLIST` in `targets.cmake` (the bundle's `cpu1_APPLIST` is `ci_lab to_lab sch_lab`); the startup script lists `CFE_LIB` and `CFE_APP` entries | nasa/cFE `v7.0.1`: `cmake/mission_build.cmake:54–80`, `:380–384`, `cmake/sample_defs/cpu1_cfe_es_startup.scr`; nasa/cFS `v7.0.1`: `sample_defs/targets.cmake:133` |
| F5 | The pc-linux PSP's modules are a fixed list per PSP target (including `soft_timebase` and `timebase_posix_clock`), read from the PSP tree. A mission can add PSP modules with `<cpu>_PSP_MODULELIST`, but cannot remove one of the base list | nasa/PSP `v7.0.1`: `fsw/pc-linux/psp_module_list.cmake`, `CMakeLists.txt:66`; nasa/cFE `v7.0.1`: `cmake/global_functions.cmake:327`, `:380` |
| F6 | The local clock `CFE_PSP_GetTime` reads `CLOCK_MONOTONIC`. `soft_timebase` creates the OSAL timebase `cFS-Master` with no external synchronization, ticking every `CFE_PSP_SOFT_TIMEBASE_PERIOD` = 10000 µs | nasa/PSP `v7.0.1`: `fsw/modules/timebase_posix_clock/cfe_psp_timebase_posix_clock.c:60`, `:104`; `fsw/modules/soft_timebase/cfe_psp_soft_timebase.c:53–59`; `fsw/pc-linux/inc/cfe_psp_config.h:73`; `fsw/inc/cfe_psp_timertick_api.h:53` |
| F7 | `OS_TimeBaseCreate` takes an optional `external_sync` function that blocks the timebase's task until the next tick; with none, the kernel's timers drive the tick | nasa/osal `v7.0.1`: `src/os/inc/osapi-timebase.h:63–89` |
| F8 | cFE TIME adds a 1 Hz timer on `cFS-Master`. Current time is the time at the last tone plus the local-clock time elapsed since it; the local clock is `CFE_PSP_GetTime` | nasa/cFE `v7.0.1`: `modules/time/fsw/src/cfe_time_task.c:311–318`, `modules/time/fsw/src/cfe_time_utils.c:80–99` |
| F9 | cFE TIME's sources are platform options: `CFE_PLATFORM_TIME_CFG_SERVER` or `_CLIENT`, `_SOURCE`, one of `_SRC_MET`, `_SRC_GPS`, `_SRC_TIME`, and `_SIGNAL`. The API has `CFE_TIME_ExternalTone`, which calls the tone handler, and, when `_SRC_MET` is set, `CFE_TIME_ExternalMET`. The mission option `CFE_MISSION_TIME_CFG_FAKE_TONE` simulates the tone with a Software Bus message | nasa/cFE `v7.0.1`: `cmake/sample_defs/example_platform_cfg.h:1538–1624`, `modules/time/fsw/src/cfe_time_api.c:600–606`, `:692–700`, `cmake/sample_defs/example_mission_cfg.h:443` |
| F10 | A tone is valid only if it comes about one second after the previous one, measured with the local clock, within `CFE_PLATFORM_TIME_CFG_TONE_LIMIT` (20000 µs in the example configuration) | nasa/cFE `v7.0.1`: `modules/time/fsw/src/cfe_time_tone.c:963–1010`, `cmake/sample_defs/example_platform_cfg.h:1671` |
| F11 | cFE time: epoch 1980, day 1 in the example configuration; subseconds in units of 2⁻³² s, converted from microseconds by `CFE_TIME_Micro2SubSecs`. The default telemetry secondary header is `Time[6]`, big-endian, 4 octets of seconds and 2 of subseconds, filled with the upper 16 bits of the 32-bit subseconds. The default command secondary header is a function code and a checksum, one octet each. Message ID version 1 is the first two octets of the primary header | nasa/cFE `v7.0.1`: `cmake/sample_defs/example_mission_cfg.h:543–548`, `modules/time/fsw/src/cfe_time_api.c:537–560`, `modules/msg/option_inc/default_cfe_msg_sechdr.h:62–75`, `modules/msg/fsw/src/cfe_msg_sechdr_time.c:61–65`, `modules/msg/fsw/src/cfe_msg_msgid_v1.c:43` |
| F12 | SCH_LAB adds an OSAL timer on `cFS-Master` with a period of 1 000 000 / `TickRate` µs; each schedule entry sends its message every `PacketRate` ticks | nasa/sch_lab `v7.0.1`: `fsw/src/sch_lab_app.c:176–184`, `:263–303`, `:125–131` |
| F13 | CI_LAB receives UDP datagrams on port 1234 + processor ID − 1, copies each unchanged into a Software Bus buffer (the pass-through decoder), and transmits it | nasa/ci_lab `v7.0.1`: `fsw/inc/ci_lab_interface_cfg.h:51`, `fsw/src/ci_lab_app.c:168`, `:251–267`, `fsw/src/ci_lab_passthru_decode.c` |
| F14 | TO_LAB subscribes to the message IDs of its subscription table and sends each message unchanged (the pass-through encoder) as a UDP datagram to port 2234 + processor ID − 1, at the IP address that the command `TO_LAB_OUTPUT_ENABLE_CC` (6) sets | nasa/to_lab `v7.0.1`: `fsw/src/to_lab_app.c:477`, `:308–338`, `fsw/src/to_lab_passthru_encode.c`, `fsw/inc/to_lab_interface_cfg.h:41`, `fsw/inc/to_lab_fcncodes.h:44`, `fsw/src/to_lab_cmds.c:39–56` |
| F15 | OSAL's generic-linux console writes to `STDOUT_FILENO`. OSAL sockets have the domains INET and INET6 and the types DATAGRAM and STREAM (no local-domain sockets) | nasa/osal `v7.0.1`: `src/bsp/generic-linux/src/bsp_console.c:82`, `src/os/inc/osapi-sockets.h:63–76` |
| F16 | `SIMULATION=native` sets `OSAL_CONFIG_DEBUG_PERMISSIVE_MODE`: without privileges, tasks run at the default priority, and a message queue deeper than `/proc/sys/fs/mqueue/msg_max` is truncated without an error | nasa/cFS `v7.0.1`: `sample_defs/native_osconfig.cmake`; nasa/osal `v7.0.1`: `default_config.cmake:159–181`, `src/os/posix/src/os-impl-queues.c:55–59` |
| F17 | OSAL's POSIX loader opens modules with `dlopen` and `RTLD_NOW`, and by default `RTLD_GLOBAL`, so a library's symbols resolve the modules loaded after it | nasa/osal `v7.0.1`: `src/os/portable/os-impl-posix-dl-loader.c:77–96` |
| F18 | Software Bus and executive API: `CFE_SB_CreatePipe`, `CFE_SB_Subscribe`, `CFE_SB_TransmitMsg`, `CFE_SB_ReceiveBuffer`, `CFE_SB_TransmitBuffer`; `CFE_ES_RunLoop`, `CFE_ES_WaitForStartupSync` | nasa/cFE `v7.0.1`: `modules/core_api/fsw/inc/cfe_sb.h:89`, `:302`, `:438`, `:479`, `:591`; `modules/core_api/fsw/inc/cfe_es.h:388`, `:449` |

## 2. Decisions

Plan and scope decisions for the maintainer (decision rights §2.2, §2.3, §2.7, §2.8, PR #76), all accepted 2026-10-21 as recommended. Each row gives the choice first, then the alternatives not chosen.

| # | Decision | Choice (recommended), and alternatives | Status |
|---|---|---|---|
| D1 | Milestone shape and name | **One milestone, ACFS, with four phases P1–P4**, each with its own entry gate and exit (roadmap §3, §4 here), plan `acfs-flight-side.md`, scope `--scope acfs`, branch prefix `acfs/`. One plan makes the cFS choices (pin, build, determinism, transport) once, adds one row to the track A table, and does not renumber A1–A6. Alternatives: four milestones A1-cFS…A4-cFS, each with a plan and scope (more plans for little content, and a file named `a1-cfs-…` would collide with A1's scope prefix, so they would need IDs such as `a1c`); or a column of cFS steps in the track A table (no plan file, so no scope) | Accepted (2026-10-21) |
| D2 | Where the code lives | **`refapp/cfs/`**, track A's directory: `defs/` (the mission's defs directory: `targets.cmake`, platform and mission configuration, startup script, tables), `apps/` (the apps of [§10.3](../reference-application.md#103-architecture)), `fetch.sh` (fetches the pin), `README.md`. Its own build, the cFE mission build (F3, F4), not part of the Cargo workspace; cFS sources are never committed (APP-111). Alternatives: a separate repository (keeps cFS out of this one, but the golden traces and harness would have to be shared across repositories); a new top-level `cfs/` (a fourth owned directory, changing the roadmap's ownership table) | Accepted (2026-10-21) |
| D3 | cFS release to pin | **nasa/cFS `v7.0.1`** (`088b2fa828db9ff7e00733f1908e0eeb59f66ce3`), with cFE, OSAL, PSP, and the lab apps at the commits of F1; only the components used are fetched (cFE, OSAL, PSP; the lab apps only for reference). An upgrade is its own pull request and re-runs every phase's cases. Alternatives: `v7.0.0` (older, same API generation); the `dev` branch (moving, not reproducible) | Accepted (2026-10-21) |
| D4 | One pin shared with the cFE `MSG` cross-check | **One machine-readable pin file, `xtask/cfs.lock`** (repository, tag, commit per component), read by both the cFE `MSG` cross-check of track R (branch `m2/crosscheck-cfe`, not yet pushed) and this milestone; whichever lands first creates it. `xtask/` is shared (roadmap §6 rule 2), and a pin change then re-runs every check, which is right for a rare change. Alternatives: each consumer records its own pin and `cargo xtask consistency` checks that they are equal; or the pin lives in `refapp/cfs/` and track R reads it from track A's directory | Accepted (2026-10-21) |
| D5 | How cFS is fetched and built | **A nightly job** that fetches the pinned commits (`fetch.sh`, shallow clones by commit), builds `libaldis_ffi.a` from the commit under test, runs the cFE mission build with `SIMULATION=native` for the pc-linux PSP (F3) and the defs of D2 (F4), and runs the harness. It runs in a **small container image defined in `refapp/cfs/`** (base pinned by digest, GCC, CMake, and Make at recorded versions), built by the job; never part of the gating checks of `cargo xtask verify` (APP-123). pc-linux only (§10.7). Alternatives: the verification image, which has pinned GCC and Clang for `cpp/` (one image less, but couples the cFS build to the C++ toolchain pins and grows an image every run pulls); the bare hosted runner (fastest to set up, versions drift) | Accepted (2026-10-21) |
| D6 | Lab apps | **Replace all three, keep none in the build**: `refapp_ci` replaces CI_LAB, `refapp_to` replaces TO_LAB, `refapp_step` replaces SCH_LAB for the scenario. The cFE core services (ES, EVS, SB, TBL, TIME) run as shipped; `sample_app` and `sample_lib` are not built. Alternatives: keep SCH_LAB to send cFE's own housekeeping requests (not in the trace, adds a real-time task); keep CI_LAB and TO_LAB beside the Aldis apps for a manual UDP demonstration (option D of §10.5) | Accepted (2026-10-21) |
| D7 | App split and names | **One module per role** ([§10.3](../reference-application.md#103-architecture)): `refapp_step` (bridge and sequencer), `refapp_ci`, `refapp_to`, `refapp_cmd` (command handler, time-tag queue, table store, memory dump), `refapp_hk`, `refapp_pl`, and the cFE library `refapp_aldis`. Events are Software Bus messages to `refapp_to`. Scenario logic and Aldis calls in sans-I/O C cores, cFS glue apart (APP-121). Names avoid `aldis_`/`ALDIS_`, the C ABI's prefix. Alternatives: one cFS app holding every flight application (simplest and trivially ordered, but shows no Software Bus integration); one app per scenario application, the event reporter included (more apps and messages, no extra evidence); housekeeping as a table-driven message (the sensor values of §3.3.1 are computed, so a table holds little) | Accepted (2026-10-21) |
| D8 | Determinism | **Option C of [§10.5](../reference-application.md#105-determinism-and-the-golden-trace): lockstep sequencing.** The harness is the clock; `refapp_step` wakes one app at a time and every output passes through one pipe; scenario time comes from the WAKE, never from cFE TIME, which runs as shipped and outside the trace. No change to the trace format. Alternatives: A, cFE TIME and the scheduler driven from simulated time (needs a PSP change or a clock shim, F5, F6, F10; OI-36); B, real time with normalised comparison (weak evidence, fault cases not reproducible); D, C plus a real-time demonstration (later) | Accepted (2026-10-21) |
| D9 | Link transport | **The node protocol of §8.2 over a loopback TCP stream**, through the OSAL socket API (F15): the harness listens on a port fixed in the bridge's platform configuration (D2's defs), and the bridge connects after cFE start-up (F18); units travel only inside the protocol (APP-118). Alternatives: stdin/stdout, as §8.2 (the OSAL console writes to stdout, F15; separating them needs non-OSAL file-descriptor code in the bridge); UDP as the lab apps use (F13, F14; asynchronous, so no lockstep); a POSIX local-domain socket (not in OSAL, F15, so not portable to the other OSAL targets) | Accepted (2026-10-21) |
| D10 | Link code in P2 and P3 | **Hand-written C link loops in P2 and P3**, mirroring A2's and A3's Rust loops and following the same LNK requirements, replaced by the M4 engines in P4. Integrators who keep their heritage link layer use Aldis exactly this way, codecs only, and it gives track R C ABI feedback on the frame codecs before the v1.0 freeze. Alternative: skip P2 and P3 and go from P1 to P4 once M4 exits (less C code thrown away; no codec-only evidence, and API feedback on the frame C ABI comes late) | Accepted (2026-10-21) |
| D11 | Linking the C ABI | **The static `libaldis_ffi.a` (CABI-001)**, hosted build with default features, from the commit under test, wrapped in one cFE library module, `refapp_aldis`, that the startup script loads before the apps, so its symbols resolve theirs (F17). Alternatives: link the archive into each app (one copy of the Rust code per loaded module, each with global symbols, F17); the shared `libaldis_ffi.so` (another file to stage and find at run time) | Accepted (2026-10-21) |
| D12 | CI placement and verify inputs | **Nightly only** for the cFS build and the trace reproduction (APP-123), as a night-only job of `nightly.yml` beside fuzzing and mutation testing (VER-059), also on manual dispatch, declaring its inputs (`refapp/cfs/**`, `refapp/golden/**`, `refapp/scenario/**`, `refapp/refapp-*/**`, `crates/**`, `include/**`, `xtask/cfs.lock`). In the gate: one light check, `refapp-cfs`, that formats, analyses (D15), and unit-tests the sans-I/O C cores (APP-121) with a host C compiler and `aldis.h`, without cFS; its inputs are `refapp/cfs/**`, `include/**`, and `refapp/scenario/**`. Today `refapp/**` is an input of every Rust check, so WP-3 narrows that pattern to the Cargo workspace's paths. Alternatives: no gating check, with `refapp/cfs/**` left under `refapp/**` (every change there re-runs the Rust checks; a path read by no named check would make every check run, VER-056); the cFS build in the gate (minutes of build per pull request, and the gate would depend on cFS) | Accepted (2026-10-21) |
| D13 | Trace comparison | **Byte equality with the same `refapp/golden/<case>.jsonl`** (APP-085), no new comparison mode; the run manifest (APP-084) names the cFS components and the `aldis-ffi` build; every case runs twice per night (APP-117). Alternative: the packet projection of APP-003 only (weaker; defined for the nominal case only) | Accepted (2026-10-21) |
| D14 | Licences | **Apache-2.0 for every component used (F2), allowed by VER-055.** The sources are fetched, not committed or shipped, so Apache-2.0's redistribution conditions are not triggered; the job keeps the fetched trees out of its artefacts. Recorded here and in `refapp/cfs/README.md`; a pin change re-checks the licences. The contributor licence agreements in the bundle concern contributions to cFS, which this milestone makes none of. Alternative: none found | Accepted (2026-10-21) |
| D15 | C code rules | **C99 (F3) and SEI CERT C (VER-051)**, checked with clang-tidy's `cert-*` checks, the free analyser the C++ track already runs (OI-22), on the C cores and the glue; deviations in a deviation log in `refapp/cfs/`. The glue that calls cFE is checked with the cFE headers in the nightly job only. Alternatives: the CodeQL CERT C pack (free only for public repositories, VER-055); cppcheck's CERT add-on | Accepted (2026-10-21) |
| D16 | Requirements that do not fit a cFS node | **APP-091, APP-077's rule on threads, APP-100's Cargo rules, and APP-102 to APP-106 do not apply to `refapp/cfs/`**; APP-112, APP-116, APP-117, and APP-121 take their place ([§10.8](../reference-application.md#108-requirements)). Alternative: amend each of those requirements with an exception (more edits to accepted text, same effect) | Accepted (2026-10-21) |
| D17 | Scenario values in C | **A C header generated from `refapp/scenario/scenario.json`** by the harness's generator, checked current in CI (APP-120), built in WP-3 ahead of A5's C++ header, which can then reuse it. Alternative: hand-written C constants (they would drift from the scenario file) | Accepted (2026-10-21) |
| D18 | Node protocol before A5 | **WP-2 implements the harness side of the node protocol** (§8.2), which the A0 plan places in A5, with both transports, so P1 can run; A5 then uses it as built. Moving work between milestones is the maintainer's (OI-38). Alternative: P1 waits for A5 (P1 then starts after M5, and the cFS evidence comes last) | Accepted (2026-10-21) |
| D19 | Releases | **ACFS gates no release**: v1.0 stays gated on M6 and A6 (roadmap §5), and a phase that lags does not hold the v1.0 tag. Alternative: P4 as a v1.0 gate (stronger integration evidence at v1.0, but ties the release to a framework outside the project) | Accepted (2026-10-21) |

## 3. Work packages

Size is relative effort: **S** small, **M** medium, **L** large. WP-1 to WP-3 can run in parallel once their decisions are accepted; WP-4 to WP-7 each wait for their phase gate (§4).

```
plan accepted ─┬─▶ WP-1 cFS build skeleton ──┐
               ├─▶ WP-2 harness, transport ──┼─▶ WP-4 P1 ─▶ WP-5 P2 ─▶ WP-6 P3 ─▶ WP-7 P4 ─▶ WP-8 exit
               └─▶ WP-3 tooling ─────────────┘   (A1)       (A2, M2)   (A3, M3)   (A4, M4)
```

### WP-1 cFS build skeleton (M) — APP-111, APP-112, APP-123, APP-124

| Task | Output |
|---|---|
| `refapp/cfs/` layout (D2); `fetch.sh` reading the pin (D3, D4) | `refapp/cfs/` |
| Mission defs: `targets.cmake` with the apps of D7, platform and mission configuration (cFE TIME left at its defaults), startup script loading `refapp_aldis` first | `refapp/cfs/defs/` |
| `refapp_aldis` wrapping `libaldis_ffi.a` (D11); `refapp_step` connecting to the harness and answering HELLO, STEP with no outputs, and END | `refapp/cfs/apps/` |
| Container image definition and the nightly job (D5, D12), running `a0-empty` | `refapp/cfs/`, `.github/workflows/nightly.yml` |
| Licence record (D14); `refapp/cfs/README.md` | docs |

**Done when:** the nightly job builds the pinned cFS with `refapp_aldis` and `refapp_step`, and the harness reproduces `a0-empty` with the cFS node as the flight node.

### WP-2 Harness: cFS node and TCP transport (M) — APP-092, APP-118

| Task | Output |
|---|---|
| The node protocol of §8.2 in the harness (D18), with the stdin/stdout and TCP transports | `refapp-harness` |
| A launcher that starts `core-cpu1` in its staging directory, listens, accepts one connection, and ends the process after END_ACK; cFS console output kept in the run's log, never in the trace | `refapp-harness` |
| The run manifest names the cFS components (D13) | `refapp-harness` |
| Tests: a scripted out-of-process test node over both transports gives the same trace as in-process (APP-092) | `refapp-harness/tests/` |

**Done when:** an out-of-process test node reproduces `a0-empty` over both transports, and the tests pass in the `refapp` check.

### WP-3 Tooling: C header, trace scan, verify inputs (S) — APP-120, APP-122, APP-125

| Task | Output |
|---|---|
| C header generation from the scenario file, `gen-scenario --check` covering it (D17) | `refapp-harness`, `refapp/cfs/` |
| `cargo xtask trace` reads `// req:` in `refapp/cfs/**/*.{c,h}`; `--scope acfs` reported by `cargo xtask ci` and the verify check `spec` | `xtask` |
| Verify inputs: `refapp/**` in the Rust inputs narrowed to the Cargo workspace's paths; the gating check `refapp-cfs` for the C cores (D12) | `xtask/src/verify/checks.rs` |
| clang-tidy `cert-*` configuration and deviation log (D15) | `refapp/cfs/` |

**Done when:** a change only under `refapp/cfs/` makes `cargo xtask verify scope` list `refapp-cfs` and `spec` and no Rust check, and a planted `// req: APP-110` in a C file is reported by `cargo xtask trace --scope acfs`.

### WP-4 P1 Packets (L) — APP-110, APP-113…APP-117, APP-121

Entry: A1 exited; WP-1 to WP-3 merged. The apps of D7 with packet-level link units: `refapp_ci` decodes Space Packets, `refapp_to` builds them, `refapp_cmd`, `refapp_hk`, and `refapp_pl` implement §2.1, `refapp_step` sequences them (§10.5). C cores unit-tested on the host (APP-121).

**Done when:** the nightly job reproduces every `a1-*` golden trace twice, byte for byte (APP-110, APP-117).

### WP-5 P2 Frames (L) — APP-110, APP-113, APP-114

Entry: A2 exited; the M2 frame C ABI merged (TM, TC with the CLCW, CRC-16, the OID generator). Hand-written C loops (D10) for VC packet generation, mux, idle fill, OID frames, frame counts, and the OCF in `refapp_to`, and for TC frame validation, MAP demultiplexing, and segment reassembly with the FARM-B Counter in `refapp_ci`.

**Done when:** the nightly job reproduces every `a2-*` golden trace twice.

### WP-6 P3 COP-1 (M) — APP-110, APP-114

Entry: A3 exited; the M3 FARM-1 handle merged. FARM-1 in `refapp_ci`, its CLCW in every frame, and its status in housekeeping.

**Done when:** the nightly job reproduces every `a3-*` golden trace twice, the fault cases included.

### WP-7 P4 Engines (M) — APP-110, APP-004

Entry: A4 exited; the M4 engine handles merged. The C loops of WP-5 and WP-6 replaced by the engines.

**Done when:** the nightly job still reproduces every `a3-*` golden trace twice (APP-004).

### WP-8 Review and exit (S) — APP-101, APP-119

`refapp/cfs/README.md` written for integrators: how the apps map onto the scenario, how to build and run, where Aldis is called. The API feedback issues filed (APP-101) listed here. OI-36 to OI-38 decided or carried with an owner. The maintainer's review of the plan's exit.

## 4. Exit criteria

Phase gates and exits, as in roadmap §3:

| Phase | Entry gate | Exit |
|---|---|---|
| P1 Packets | A1; the M1 codecs' C ABI (M2 WP-0c, #72) | `a1-*` traces reproduced in the nightly job |
| P2 Frames | P1; A2; the M2 frame C ABI | `a2-*` traces reproduced in the nightly job |
| P3 COP-1 | P2; A3; the M3 FARM-1 handle | `a3-*` traces reproduced in the nightly job, fault cases included |
| P4 Engines | P3; A4; the M4 engine handles | `a3-*` traces reproduced through the engines in the nightly job |

ACFS is complete when every item below is green on `main` and in the nightly job. `cargo xtask trace --scope acfs` measures the requirements.

| Area | Criteria |
|---|---|
| Traces | APP-110 for every phase, APP-117 (every case twice), APP-004 (P4), APP-092 and APP-118 (transports) |
| cFS apps | APP-113, APP-114, APP-115, APP-116, APP-121 |
| Build and linking | APP-111, APP-112, APP-120 |
| Code rules | APP-122, VER-051 |
| CI | APP-123, VER-059; APP-125, VER-020 (`--scope acfs`) |
| Licences | APP-124, VER-055 |
| Process | APP-119 (each phase started on its gate, recorded in the work-package table), APP-101 (API feedback listed) |
| Docs | `refapp/cfs/README.md`; roadmap §3 updated; OI-36…OI-38 decided or carried |

## 5. Risks

| Risk | Mitigation |
|---|---|
| Non-determinism leaks in: an app reads cFE TIME or an OSAL clock, or two apps produce at once | One active app and one output pipe (§10.5); a check in `refapp-cfs` that the C cores call no cFE, OSAL, or PSP function (APP-121); every case twice per night (APP-117) |
| The cFS node's records come out in another order than the Rust node's within an instant | The wake order and the single output stream follow §2.1; the comparator prints the first differing record; the Rust flight node's order is the reference (roadmap §6 rule 5) |
| OSAL truncates Software Bus pipes to `/proc/sys/fs/mqueue/msg_max` without an error when run without privileges (F16), so a step's messages overflow a pipe | Bound the messages per wake; set `fs.mqueue.msg_max` for the container; `refapp_step` checks the pipe depths at start-up and fails the run |
| `libaldis_ffi.a` inside a `dlopen`ed cFE library: position-independent code, symbol visibility, and two copies of the Rust runtime | One library module loaded first (D11, F17); WP-1 proves the link before any app uses Aldis |
| A cFS upgrade changes the build or API | Pinned (D3, D4); an upgrade is its own pull request that re-runs every phase |
| The hand-written C loops (P2, P3) diverge from A2's and A3's Rust loops | The same LNK requirements and managed parameters (§4 of the scenario); divergences are resolved against the Blue Book, never by tuning the trace; the loops are replaced in P4 |
| Scenario logic is written twice, in Rust and C | Values from one file (APP-120); the golden traces catch any difference |
| The C ABI changes under the apps while track R is in M2–M4 | The nightly job builds against the commit's header; header changes are announced to track C and A (roadmap §6 rule 4); breakages become `api-feedback` issues |
| Moving the node protocol ahead of A5 (D18) changes A5's plan | OI-38; the protocol is specified (§8.2), and A5 uses what WP-2 builds |
| cFE start-up time and console output slow or clutter the nightly job | The harness waits for the connection under the job's time limit; console output goes to the run's log |
| The maintainer prefers cFE TIME to drive the scenario (option A) | OI-36 records what it would take; option C does not preclude adding it later |

## 6. For the maintainer

Questions to decide before WP-1 starts, in the order they block work, in the shape of the decision rights §3 (PR #76):

1. **D1 (`plan`).** One milestone ACFS with phases P1–P4, or four milestones? Recommended: one.
2. **D8 (`plan`).** Lockstep sequencing (option C), with cFE TIME untouched? Recommended: yes. OI-36 stays open.
3. **D18 and OI-38 (`scope`).** Build the harness side of the node protocol in this milestone, ahead of A5, with a TCP transport? Recommended: yes.
4. **D12 (`process`).** A nightly-only cFS job, plus a light gating check for the C cores, with `refapp/**` narrowed in the Rust inputs? Recommended: yes.
5. **D5, D14 (`deps`).** cFS `v7.0.1` (Apache-2.0) as a new external dependency of the nightly job, fetched and never committed? Recommended: yes.
6. **D19 (`scope`).** ACFS gates no release? Recommended: yes.
7. **D16 (`scope`).** APP-091, APP-077 (threads), APP-100 (Cargo rules), and APP-102…APP-106 do not apply to `refapp/cfs/`? Recommended: yes.

The other decisions (D2–D4, D6, D7, D9–D11, D13, D15, D17) follow from these and can be accepted together.
