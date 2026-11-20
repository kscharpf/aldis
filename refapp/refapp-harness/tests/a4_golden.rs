//! The invariants of the A4 cases, checked on their golden traces (A4 WP-5; D8 and D9 of
//! docs/milestones/a4-variants.md; docs/reference-application.md §4.6).
//!
//! Every expected frame is built here, octet by octet, from the tables of §4.6.2 and §4.6.3 and
//! the packets the flight node generated, and compared with the trace; no expectation is read
//! back from the trace except the packets themselves (their instants and octets are checked
//! against `a1-nominal` and `a3-nominal` in `the_packets_are_those_of_the_other_levels`).

use std::collections::BTreeMap;

use refapp_harness::golden_path;
use refapp_harness::trace::{check_canonical, packet_projection};
use serde_json::Value;

const SLOT: u64 = 125_000;
const DELAY: u64 = 10_000;
const AOS: &str = "a4-aos-nominal";
const USLP: &str = "a4-uslp-nominal";

/// The records of a case's golden trace, after the header.
fn records(case: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(golden_path(case)).unwrap();
    check_canonical(&text).unwrap();
    text.lines()
        .skip(1)
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// A unit sent: its send instant and octets, by index.
fn units(case: &str, dir: &str) -> Vec<(u64, Vec<u8>)> {
    let mut sent: Vec<(u64, u64, Vec<u8>)> = records(case)
        .iter()
        .filter(|r| r["kind"] == "unit_sent" && r["dir"] == dir)
        .map(|r| {
            (
                r["index"].as_u64().unwrap(),
                r["t_us"].as_u64().unwrap(),
                unhex(r["hex"].as_str().unwrap()),
            )
        })
        .collect();
    sent.sort_by_key(|u| u.0);
    for (want, u) in sent.iter().enumerate() {
        assert_eq!(u.0, want as u64, "{case}: {dir} indexes are 0, 1, 2, …");
    }
    sent.into_iter().map(|u| (u.1, u.2)).collect()
}

/// One application event: instant, name, arguments.
type Event = (u64, String, Vec<u64>);

fn app_events(case: &str, node: &str) -> Vec<Event> {
    records(case)
        .iter()
        .filter(|r| r["kind"] == "app_event" && r["node"] == node)
        .map(|r| {
            (
                r["t_us"].as_u64().unwrap(),
                r["name"].as_str().unwrap().to_owned(),
                r["args"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|a| a.as_u64().unwrap())
                    .collect(),
            )
        })
        .collect()
}

fn event(t: u64, name: &str, args: &[u64]) -> Event {
    (t, name.to_owned(), args.to_vec())
}

/// The `packet_out` or `packet_in` records of one node: instant, APID, octets.
fn packets(case: &str, kind: &str, node: &str) -> Vec<(u64, u64, Vec<u8>)> {
    records(case)
        .iter()
        .filter(|r| r["kind"] == kind && r["node"] == node)
        .map(|r| {
            (
                r["t_us"].as_u64().unwrap(),
                r["apid"].as_u64().unwrap(),
                unhex(r["hex"].as_str().unwrap()),
            )
        })
        .collect()
}

/// The arrival instants of the uplink units delivered, in trace order: index, instant.
fn up_arrivals(case: &str) -> Vec<(u64, u64)> {
    records(case)
        .iter()
        .filter(|r| r["kind"] == "unit_delivered" && r["dir"] == "up")
        .map(|r| (r["index"].as_u64().unwrap(), r["t_us"].as_u64().unwrap()))
        .collect()
}

/// CRC-16/CCITT-FALSE, the FECF of 232.0 §4.1.4, 732.0 §4.1.6 and 732.1 §4.1.6.
fn crc16(data: &[u8]) -> u16 {
    let mut crc = 0xFFFF_u16;
    for &b in data {
        crc ^= u16::from(b) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 == 0 {
                crc << 1
            } else {
                (crc << 1) ^ 0x1021
            };
        }
    }
    crc
}

/// `body` followed by its FECF.
fn with_fecf(mut body: Vec<u8>) -> Vec<u8> {
    let c = crc16(&body);
    body.extend(c.to_be_bytes());
    body
}

// ---------------------------------------------------------------------------------------
// The schedule of the downlink, from §4.6.2 and §4.6.3.
// ---------------------------------------------------------------------------------------

