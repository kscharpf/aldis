//! `FrameLayout::compute` either rejects the spec or returns contiguous ranges that cover the
//! frame exactly with a non-empty data field (CMN-040, CMN-041, API-070).
#![no_main]

// req: CMN-040, CMN-041, API-070, VER-022

use aldis_core::ErrorKind;
use aldis_core::layout::{FECF_LEN, FrameLayout, LayoutSpec, OCF_LEN};
use aldis_core::security::SecurityLayout;
use libfuzzer_sys::arbitrary::{Result, Unstructured};
use libfuzzer_sys::fuzz_target;

fn check(u: &mut Unstructured<'_>) -> Result<()> {
    // Mostly small values so that valid layouts are common, plus the occasional extreme.
    let size = |u: &mut Unstructured<'_>| -> Result<usize> {
        Ok(if u.ratio(1, 16)? {
            u.arbitrary()?
        } else {
            usize::from(u.arbitrary::<u16>()?)
        })
    };
    let frame_len = size(u)?;
    let primary_header_len = size(u)?;
    let secondary_len = size(u)?;
    let header = size(u)?;
    let trailer = size(u)?;
    let ocf: bool = u.arbitrary()?;
    let fecf: bool = u.arbitrary()?;

    let Ok(security) = SecurityLayout::new(header, trailer) else {
        assert!(header > 64 || trailer > 65_535);
        return Ok(());
    };
    assert_eq!(
        (security.header_len(), security.trailer_len()),
        (header, trailer)
    );
    assert_eq!(security.is_none(), header == 0 && trailer == 0);

    let spec = LayoutSpec {
        frame_len,
        primary_header_len,
        secondary_len,
        security,
        ocf,
        fecf,
    };
    let fixed = [
        Some(primary_header_len),
        Some(secondary_len),
        Some(header),
        Some(trailer),
        Some(if ocf { OCF_LEN } else { 0 }),
        Some(if fecf { FECF_LEN } else { 0 }),
    ]
    .into_iter()
    .try_fold(0_usize, |acc, n| n.and_then(|n| acc.checked_add(n)));
    let data_len = fixed
        .and_then(|f| frame_len.checked_sub(f))
        .filter(|&d| d > 0);

    match FrameLayout::compute(&spec) {
        Ok(layout) => {
            let data_len = data_len.expect("accepted a layout with no data octet");
            let ranges = layout.ranges();
            let mut at = 0;
            for r in &ranges {
                assert_eq!(r.start, at, "ranges are contiguous");
                assert!(r.end >= r.start);
                at = r.end;
            }
            assert_eq!(at, frame_len, "ranges cover the frame exactly");
            let lens: Vec<usize> = ranges.iter().map(|r| r.end - r.start).collect();
            assert_eq!(
                lens,
                [
                    primary_header_len,
                    secondary_len,
                    header,
                    data_len,
                    trailer,
                    if ocf { OCF_LEN } else { 0 },
                    if fecf { FECF_LEN } else { 0 },
                ]
            );
        }
        Err(e) => {
            assert!(data_len.is_none(), "rejected a valid layout");
            assert_eq!(e.kind(), ErrorKind::InvalidConfig);
        }
    }
    Ok(())
}

fuzz_target!(|data: &[u8]| {
    let _ = check(&mut Unstructured::new(data));
});
