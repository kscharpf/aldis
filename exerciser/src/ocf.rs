//! Calls every public `aldis_core::ocf` API (VER-010; M2 WP-1) and runs every
//! `test-vectors/core/ocf.json` case (`vector`), as `crates/aldis-core/tests/vectors.rs` does.
//! Inputs pass through `black_box`.

use core::hint::black_box as bb;

use aldis_core::ErrorKind;
use aldis_core::field;
use aldis_core::ocf::{FsrReport, OCF_LEN, Ocf, ProjectSpecificReport, Type1Report};

use crate::exercise::Tally;
use crate::vectors::{Case, Fail, Out, Params as _, Run, need};

/// Every constructor, conversion, and accessor, on one octet string of each report type.
pub fn run(t: &mut Tally) {
    let clcw = [0x01, 0x14, 0x0C, 0x2A];
    let project = [0x80, 0x12, 0x34, 0x56];
    let fsr = [0xC1, 0x23, 0x45, 0x67];
    for octets in [clcw, project, fsr] {
        let ocf = Ocf::decode(bb(octets));
        t.check(ocf.to_octets() == octets && <[u8; OCF_LEN]>::from(ocf) == octets);
        t.check(Ocf::from(bb(octets)) == ocf);
    }
    t.check(matches!(Ocf::decode(bb(clcw)), Ocf::Clcw(r) if r.to_octets() == clcw));
    t.check(
        matches!(Ocf::decode(bb(project)), Ocf::ProjectSpecific(r) if r.to_octets() == project),
    );
    t.check(matches!(Ocf::decode(bb(fsr)), Ocf::SdlsFsr(r) if r.to_octets() == fsr));

    let type1 = Type1Report::new(bb(clcw));
    t.check(type1.is_some_and(|r| Ocf::from(r) == Ocf::decode(clcw)));
    t.check(Type1Report::new(bb(project)).is_none());
    t.check(Type1Report::try_from(bb(clcw)).map(<[u8; OCF_LEN]>::from) == Ok(clcw));
    t.check(Type1Report::try_from(bb(fsr)).is_err_and(|e| {
        e.kind() == ErrorKind::InvalidField && e.field() == Some(field::OCF_TYPE_FLAG)
    }));

    let ps = ProjectSpecificReport::new(bb(project));
    t.check(ps.is_some_and(|r| Ocf::from(r) == Ocf::decode(project) && r.to_octets() == project));
    t.check(ProjectSpecificReport::new(bb(fsr)).is_none());
    t.check(ProjectSpecificReport::try_from(bb(project)).map(<[u8; OCF_LEN]>::from) == Ok(project));
    t.check(
        ProjectSpecificReport::try_from(bb(clcw))
            .is_err_and(|e| e.field() == Some(field::OCF_TYPE_FLAG)),
    );
    t.check(
        ProjectSpecificReport::try_from(bb(fsr))
            .is_err_and(|e| e.field() == Some(field::OCF_REPORT_USE)),
    );

    let sdls = FsrReport::new(bb(fsr));
    t.check(sdls.is_some_and(|r| Ocf::from(r) == Ocf::decode(fsr) && r.to_octets() == fsr));
    t.check(FsrReport::new(bb(project)).is_none());
    t.check(FsrReport::try_from(bb(fsr)).map(<[u8; OCF_LEN]>::from) == Ok(fsr));
    t.check(FsrReport::try_from(bb(clcw)).is_err_and(|e| e.field() == Some(field::OCF_TYPE_FLAG)));
    t.check(
        FsrReport::try_from(bb(project)).is_err_and(|e| e.field() == Some(field::OCF_REPORT_USE)),
    );
}

/// The four OCF octets of a case.
fn octets(case: &Case) -> Result<[u8; OCF_LEN], Fail> {
    <[u8; OCF_LEN]>::try_from(case.input).map_err(|_| Fail::Vector)
}

/// Runs one `core/ocf.json` case.
pub fn vector(case: &Case, out: &mut Out) -> Run {
    let input = octets(case)?;
    match case.op {
        "ocf_classify" => {
            let ocf = Ocf::decode(bb(input));
            need((Ocf::from(input) == ocf).then_some(()))?;
            out.text(
                "report",
                match ocf {
                    Ocf::Clcw(_) => "clcw",
                    Ocf::ProjectSpecific(_) => "project_specific",
                    Ocf::SdlsFsr(_) => "sdls_fsr",
                },
            );
            out.bytes("octets_hex", &ocf.to_octets());
        }
        "ocf_report" => {
            let ocf = match case.params.text("report")? {
                "clcw" => Ocf::from(Type1Report::try_from(bb(input))?),
                "project_specific" => Ocf::from(ProjectSpecificReport::try_from(bb(input))?),
                "sdls_fsr" => Ocf::from(FsrReport::try_from(bb(input))?),
                _ => return Err(Fail::Vector),
            };
            need((Ocf::decode(input) == ocf).then_some(()))?;
            out.bytes("octets_hex", &ocf.to_octets());
        }
        _ => return Err(Fail::Vector),
    }
    Ok(())
}