/// Events of the second `k` s (VC 0, §2.4, §4.4.4): the counts that make the idle packets of
/// 142, 121, and 100 octets (AOS).
const EVENTS_AT: [(u64, usize); 5] = [(6, 1), (10, 2), (12, 1), (20, 3), (24, 2)];

/// The idle packet that fills the VC 0 zone, per number of events (0 to 3): §4.6.2, §4.6.3.
const AOS_IDLE0: [usize; 4] = [163, 142, 121, 100];
const USLP_IDLE0: [usize; 4] = [162, 141, 120, 99];
/// The idle packet of frame 102 (`DUMP_COMPLETED` alone).
const AOS_IDLE102: usize = 186;
const USLP_IDLE102: usize = 185;

/// The payload packets: T(n) in µs and the kind (§4.6.2), without packet 6 (the window).
const AOS_PAYLOADS: [(usize, u64, char); 12] = [
    (0, 500_000, 'E'),
    (1, 2_500_000, 'O'),
    (2, 4_500_000, 'E'),
    (3, 6_500_000, 'O'),
    (4, 8_500_000, 'E'),
    (5, 10_500_000, 'O'),
    (7, 14_500_000, 'E'),
    (8, 16_500_000, 'O'),
    (9, 18_500_000, 'E'),
    (10, 24_500_000, 'E'),
    (11, 26_500_000, 'O'),
    (12, 28_500_000, 'E'),
];
/// The same for the USLP case (§4.6.3, no kinds).
const USLP_PAYLOADS: [(usize, u64); 12] = [
    (0, 500_000),
    (1, 2_500_000),
    (2, 4_500_000),
    (3, 6_500_000),
    (4, 8_500_000),
    (5, 10_500_000),
    (7, 14_500_000),
    (8, 16_500_000),
    (9, 18_500_000),
    (10, 24_500_000),
    (11, 26_500_000),
    (12, 28_500_000),
];

/// VC 1 frames of the two cases with their VC Frame Counts, as the tables of §4.6.2 and §4.6.3
/// print them.
const AOS_VC1: [(usize, u32); 37] = [
    (4, 0),
    (12, 1),
    (20, 2),
    (21, 3),
    (29, 4),
    (36, 5),
    (44, 6),
    (52, 7),
    (53, 8),
    (61, 9),
    (68, 10),
    (76, 11),
    (84, 12),
    (85, 13),
    (93, 14),
    (97, 15),
    (99, 16),
    (100, 17),
    (101, 18),
    (103, 19),
    (110, 20),
    (116, 21),
    (124, 22),
    (132, 23),
    (133, 24),
    (141, 25),
    (148, 26),
    (156, 27),
    (164, 28),
    (196, 29),
    (204, 30),
    (212, 31),
    (213, 32),
    (221, 33),
    (228, 34),
    (236, 35),
    (244, 36),
];
const USLP_VC1: [(usize, u32); 30] = [
    (4, 0),
    (5, 1),
    (20, 2),
    (21, 3),
    (36, 4),
    (37, 5),
    (52, 6),
    (53, 7),
    (68, 8),
    (69, 9),
    (84, 10),
    (85, 11),
    (97, 12),
    (99, 13),
    (100, 14),
    (101, 15),
    (103, 16),
    (110, 17),
    (116, 18),
    (117, 19),
    (132, 20),
    (133, 21),
    (148, 22),
    (149, 23),
    (196, 24),
    (197, 25),
    (212, 26),
    (213, 27),
    (228, 28),
    (229, 29),
];

/// The VC 0 frames: index and the VC Frame Count of the tables (§4.6.2: *k* for 8*k* + 2 with
/// *k* ≤ 12, 13 for frame 102, *k* + 1 after).
fn vc0_frames() -> Vec<(usize, u32)> {
    let mut v: Vec<(usize, u32)> = (0..30_u32)
        .map(|k| {
            let i = 8 * ix(u64::from(k)) + 2;
            (i, if k <= 12 { k } else { k + 1 })
        })
        .collect();
    v.push((102, 13));
    v.sort_unstable();
    v
}

fn ix(i: u64) -> usize {
    usize::try_from(i).unwrap()
}

