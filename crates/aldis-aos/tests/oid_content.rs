//! A decoder does not verify the data of an OID frame (CMN-024). The OID sequence is the
//! transmitter's (CMN-020, CMN-021); a receiver that is handed an OID frame whose data is not the
//! sequence still decodes it as idle data.

use aldis_aos::{
    AosEdition, AosFrame, AosFrameFields, AosFrameWriter, AosPhysicalChannelConfig,
    AosVirtualChannelConfig, DataFieldContent, DataUnit, VcFrameCount, VcTable,
};
use aldis_core::ids::{SpacecraftId, VcId};
use aldis_core::oid::OidGenerator;

#[test]
fn an_oid_frame_with_data_that_is_not_the_sequence_decodes() {
    // req: CMN-024, AOS-016
    let pc = AosPhysicalChannelConfig::new(40, false, 0, false, AosEdition::Issue5).unwrap();
    let scid = SpacecraftId::new(0x42).unwrap();
    let vc =
        AosVirtualChannelConfig::new(scid, VcId::new(1).unwrap(), DataFieldContent::Mpdu).unwrap();
    let table = VcTable::<1>::new(pc, &[vc]).unwrap();
    let mut out = [0_u8; 40];
    let count = VcFrameCount::new(0).unwrap();
    let mut oid = OidGenerator::new();
    let written = AosFrameWriter::begin_oid(
        &mut out,
        &pc,
        &table,
        &AosFrameFields::oid(scid, count),
        &mut oid,
    )
    .unwrap()
    .unsecured()
    .unwrap()
    .finalize()
    .len();
    assert_eq!(written, 40);
    // The data field follows the 6-octet header; overwrite it with a pattern that is not the
    // sequence.
    for (i, octet) in out.iter_mut().skip(6).enumerate() {
        *octet = u8::try_from(i % 251)
            .unwrap()
            .wrapping_mul(7)
            .wrapping_add(1);
    }
    let frame = AosFrame::decode(&out, &pc, &table).unwrap();
    assert!(frame.is_oid());
    assert_eq!(frame.data_unit(), DataUnit::IdleData(&out[6..]));
}
