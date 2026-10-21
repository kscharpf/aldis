//! Random packets compared between Aldis and NASA cFE's `MSG` module (VER-040,
//! cross-checks.md §2).
//!
//! The packets come from `aldis_testkit::strategies::spp`, built by `aldis-spp`'s own encoder,
//! with a fixed seed so that every run checks the same cases. Each property compares both ways:
//! cFE reads what Aldis wrote, and cFE builds from the same fields the octets Aldis wrote.

use aldis_spp::{PacketFields, PacketType, SppConfig, encode};
use aldis_testkit::strategies::proptest::prelude::*;
use aldis_testkit::strategies::proptest::strategy::ValueTree;
use aldis_testkit::strategies::proptest::test_runner::{Config, RngAlgorithm, TestRng, TestRunner};
use aldis_testkit::strategies::{octets, spp};
use aldis_time::{Cuc, CucEpoch, CucFormat, CucValue, TimeCode};

use crate::cfe_msg::{self, Fields, Header};

/// The seed of every property run, so that the cases are the same in each run.
const SEED: [u8; 32] = *b"aldis-crosscheck-cfe-msg-v7.0.1!";

/// The result of one property.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropertyReport {
    /// What the property compares.
    pub name: &'static str,
    /// Cases on which cFE and Aldis agree.
    pub agreed: usize,
    /// Disagreements: the case and the differences.
    pub disagreements: Vec<String>,
}

/// Runs every property with its fixed number of cases.
#[must_use]
pub fn run() -> Vec<PropertyReport> {
    vec![
        property(
            "primary header, data field up to 46 octets",
            2000,
            spp::space_packet(),
            |packet| header_agrees(&packet),
        ),
        property(
            "primary header, data field up to 65 536 octets",
            200,
            spp::space_packet_with(spp::packet_fields(), 8, 65_537),
            |packet| header_agrees(&packet),
        ),
        property(
            "telemetry time, CUC 4+2 without a P-field",
            2000,
            (
                spp::user_packet_fields(),
                any::<u32>(),
                any::<u16>(),
                octets(0..40),
            ),
            |(fields, coarse, fine, user)| time_agrees(fields, coarse, fine, &user),
        ),
    ]
}

/// Draws `cases` values from `strategy` with the fixed seed and checks each with `check`.
///
/// # Panics
/// If the strategy rejects too many values to produce one, which `aldis_testkit`'s strategies
/// do not.
fn property<S: Strategy>(
    name: &'static str,
    cases: usize,
    strategy: S,
    check: impl Fn(S::Value) -> Result<(), String>,
) -> PropertyReport {
    let mut runner = TestRunner::new_with_rng(
        Config::default(),
        TestRng::from_seed(RngAlgorithm::ChaCha, &SEED),
    );
    let mut report = PropertyReport {
        name,
        agreed: 0,
        disagreements: Vec::new(),
    };
    for index in 0..cases {
        let value = strategy
            .new_tree(&mut runner)
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .current();
        match check(value) {
            Ok(()) => report.agreed += 1,
            Err(diff) => report.disagreements.push(format!("case {index}: {diff}")),
        }
    }
    report
}

/// The fields cFE's setters take for `fields` with the Secondary Header Flag `secondary_header`.
const fn cfe_fields(fields: PacketFields, secondary_header: bool) -> Fields {
    Fields {
        version: 0,
        packet_type: match fields.packet_type {
            PacketType::Telemetry => 0,
            PacketType::Telecommand => 1,
        },
        secondary_header,
        apid: fields.apid.get(),
        sequence_flags: fields.sequence_flags.bits(),
        sequence_count: fields.sequence_count.get(),
    }
}

/// cFE reads the fields Aldis encoded, and builds the same octets from them.
fn header_agrees(packet: &spp::EncodedSpacePacket) -> Result<(), String> {
    let fields = cfe_fields(packet.fields, !packet.secondary_header.is_empty());
    let want = Header {
        version: fields.version,
        packet_type: fields.packet_type,
        secondary_header: fields.secondary_header,
        apid: fields.apid,
        sequence_flags: fields.sequence_flags,
        sequence_count: fields.sequence_count,
        size: packet.octets.len(),
    };
    let got = cfe_msg::read_header(&packet.octets).map_err(|e| format!("cFE returns {e}"))?;
    if got != want {
        return Err(format!("cFE reads {got:?}, Aldis encoded {want:?}"));
    }
    let built = cfe_msg::build(fields, &packet.data_field()).map_err(|e| e.to_string())?;
    if built != packet.octets {
        return Err(format!(
            "{fields:?}: cFE builds {} octets that differ from Aldis's {}",
            built.len(),
            packet.octets.len()
        ));
    }
    Ok(())
}

/// A telemetry packet whose secondary header is a CUC 4+2 time code without a P-field: cFE
/// reads the time Aldis wrote, and writes the same octets into a packet it built.
fn time_agrees(fields: PacketFields, coarse: u32, fine: u16, user: &[u8]) -> Result<(), String> {
    let fields = PacketFields {
        packet_type: PacketType::Telemetry,
        ..fields
    };
    let format = CucFormat::new(CucEpoch::Tai1958, 4, 2).map_err(|e| e.to_string())?;
    let cuc = Cuc::new(
        format,
        CucValue::new(u64::from(coarse), u128::from(fine) << 112),
    )
    .map_err(|e| e.to_string())?;
    let mut tfield = [0_u8; 6];
    TimeCode::Cuc(cuc)
        .encode(false, &mut tfield)
        .map_err(|e| e.to_string())?;
    let data = SppConfig::DEFAULT
        .packet(fields, &tfield, user)
        .map_err(|e| e.to_string())?;
    let mut octets = vec![0_u8; data.packet_len()];
    encode(&data, &mut octets).map_err(|e| e.to_string())?;
    let subseconds = u32::from(fine) << 16;
    let read = cfe_msg::read_time(&octets).map_err(|e| format!("cFE returns {e}"))?;
    if read != (coarse, subseconds) {
        return Err(format!(
            "cFE reads {read:?}, Aldis wrote ({coarse}, {subseconds:#010x})"
        ));
    }
    let data_field = [tfield.as_slice(), user].concat();
    let mut cleared = cfe_msg::build(cfe_fields(fields, true), &data_field)
        .map_err(|e| format!("cFE cannot build it: {e}"))?;
    if let Some(time) = cleared.get_mut(6..12) {
        time.fill(0);
    }
    let written = cfe_msg::write_time(&cleared, coarse, subseconds)
        .map_err(|e| format!("cFE returns {e}"))?;
    if written != octets {
        return Err(format!(
            "cFE writes the time ({coarse}, {subseconds:#010x}) differently from Aldis"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run;

    #[test]
    fn the_properties_hold() {
        // req: VER-040
        let reports = run();
        assert!(
            reports.iter().all(|r| r.disagreements.is_empty()),
            "{reports:?}"
        );
        assert_eq!(reports.iter().map(|r| r.agreed).sum::<usize>(), 4200);
    }
}