/// The first `n` octets of the pseudo-noise sequence of the OID frames (732.1 §4.1.4.1.10,
/// annex H): the LFSR of the polynomial D0 + D1 + D2 + D22 + D32 in the Fibonacci form with an
/// all-ones seed, so that every bit is the sum of the bits 1, 2, 22, and 32 places before it.
fn oid_sequence(n: usize) -> Vec<u8> {
    let mut bits = vec![true; 32];
    for k in 32..8 * n {
        bits.push(bits[k - 1] ^ bits[k - 2] ^ bits[k - 22] ^ bits[k - 32]);
    }
    bits.chunks(8)
        .map(|c| c.iter().fold(0_u8, |a, &b| a << 1 | u8::from(b)))
        .collect()
}

/// The Space Packet Idle Packet of `len` octets: APID 2047, count 0 (§4.6.1).
fn aos_idle(len: usize) -> Vec<u8> {
    let l = u16::try_from(len - 7).unwrap().to_be_bytes();
    let mut v = vec![0x07, 0xFF, 0xC0, 0x00, l[0], l[1]];
    v.resize(len, 0);
    v
}

/// The Encapsulation Idle Packet of `len` octets: `E1`, the length, idle octets `00` (§4.6.1).
fn uslp_idle(len: usize) -> Vec<u8> {
    let mut v = vec![0xE1, u8::try_from(len).unwrap()];
    v.resize(len, 0);
    v
}

/// The packets the flight node generated, by APID, in generation order.
fn by_apid(case: &str, apid: u64) -> Vec<(u64, Vec<u8>)> {
    packets(case, "packet_out", "flight")
        .into_iter()
        .filter(|p| p.1 == apid)
        .map(|p| (p.0, p.2))
        .collect()
}

/// The zone of a VC 0 frame: the packets of the instant (events, then HK) and the idle packet
/// (§4.6.2). `at` is the generation instant; `dump` selects frame 102 (the event of 12.5 s).
fn vc0_zone(
    case: &str,
    k: u64,
    dump: bool,
    idle: impl Fn(usize) -> Vec<u8>,
    table: &[usize; 4],
    size: usize,
    idle102: usize,
) -> Vec<u8> {
    let mut z = Vec::new();
    let (at, apids): (u64, &[u64]) = if dump {
        (12_500_000, &[17])
    } else {
        (k * 1_000_000, &[17, 16])
    };
    let all = packets(case, "packet_out", "flight");
    // Events first (they are generated before the HK packet of the same instant).
    for apid in apids {
        for p in all.iter().filter(|p| p.0 == at && p.1 == *apid) {
            z.extend(&p.2);
        }
    }
    let n_events = if dump {
        0
    } else {
        EVENTS_AT.iter().find(|e| e.0 == k).map_or(0, |e| e.1)
    };
    let want_idle = if dump { idle102 } else { table[n_events] };
    assert_eq!(z.len() + want_idle, size, "{case}: k = {k}: idle fill");
    z.extend(idle(want_idle));
    z
}

/// The CLCW word of slot `i`: the first word until the first row, then the word of the last row
/// at or before the slot (§4.6.2, §4.6.3: N(R) = frames accepted, nothing else set).
fn ocf_of(rows: &[usize], i: usize) -> [u8; 4] {
    let n_r = u32::try_from(rows.iter().filter(|&&r| r <= i).count()).unwrap();
    (0x0100_0000 + n_r).to_be_bytes()
}

/// The rows at which the CLCW changes (downlink frame of the arrival: §4.6.2, §4.6.3).
const AOS_ROWS: [usize; 8] = [9, 17, 19, 21, 25, 33, 41, 49];
const USLP_ROWS: [usize; 6] = [9, 17, 25, 33, 41, 49];

/// An expected zone and First Header Pointer of a VC 1 frame.
type Field = (u16, Vec<u8>);

/// VC 1 of the AOS case: the E and O payload packets, the two residues, and the window
/// (§4.6.2).
fn aos_vc1(case: &str) -> BTreeMap<usize, Field> {
    let pl: Vec<Vec<u8>> = by_apid(case, 32).into_iter().map(|p| p.1).collect();
    let dump: Vec<Vec<u8>> = by_apid(case, 33).into_iter().map(|p| p.1).collect();
    assert_eq!((pl.len(), dump.len()), (13, 5));
    let residue5 = [0xC0, 0, 0, 0, 0];
    let mut m = BTreeMap::new();
    for (n, t, kind) in AOS_PAYLOADS {
        let p = &pl[n];
        let i = ix(t / SLOT);
        assert_eq!(
            by_apid(case, 32)[n].0,
            t,
            "payload {n} is generated at T(n)"
        );
        assert_eq!(p.len(), 412);
        if kind == 'E' {
            m.insert(i, (0, p[..207].to_vec()));
            let mut z = p[207..].to_vec();
            z.extend([0x07, 0xFF]);
            m.insert(i + 8, (205, z));
        } else {
            let mut z = residue5.to_vec();
            z.extend(&p[..202]);
            m.insert(i, (5, z));
            m.insert(i + 1, (0xFFFF, p[202..409].to_vec()));
            let mut z = p[409..].to_vec();
            z.extend(aos_idle(204));
            m.insert(i + 9, (3, z));
        }
    }
    for i in [164, 244] {
        let mut z = residue5.to_vec();
        z.extend(aos_idle(202));
        m.insert(i, (5, z));
    }
    // The window: dumps 0…4 and payload 6, one stream of 1 078 octets.
    let mut s: Vec<u8> = dump.concat();
    s.extend(&pl[6]);
    assert_eq!(s.len(), 1078);
    for (j, (i, fhp)) in [(97, 0), (99, 73), (100, 6), (101, 45), (103, 0xFFFF)]
        .into_iter()
        .enumerate()
    {
        m.insert(i, (fhp, s[207 * j..207 * (j + 1)].to_vec()));
    }
    let mut z = s[1035..].to_vec();
    z.extend(aos_idle(164));
    m.insert(110, (43, z));
    m
}

/// VC 1 of the USLP case (§4.6.3).
fn uslp_vc1(case: &str) -> BTreeMap<usize, Field> {
    let pl: Vec<Vec<u8>> = by_apid(case, 32).into_iter().map(|p| p.1).collect();
    let dump: Vec<Vec<u8>> = by_apid(case, 33).into_iter().map(|p| p.1).collect();
    let mut m = BTreeMap::new();
    for (n, t) in USLP_PAYLOADS {
        let i = ix(t / SLOT);
        assert_eq!(
            by_apid(case, 32)[n].0,
            t,
            "payload {n} is generated at T(n)"
        );
        m.insert(i, (0, pl[n][..206].to_vec()));
        m.insert(i + 1, (0xFFFF, pl[n][206..].to_vec()));
    }
    let mut s: Vec<u8> = dump.concat();
    s.extend(&pl[6]);
    assert_eq!(s.len(), 1078);
    for (j, (i, fhp)) in [(97, 0), (99, 74), (100, 8), (101, 48), (103, 0xFFFF)]
        .into_iter()
        .enumerate()
    {
        m.insert(i, (fhp, s[206 * j..206 * (j + 1)].to_vec()));
    }
    let mut z = s[1030..].to_vec();
    z.extend(uslp_idle(158));
    m.insert(110, (48, z));
    m
}

/// The FHEC of the three AOS VCs (§4.6.2, from `test-vectors/models/aos_fhec.py`).
const FHEC: [(u8, [u8; 2]); 3] = [(0, [0xA5, 0x79]), (1, [0x87, 0x67]), (7, [0x4B, 0x05])];

/// An expected AOS frame (§4.6.2): header 6, FHEC 2, `M_PDU` header 2, zone 207, OCF 4, FECF 2.
fn aos_frame(vc: u8, count: u32, fhp: u16, zone: &[u8], ocf: [u8; 4]) -> Vec<u8> {
    assert_eq!(zone.len(), 207);
    let c = count.to_be_bytes();
    let fhec = FHEC.iter().find(|f| f.0 == vc).unwrap().1;
    // Octet 5 is `10`: Replay 0, Cycle Use 0, SCID MSBs `01`, cycle `0000`.
    let mut f = vec![0x55, 0x40 | vc, c[1], c[2], c[3], 0x10, fhec[0], fhec[1]];
    f.extend(fhp.to_be_bytes());
    f.extend(zone);
    f.extend(ocf);
    with_fecf(f)
}

/// An expected USLP data frame (§4.6.3): header 8, TFDF header 3, zone 206, OCF 4, FECF 2.
fn uslp_data_frame(vc: u8, count: u32, fhp: u16, zone: &[u8], ocf: [u8; 4]) -> Vec<u8> {
    assert_eq!(zone.len(), 206);
    let mut f = vec![
        0xC0,
        0x15,
        0x50,
        vc << 5,
        0x00,
        0xDE,
        0x89,
        u8::try_from(count).unwrap(),
        0x00,
    ];
    f.extend(fhp.to_be_bytes());
    f.extend(zone);
    f.extend(ocf);
    with_fecf(f)
}

/// An expected USLP OID frame (§4.6.3): header 7, TFDF header 3, zone 207, OCF 4, FECF 2.
fn uslp_oid_frame(zone: &[u8], ocf: [u8; 4]) -> Vec<u8> {
    assert_eq!(zone.len(), 207);
    let mut f = vec![0xC0, 0x15, 0x57, 0xE0, 0x00, 0xDE, 0x88, 0x3F, 0x00, 0xCE];
    f.extend(zone);
    f.extend(ocf);
    with_fecf(f)
}

#[test]
fn the_packets_are_those_of_the_other_levels() {
    // req: APP-003, APP-045, APP-188
    let a1 = std::fs::read_to_string(golden_path("a1-nominal")).unwrap();
    for case in [AOS, USLP] {
        let t = std::fs::read_to_string(golden_path(case)).unwrap();
        assert_eq!(
            packet_projection(&t).unwrap(),
            packet_projection(&a1).unwrap(),
            "{case}: the packet projection is that of a1-nominal"
        );
        for node in ["flight", "ground"] {
            let x = packets("a3-nominal", "packet_out", node);
            let y = packets(case, "packet_out", node);
            assert!(!x.is_empty(), "{node} outputs packets");
            let instants = |v: &[(u64, u64, Vec<u8>)]| v.iter().map(|p| p.0).collect::<Vec<_>>();
            assert_eq!(
                instants(&y),
                instants(&x),
                "{case}: {node}: packet_out instants"
            );
        }
        assert_eq!(units(case, "down").len(), 280);
    }
}

#[test]
fn the_aos_downlink_follows_the_schedule() {
    // req: APP-169, APP-170, APP-171, APP-172, APP-173, APP-188
    let down = units(AOS, "down");
    let vc0 = vc0_frames();
    let vc1 = aos_vc1(AOS);
    assert_eq!(vc1.len(), AOS_VC1.len());
    let mut vc0_map: BTreeMap<usize, u32> = vc0.iter().copied().collect();
    let vc1_map: BTreeMap<usize, u32> = AOS_VC1.iter().copied().collect();
    assert_eq!((vc0_map.len(), vc1_map.len()), (31, 37));
    let mut count7 = 0_u32;
    let mut kinds = [0_u32; 3];
    for (i, (t, f)) in down.iter().enumerate() {
        assert_eq!(*t, i as u64 * SLOT, "frame {i}: sent at its slot");
        let ocf = ocf_of(&AOS_ROWS, i);
        let want = if let Some(c) = vc0_map.remove(&i) {
            kinds[0] += 1;
            let dump = i == 102;
            let k = (i as u64).wrapping_sub(2) / 8;
            let zone = vc0_zone(AOS, k, dump, aos_idle, &AOS_IDLE0, 207, AOS_IDLE102);
            aos_frame(0, c, 0, &zone, ocf)
        } else if let Some(c) = vc1_map.get(&i) {
            kinds[1] += 1;
            let (fhp, zone) = &vc1[&i];
            aos_frame(1, *c, *fhp, zone, ocf)
        } else {
            kinds[2] += 1;
            // VC 7: the idle M_PDU `FF FE`, 207 octets `00`; the count is the rank (APP-171).
            let c = count7;
            count7 += 1;
            aos_frame(7, c, 0xFFFE, &[0; 207], ocf)
        };
        assert_eq!(f, &want, "frame {i}");
        assert_eq!(f.len(), 223);
    }
    assert!(
        vc0_map.is_empty(),
        "every VC 0 frame of the schedule exists"
    );
    // Totals and last counts (§4.6.2).
    assert_eq!(kinds, [31, 37, 212]);
    assert_eq!(count7, 212);
    // The last frame of each VC shows its final count (24 bits, octets 2 to 4).
    let count = |i: usize| u32::from_be_bytes([0, down[i].1[2], down[i].1[3], down[i].1[4]]);
    assert_eq!((count(234), count(244), count(279)), (30, 36, 211));
    // The first frames of the three VCs (§4.6.2).
    assert_eq!(
        down[0].1[..10],
        [0x55, 0x47, 0, 0, 0, 0x10, 0x4B, 0x05, 0xFF, 0xFE]
    );
    assert_eq!(
        down[2].1[..10],
        [0x55, 0x40, 0, 0, 0, 0x10, 0xA5, 0x79, 0x00, 0x00]
    );
    assert_eq!(
        down[4].1[..10],
        [0x55, 0x41, 0, 0, 0, 0x10, 0x87, 0x67, 0x00, 0x00]
    );
}

#[test]
fn the_uslp_downlink_follows_the_schedule() {
    // req: APP-175, APP-176, APP-177, APP-178, APP-179, APP-188
    let down = units(USLP, "down");
    let vc0 = vc0_frames();
    let vc1 = uslp_vc1(USLP);
    let mut vc0_map: BTreeMap<usize, u32> = vc0.iter().copied().collect();
    let vc1_map: BTreeMap<usize, u32> = USLP_VC1.iter().copied().collect();
    assert_eq!((vc0_map.len(), vc1_map.len()), (31, 30));
    // The OID generator is never restarted (CMN-021): one sequence over the 219 OID frames.
    let stream = oid_sequence(207 * 219);
    let first_ten = [0xFF, 0xFF, 0xFF, 0xFF, 0x6D, 0xB6, 0xD8, 0x61, 0x45, 0x1F];
    assert_eq!(
        stream[..10],
        first_ten,
        "732.1 §4.1.4.1.10: the sequence of the doc"
    );
    let mut oid = 0_usize;
    let mut kinds = [0_u32; 3];
    for (i, (t, f)) in down.iter().enumerate() {
        assert_eq!(*t, i as u64 * SLOT, "frame {i}: sent at its slot");
        let ocf = ocf_of(&USLP_ROWS, i);
        let want = if let Some(c) = vc0_map.remove(&i) {
            kinds[0] += 1;
            let k = (i as u64).wrapping_sub(2) / 8;
            let zone = vc0_zone(USLP, k, i == 102, uslp_idle, &USLP_IDLE0, 206, USLP_IDLE102);
            uslp_data_frame(0, c, 0, &zone, ocf)
        } else if let Some(c) = vc1_map.get(&i) {
            kinds[1] += 1;
            let (fhp, zone) = &vc1[&i];
            uslp_data_frame(1, *c, *fhp, zone, ocf)
        } else {
            kinds[2] += 1;
            let z = &stream[207 * oid..207 * (oid + 1)];
            oid += 1;
            uslp_oid_frame(z, ocf)
        };
        assert_eq!(f, &want, "frame {i}");
        assert_eq!(f.len(), 223);
    }
    assert!(vc0_map.is_empty());
    assert_eq!(kinds, [31, 30, 219], "61 data frames and 219 OID frames");
    assert_eq!(
        down[0].1[10..20],
        first_ten,
        "the OID frame 0 zone (§4.6.3)"
    );
    assert_eq!(
        down[0].1[..10],
        [0xC0, 0x15, 0x57, 0xE0, 0x00, 0xDE, 0x88, 0x3F, 0x00, 0xCE]
    );
    assert_eq!(
        down[2].1[..11],
        [
            0xC0, 0x15, 0x50, 0x00, 0x00, 0xDE, 0x89, 0x00, 0x00, 0x00, 0x00
        ]
    );
    assert_eq!(
        down[4].1[..11],
        [
            0xC0, 0x15, 0x50, 0x20, 0x00, 0xDE, 0x89, 0x00, 0x00, 0x00, 0x00
        ]
    );
    assert_eq!(
        down[5].1[..11],
        [
            0xC0, 0x15, 0x50, 0x20, 0x00, 0xDE, 0x89, 0x01, 0x00, 0xFF, 0xFF
        ]
    );
    // The last data frame of each VC shows its final count (octet 7).
    assert_eq!((down[234].1[7], down[229].1[7]), (30, 29));
}

#[test]
fn the_clcw_words_are_those_of_the_tables() {
    // req: APP-173, APP-179, APP-188
    // 232.0 §4.2.1: type 0, version 00, status 000, COP in effect 01, VCID 0 (the uplink VC),
    // no flag, FARM-B Counter 0, N(R) = the frames accepted.
    for (case, rows) in [(AOS, &AOS_ROWS[..]), (USLP, &USLP_ROWS[..])] {
        let down = units(case, "down");
        let mut n_r = 0_u32;
        for (i, (_, f)) in down.iter().enumerate() {
            if rows.contains(&i) {
                n_r += 1;
            }
            let ocf = u32::from_be_bytes([f[217], f[218], f[219], f[220]]);
            assert_eq!(ocf, 0x0100_0000 + n_r, "{case}: frame {i}: OCF");
            assert_eq!(ocf >> 31, 0, "Type Flag 0: a CLCW (232.0 §4.2.1.1.1)");
            assert_eq!(
                ocf >> 8 & 0xFF_FFFF,
                0x01_0000,
                "COP in effect 1, VCID 0, no flag, counter 0"
            );
        }
        assert_eq!(usize::try_from(n_r).unwrap(), rows.len());
    }
}

/// The ground events of the nominal case: the directives of 0.0 s, then per row `clcw_changed`
/// and `fop1_transfer`, and the dump (§4.5.5, §4.6.2, §4.6.3).
fn ground_events(rows: &[usize]) -> Vec<Event> {
    let mut v = vec![
        event(0, "fop1_directive", &[1, 1, 0]),
        event(0, "fop1_directive", &[3, 1, 0]),
        event(0, "fop1_state", &[1]),
    ];
    for (n, &i) in rows.iter().enumerate() {
        let t = i as u64 * SLOT + DELAY;
        v.push(event(t, "clcw_changed", &[0x0100_0000 + n as u64 + 1]));
        v.push(event(t, "fop1_transfer", &[3, n as u64]));
    }
    v.push(event(
        12_635_000,
        "dump_reassembled",
        &[65_536, 600, 32_936],
    ));
    v.sort_by_key(|e| e.0);
    v
}

fn commands() -> Vec<Event> {
    vec![
        event(1_010_000, "cmd_received", &[1, 0]),
        event(3_010_000, "cmd_received", &[3, 1]),
        event(4_010_000, "cmd_received", &[2, 2]),
        event(5_010_000, "cmd_received", &[4, 3]),
        event(5_010_000, "cmd_received", &[1, 4]),
        event(6_010_000, "cmd_received", &[4, 5]),
    ]
}

#[test]
fn the_aos_events_follow_the_schedule() {
    // req: APP-174, APP-188
    assert_eq!(app_events(AOS, "ground"), ground_events(&AOS_ROWS));
    let mut flight = commands();
    flight.insert(1, event(2_510_000, "table_received", &[1, 300]));
    assert_eq!(
        app_events(AOS, "flight"),
        flight,
        "no frame-level event of the flight node"
    );
}

#[test]
fn the_uslp_events_follow_the_schedule() {
    // req: APP-180, APP-185, APP-186, APP-188
    assert_eq!(app_events(USLP, "ground"), ground_events(&USLP_ROWS));
    let mut flight = commands();
    flight.insert(1, event(2_010_000, "table_received", &[1, 300]));
    assert_eq!(
        app_events(USLP, "flight"),
        flight,
        "FARM-1 accepts every frame (E1), no event"
    );
}

#[test]
fn every_command_arrives_before_its_execution_time() {
    // req: APP-188
    // §4.6.4: arrival (the release instant and the one-way delay), execution, per case.
    let exec = [
        6_000_000, 10_000_000, 12_000_000, 20_000_000, 20_000_000, 24_000_000,
    ];
    for (case, table_at) in [(AOS, 2_510_000_u64), (USLP, 2_010_000)] {
        let received = app_events(case, "flight");
        let cmds: Vec<&Event> = received.iter().filter(|e| e.1 == "cmd_received").collect();
        assert_eq!(cmds.len(), exec.len());
        let mut least = u64::MAX;
        for (e, x) in cmds.iter().zip(exec) {
            assert!(e.0 < x, "{case}: command {:?} arrives before {x}", e.2);
            least = least.min(x - e.0);
        }
        assert_eq!(least, 4_990_000, "{case}: the smallest margin is 4.990 s");
        // The table is stored before `ACTIVATE_TABLE` arrives at 3.010 s.
        let table = received.iter().find(|e| e.1 == "table_received").unwrap();
        assert_eq!(table.0, table_at);
        assert!(table.0 < 3_010_000);
        // No command is rejected at delivery (APP-012): the event packets are the ten of
        // a1-nominal and every one of the six commands was received above.
        let event_packets = packets(case, "packet_out", "flight")
            .iter()
            .filter(|p| p.1 == 17)
            .count();
        assert_eq!(event_packets, 10, "{case}: the event packets of a1-nominal");
        // Every uplink frame arrives one delay after it is sent.
        let up = units(case, "up");
        for (i, t) in up_arrivals(case) {
            assert_eq!(t, up[ix(i)].0 + DELAY, "{case}: unit {i}");
        }
    }
}

#[test]
fn the_aos_uplink_is_that_of_the_tc_case() {
    // req: APP-188
    // §4.6.2: the eight Type-AD frames of §4.5.5, first octets `01 55`, N(S) 0 to 7.
    let up = units(AOS, "up");
    let a3 = units("a3-nominal", "up");
    assert_eq!(up, a3, "the uplink is that of a3-nominal, unchanged");
    let sent = [
        1_000_000, 2_000_000, 2_250_000, 2_500_000, 3_000_000, 4_000_000, 5_000_000, 6_000_000,
    ];
    assert_eq!(up.len(), 8);
    for (i, (t, f)) in up.iter().enumerate() {
        assert_eq!(*t, sent[i]);
        assert_eq!(f[..2], [0x01, 0x55], "unit {i}");
        assert_eq!(usize::from(f[3]), f.len() - 1, "unit {i}: length field");
        assert_eq!(usize::from(f[4]), i, "unit {i}: N(S)");
        let n = f.len();
        assert_eq!(
            u16::from_be_bytes([f[n - 2], f[n - 1]]),
            crc16(&f[..n - 2]),
            "unit {i}: FECF"
        );
    }
}

#[test]
fn the_uslp_uplink_follows_the_table() {
    // req: APP-181, APP-182, APP-183, APP-184, APP-188
    // (index, send instant, N(S), packets of the TFDZ as the ground node's outputs, TFDZ length)
    let rows: [(usize, u64, u8, &[usize], usize); 6] = [
        (0, 1_000_000, 0, &[0], 14),
        (1, 2_000_000, 1, &[1], 309),
        (2, 3_000_000, 2, &[2], 17),
        (3, 4_000_000, 3, &[3], 20),
        (4, 5_000_000, 4, &[4, 5], 29),
        (5, 6_000_000, 5, &[6], 15),
    ];
    let out = packets(USLP, "packet_out", "ground");
    assert_eq!(out.len(), 7);
    let up = units(USLP, "up");
    assert_eq!(up.len(), 6, "the units are the six frames of the table");
    for (i, t, ns, pk, len) in rows {
        let tfdz: Vec<u8> = pk.iter().flat_map(|&p| out[p].2.clone()).collect();
        assert_eq!(tfdz.len(), len, "unit {i}: TFDZ length");
        assert!(
            pk.iter().all(|&p| out[p].0 == t),
            "unit {i}: released at its instant"
        );
        // Type-AD, SCID 0x155, Source-or-Destination 1, VC 0, MAP 0; Frame Length one less than
        // the octets; Bypass 0, PCC 0, OCF flag 0, count length 1; N(S); TFDF Header `E0`.
        let octets = len + 8 + 1 + 2;
        let l = u16::try_from(octets - 1).unwrap().to_be_bytes();
        let mut want = vec![0xC0, 0x15, 0x58, 0x00, l[0], l[1], 0x01, ns, 0xE0];
        want.extend(tfdz);
        let want = with_fecf(want);
        assert_eq!(up[i].0, t, "unit {i}: send instant");
        assert_eq!(up[i].1, want, "unit {i}");
        assert!(
            up[i].1.len() <= 512,
            "unit {i}: within the maximum frame length"
        );
    }
    assert_eq!(
        up[1].1.len(),
        320,
        "the table upload is one frame of 320 octets"
    );
    // No Type-BC frame is sent, and no fault is applied.
    assert!(
        up.iter().all(|u| u.1[6] & 0x80 == 0),
        "every frame is Type-AD (Bypass 0)"
    );
    assert!(records(USLP).iter().all(|r| r["kind"] != "fault"));
    assert!(records(AOS).iter().all(|r| r["kind"] != "fault"));
}
