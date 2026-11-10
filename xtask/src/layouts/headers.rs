//! The headers to probe: for each, how the crate's public encoder writes it with one field
//! changed, and how its decoder rejects a fixed field (VER-072).
//!
//! The only hand-written input is the list of fields with their names, their catalogue rows,
//! the clause each cites, and a function that sets the field. No offset or width is written
//! here; the probe finds them.

use aldis_core::FieldId;

use super::probe::{Field, Header, How};

/// A field of a header, set by `set` on the parameters `p`: the value `base` is the reference.
fn encoded<P: Copy + 'static>(
    name: &'static str,
    id: Option<FieldId>,
    clause: &'static str,
    (p, base): (P, u64),
    set: fn(P, u64) -> P,
    encode: fn(P) -> Option<Vec<u8>>,
) -> Field {
    Field {
        name,
        id,
        clause,
        how: How::Encode {
            base,
            set: Box::new(move |v| encode(set(p, v))),
        },
    }
}

/// A fixed field whose bits the decoder names when it rejects them.
const fn rejected(name: &'static str, id: FieldId, clause: &'static str) -> Field {
    Field {
        name,
        id: Some(id),
        clause,
        how: How::Rejected,
    }
}

/// The field a failed decode names.
fn named<T>(result: Result<T, aldis_core::Error>) -> Option<FieldId> {
    result.err().and_then(|e| e.field())
}

/// `head` over the first octets of `full`.
fn patch(full: &[u8], head: &[u8]) -> Vec<u8> {
    let mut out = full.to_vec();
    for (d, s) in out.iter_mut().zip(head) {
        *d = *s;
    }
    out
}

/// `v` as a `u8` below `limit`.
fn small(v: u64, limit: u8) -> Option<u8> {
    u8::try_from(v).ok().filter(|v| *v < limit)
}

/// `v` as a `bool`: 0 or 1.
const fn flag(v: u64) -> Option<bool> {
    match v {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// The first `n` octets of `bytes`.
fn head(mut bytes: Vec<u8>, n: usize) -> Vec<u8> {
    bytes.truncate(n);
    bytes
}

/// The headers, in the order of the document.
pub fn all() -> Vec<Header> {
    let mut out = vec![
        spp::primary(),
        epp::header(epp::Len::One),
        epp::header(epp::Len::Two),
        epp::header(epp::Len::Four),
        epp::header(epp::Len::Eight),
        tm::primary(),
        tm::secondary_id(),
        tc::primary(),
        tc::segment(),
        tc::clcw(),
        aos::primary(false),
        aos::primary(true),
        uslp::primary(),
        uslp::truncated(),
    ];
    out.extend(time::all());
    out
}

mod spp {
    //! The Space Packet Primary Header (133.0 §4.1.3).

    use aldis_core::field;
    use aldis_core::ids::{Apid, SeqCount};
    use aldis_spp::{
        MAX_PACKET_LEN, PRIMARY_HEADER_LEN, PacketFields, PacketType, SequenceFlags, SpacePacket,
        SppConfig, encode,
    };

    use super::{Header, encoded, flag, named, patch, rejected, small};

    /// The values of the fields, as they are written.
    #[derive(Clone, Copy)]
    struct P {
        packet_type: u64,
        secondary: u64,
        apid: u64,
        flags: u64,
        count: u64,
        /// The Packet Data Length field: the data field length minus one.
        length: u64,
    }

    const BASE: P = P {
        packet_type: 0,
        secondary: 0,
        apid: 0,
        flags: 3,
        count: 0,
        length: 0,
    };

    /// The packet, whole.
    fn packet(p: P) -> Option<Vec<u8>> {
        let fields = PacketFields {
            packet_type: if flag(p.packet_type)? {
                PacketType::Telecommand
            } else {
                PacketType::Telemetry
            },
            apid: Apid::new(u16::try_from(p.apid).ok()?)?,
            sequence_flags: SequenceFlags::from_bits(small(p.flags, 4)?),
            sequence_count: SeqCount::new(u16::try_from(p.count).ok()?)?,
        };
        let data_len = usize::try_from(p.length).ok()?.checked_add(1)?;
        if data_len > 65_536 {
            return None;
        }
        let data = vec![0_u8; data_len];
        let (secondary, user) = data.split_at(usize::from(flag(p.secondary)?));
        let config = SppConfig::new(MAX_PACKET_LEN).ok()?;
        let packet = config.packet(fields, secondary, user).ok()?;
        let mut out = vec![0_u8; data_len + PRIMARY_HEADER_LEN];
        let n = encode(&packet, &mut out).ok()?;
        out.truncate(n);
        Some(out)
    }

    /// The header of the packet.
    fn header(p: P) -> Option<Vec<u8>> {
        packet(p).map(|v| super::head(v, PRIMARY_HEADER_LEN))
    }

    pub fn primary() -> Header {
        let baseline = packet(BASE).unwrap_or_default();
        let full = baseline.clone();
        Header {
            title: "Space Packet primary header",
            note: "Probed with `aldis_spp::encode`; the Packet Data Length is set through the \
length of the data field, the Secondary Header Flag through a secondary header",
            bits: PRIMARY_HEADER_LEN * 8,
            baseline: super::head(baseline, PRIMARY_HEADER_LEN),
            fields: vec![
                rejected(
                    "Packet Version Number",
                    field::PACKET_VERSION,
                    "133.0 §4.1.3.2",
                ),
                encoded(
                    "Packet Type",
                    None,
                    "133.0 §4.1.3.3.2",
                    (BASE, 0),
                    |p, v| P {
                        packet_type: v,
                        ..p
                    },
                    header,
                ),
                encoded(
                    "Secondary Header Flag",
                    Some(field::SECONDARY_HEADER_FLAG),
                    "133.0 §4.1.3.3.3",
                    (BASE, 0),
                    |p, v| P { secondary: v, ..p },
                    header,
                ),
                encoded(
                    "Application Process Identifier",
                    Some(field::APID),
                    "133.0 §4.1.3.3.4",
                    (BASE, 0),
                    |p, v| P { apid: v, ..p },
                    header,
                ),
                encoded(
                    "Sequence Flags",
                    None,
                    "133.0 §4.1.3.4.2",
                    (BASE, 3),
                    |p, v| P { flags: v, ..p },
                    header,
                ),
                encoded(
                    "Packet Sequence Count or Packet Name",
                    Some(field::SEQUENCE_COUNT),
                    "133.0 §4.1.3.4.3",
                    (BASE, 0),
                    |p, v| P { count: v, ..p },
                    header,
                ),
                encoded(
                    "Packet Data Length",
                    None,
                    "133.0 §4.1.3.5",
                    (BASE, 0),
                    |p, v| P { length: v, ..p },
                    header,
                ),
            ],
            reject: Box::new(move |h| named(SpacePacket::decode(&patch(&full, h)))),
        }
    }
}

mod tm {
    //! The TM Transfer Frame Primary Header and the Secondary Header Identification Field
    //! (132.0 §4.1.2, §4.1.3.2).

    use aldis_core::field;
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_tm::header::{FirstHeaderPointer, FshLen, SegmentLengthId, TmPrimaryHeader};
    use aldis_tm::{TmFrame, TmPhysicalChannelConfig};

    use super::{Field, Header, encoded, flag, named, rejected};

    /// The values of the fields.
    #[derive(Clone, Copy)]
    struct P {
        scid: u64,
        vcid: u64,
        ocf: u64,
        mc: u64,
        vc: u64,
        fsh: u64,
        sync: u64,
        order: u64,
        segment: u64,
        pointer: u64,
    }

    const BASE: P = P {
        scid: 0,
        vcid: 0,
        ocf: 0,
        mc: 0,
        vc: 0,
        fsh: 0,
        sync: 0,
        order: 0,
        segment: 0,
        pointer: 0,
    };

    /// The header as the crate writes it.
    fn octets(p: P) -> Option<Vec<u8>> {
        let header = TmPrimaryHeader {
            scid: SpacecraftId::new(u16::try_from(p.scid).ok()?)?,
            vcid: VcId::new(u8::try_from(p.vcid).ok()?)?,
            ocf_flag: flag(p.ocf)?,
            mc_frame_count: u8::try_from(p.mc).ok()?,
            vc_frame_count: u8::try_from(p.vc).ok()?,
            fsh_flag: flag(p.fsh)?,
            sync_flag: flag(p.sync)?,
            packet_order_flag: flag(p.order)?,
            segment_length_id: SegmentLengthId::new(u8::try_from(p.segment).ok()?)?,
            first_header_pointer: FirstHeaderPointer::new(u16::try_from(p.pointer).ok()?)?,
        };
        Some(header.to_octets().to_vec())
    }

    pub fn primary() -> Header {
        Header {
            title: "TM Transfer Frame primary header",
            note: "Probed with `TmPrimaryHeader::to_octets`",
            bits: 48,
            baseline: octets(BASE).unwrap_or_default(),
            fields: vec![
                rejected(
                    "Transfer Frame Version Number",
                    field::FRAME_VERSION,
                    "132.0 §4.1.2.2.2",
                ),
                encoded(
                    "Spacecraft Identifier",
                    Some(field::SPACECRAFT_ID),
                    "132.0 §4.1.2.2.3",
                    (BASE, 0),
                    |p, v| P { scid: v, ..p },
                    octets,
                ),
                encoded(
                    "Virtual Channel Identifier",
                    Some(field::VC_ID),
                    "132.0 §4.1.2.3",
                    (BASE, 0),
                    |p, v| P { vcid: v, ..p },
                    octets,
                ),
                encoded(
                    "Operational Control Field Flag",
                    Some(field::OCF_FLAG),
                    "132.0 §4.1.2.4",
                    (BASE, 0),
                    |p, v| P { ocf: v, ..p },
                    octets,
                ),
                encoded(
                    "Master Channel Frame Count",
                    None,
                    "132.0 §4.1.2.5",
                    (BASE, 0),
                    |p, v| P { mc: v, ..p },
                    octets,
                ),
                encoded(
                    "Virtual Channel Frame Count",
                    None,
                    "132.0 §4.1.2.6",
                    (BASE, 0),
                    |p, v| P { vc: v, ..p },
                    octets,
                ),
                encoded(
                    "Transfer Frame Secondary Header Flag",
                    Some(field::FSH_FLAG),
                    "132.0 §4.1.2.7.2",
                    (BASE, 0),
                    |p, v| P { fsh: v, ..p },
                    octets,
                ),
                encoded(
                    "Synchronization Flag",
                    Some(field::SYNC_FLAG),
                    "132.0 §4.1.2.7.3",
                    (BASE, 0),
                    |p, v| P { sync: v, ..p },
                    octets,
                ),
                encoded(
                    "Packet Order Flag",
                    Some(field::PACKET_ORDER_FLAG),
                    "132.0 §4.1.2.7.4",
                    (BASE, 0),
                    |p, v| P { order: v, ..p },
                    octets,
                ),
                encoded(
                    "Segment Length Identifier",
                    Some(field::SEGMENT_LENGTH_ID),
                    "132.0 §4.1.2.7.5",
                    (BASE, 0),
                    |p, v| P { segment: v, ..p },
                    octets,
                ),
                encoded(
                    "First Header Pointer",
                    Some(field::FIRST_HEADER_POINTER),
                    "132.0 §4.1.2.7.6",
                    (BASE, 0),
                    |p, v| P { pointer: v, ..p },
                    octets,
                ),
            ],
            reject: Box::new(|h| named(TmPrimaryHeader::decode(h))),
        }
    }

    /// The Identification Field for a Secondary Header of `v + 1` octets.
    fn id_octet(v: u64) -> Option<Vec<u8>> {
        let len = FshLen::new(u8::try_from(v.checked_add(1)?).ok()?)?;
        Some(vec![len.id_octet()])
    }

    pub fn secondary_id() -> Header {
        let pc = TmPhysicalChannelConfig::new(16, false);
        // A frame with the Secondary Header Flag, the Identification Field, one octet of
        // secondary header data, and the data field.
        let frame = |id: &[u8]| {
            let mut bytes = octets(P {
                fsh: 1,
                segment: 3,
                ..BASE
            })
            .unwrap_or_default();
            bytes.extend_from_slice(id);
            bytes.resize(16, 0);
            bytes
        };
        Header {
            title: "TM Transfer Frame Secondary Header identification field",
            note: "Probed with `FshLen::id_octet`; the decoder is `TmFrame::decode` on a frame \
whose Secondary Header Flag is set",
            bits: 8,
            baseline: vec![FshLen::new(2).map_or(0, FshLen::id_octet)],
            fields: vec![
                rejected(
                    "Transfer Frame Secondary Header Version Number",
                    field::FSH_VERSION,
                    "132.0 §4.1.3.2.2",
                ),
                Field {
                    name: "Transfer Frame Secondary Header Length",
                    id: Some(field::FSH_LEN),
                    clause: "132.0 §4.1.3.2.3",
                    how: super::How::Encode {
                        base: 1,
                        set: Box::new(id_octet),
                    },
                },
            ],
            reject: Box::new(move |h| {
                let bytes = frame(h);
                let decoded = pc.and_then(|pc| TmFrame::decode(&bytes, &pc).map(|_| ()));
                named(decoded)
            }),
        }
    }
}

mod epp {
    //! The Encapsulation Packet header in each of its four lengths (133.1 §4.1.2).

    use aldis_core::field;
    use aldis_epp::config::MAX_DATA_LEN;
    use aldis_epp::header::{Epi, EpiExtension, HeaderLen, HeaderSize, UserDefined};
    use aldis_epp::{EncapsulationFields, EncapsulationPacket, EpiSet, EppConfig, ExtendedEpiSet};

    use super::{Field, Header, How, encoded, named, patch, rejected};

    /// The length of the header.
    #[derive(Clone, Copy)]
    pub enum Len {
        One,
        Two,
        Four,
        Eight,
    }

    impl Len {
        const fn header_len(self) -> HeaderLen {
            match self {
                Len::One => HeaderLen::One,
                Len::Two => HeaderLen::Two,
                Len::Four => HeaderLen::Four,
                Len::Eight => HeaderLen::Eight,
            }
        }
    }

    /// The values of the fields, and the header length the encoder is asked for.
    #[derive(Clone, Copy)]
    struct P {
        len: HeaderLen,
        epi: u64,
        extension: u64,
        user: u64,
        /// The Packet Length field: the length of the whole packet.
        total: u64,
    }

    /// The header, as the encoder writes it for a data field that makes up the total length.
    fn octets(p: P) -> Option<Vec<u8>> {
        let config = EppConfig::new(
            0,
            MAX_DATA_LEN,
            EpiSet::from_bits(u8::MAX),
            ExtendedEpiSet::from_bits(u16::MAX),
        )
        .ok()?;
        let fields = EncapsulationFields::from_parts(
            Epi::new(u8::try_from(p.epi).ok()?)?,
            EpiExtension::new(u8::try_from(p.extension).ok()?)?,
            UserDefined::new(u8::try_from(p.user).ok()?)?,
        )
        .ok()?;
        let data_len = usize::try_from(p.total).ok()?.checked_sub(p.len.octets())?;
        let header = config
            .header(fields, HeaderSize::Fixed(p.len), data_len)
            .ok()?;
        let mut out = [0_u8; 8];
        let n = header.encode(&mut out).ok()?;
        Some(out.get(..n)?.to_vec())
    }

    #[expect(
        clippy::too_many_lines,
        reason = "a table of the fields of one header, with nothing to factor out"
    )]
    pub fn header(len: Len) -> Header {
        let hl = len.header_len();
        let n = hl.octets();
        // The idle packet: any total length from the header on is valid.
        let idle = P {
            len: hl,
            epi: 0,
            extension: 0,
            user: 0,
            total: if n == 1 {
                1
            } else {
                u64::try_from(n).unwrap_or(0) + 1
            },
        };
        let extended = P { epi: 6, ..idle };
        let plain = P { epi: 1, ..idle };
        let full = {
            let mut b = octets(idle).unwrap_or_default();
            b.resize(usize::try_from(idle.total).unwrap_or(0), 0);
            b
        };
        let title = match len {
            Len::One => "Encapsulation Packet header, 1 octet",
            Len::Two => "Encapsulation Packet header, 2 octets",
            Len::Four => "Encapsulation Packet header, 4 octets",
            Len::Eight => "Encapsulation Packet header, 8 octets",
        };
        let mut fields = vec![rejected(
            "Packet Version Number",
            field::PACKET_VERSION,
            "133.1 §4.1.2.2",
        )];
        fields.push(match len {
            Len::One => rejected("Encapsulation Protocol ID", field::EPI, "133.1 §4.1.2.3"),
            Len::Two => encoded(
                "Encapsulation Protocol ID",
                Some(field::EPI),
                "133.1 §4.1.2.3",
                (plain, 1),
                |p, v| P { epi: v, ..p },
                octets,
            ),
            Len::Four | Len::Eight => encoded(
                "Encapsulation Protocol ID",
                Some(field::EPI),
                "133.1 §4.1.2.3",
                (extended, 6),
                |p, v| P { epi: v, ..p },
                octets,
            ),
        });
        // The Length of Length is in the first octet of every header: it is probed by asking
        // for another header length and keeping that octet.
        let reference = octets(idle).unwrap_or_default();
        fields.push(Field {
            name: "Length of Length",
            id: None,
            clause: "133.1 §4.1.2.4",
            how: How::Encode {
                base: u64::from(hl.length_of_length()),
                set: Box::new(move |v| {
                    let other =
                        HeaderLen::from_length_of_length(u8::try_from(v).ok().filter(|v| *v < 4)?);
                    let total = u64::try_from(other.octets()).ok()?.checked_add(1)?;
                    let total = if matches!(other, HeaderLen::One) {
                        1
                    } else {
                        total
                    };
                    let first = *octets(P {
                        len: other,
                        total,
                        ..idle
                    })?
                    .first()?;
                    let mut out = reference.clone();
                    *out.first_mut()? = first;
                    Some(out)
                }),
            },
        });
        if matches!(len, Len::Four | Len::Eight) {
            fields.push(encoded(
                "User Defined",
                Some(field::USER_DEFINED),
                "133.1 §4.1.2.5",
                (extended, 0),
                |p, v| P { user: v, ..p },
                octets,
            ));
            fields.push(encoded(
                "Protocol ID Extension",
                Some(field::EPI_EXTENSION),
                "133.1 §4.1.2.6",
                (extended, 0),
                |p, v| P { extension: v, ..p },
                octets,
            ));
        }
        if matches!(len, Len::Eight) {
            fields.push(rejected(
                "CCSDS Defined Field",
                field::CCSDS_DEFINED,
                "133.1 §4.1.2.7",
            ));
        }
        if !matches!(len, Len::One) {
            fields.push(encoded(
                "Packet Length",
                Some(field::PACKET_LEN),
                "133.1 §4.1.3",
                (idle, idle.total),
                |p, v| P { total: v, ..p },
                octets,
            ));
        }
        Header {
            title,
            note: "Probed with `EppConfig::header` and `EncapsulationHeader::encode`, one \
header length at a time (`HeaderSize::Fixed`)",
            bits: n * 8,
            baseline: octets(idle).unwrap_or_default(),
            fields,
            reject: Box::new(move |h| named(EncapsulationPacket::decode(&patch(&full, h)))),
        }
    }
}

mod tc {
    //! The TC Transfer Frame Primary Header and Segment Header, and the CLCW (232.0 §4.1.2,
    //! §4.1.3.2.2, §4.2).

    use aldis_core::field;
    use aldis_core::ids::FrameSeqNum;
    use aldis_core::lookup::{ChannelLookup, MapLookup, PhysicalLookup};
    use aldis_core::security::SecurityLayout;
    use aldis_tc::config::{MapConfig, PhysicalChannelConfig, SegmentHeaderUse, VcConfig};
    use aldis_tc::header::{
        FrameFields, FrameType, SegmentHeader, SequenceFlags, TcMapId, TcSpacecraftId, TcVcId,
    };
    use aldis_tc::writer::{FrameData, encode};
    use aldis_tc::{
        Clcw, ClcwStatus, ControlCommand, CopInEffect, FarmBCounter, PRIMARY_HEADER_LEN, TcFrame,
    };

    use super::{Header, encoded, flag, head, named, patch, rejected, small};

    /// A Physical Channel whose Virtual Channels are all valid, with no Segment Header.
    struct AnyChannel;

    impl PhysicalLookup for AnyChannel {
        type Physical = PhysicalChannelConfig;

        fn physical_channel(&self) -> PhysicalChannelConfig {
            PhysicalChannelConfig::new(1024, false)
                .unwrap_or_else(|_| unreachable!("1024 is a valid maximum frame length"))
        }
    }

    impl ChannelLookup<TcSpacecraftId, TcVcId> for AnyChannel {
        type Vc = VcConfig;

        fn has_spacecraft(&self, _: TcSpacecraftId) -> bool {
            true
        }

        fn vc(&self, spacecraft_id: TcSpacecraftId, vc_id: TcVcId) -> Option<VcConfig> {
            VcConfig::new(
                spacecraft_id,
                vc_id,
                1024,
                SegmentHeaderUse::Absent(SecurityLayout::NONE),
            )
            .ok()
        }
    }

    impl MapLookup<TcSpacecraftId, TcVcId, TcMapId> for AnyChannel {
        type Map = MapConfig;

        fn map(&self, _: TcSpacecraftId, _: TcVcId, _: TcMapId) -> Option<MapConfig> {
            None
        }
    }

    /// The values of the fields.
    #[derive(Clone, Copy)]
    struct P {
        bypass: u64,
        control: u64,
        scid: u64,
        vcid: u64,
        /// The Frame Length field: the length of the frame minus one.
        length: u64,
        sequence: u64,
    }

    /// A Type-BD frame of the shortest length.
    const BASE: P = P {
        bypass: 1,
        control: 0,
        scid: 0,
        vcid: 0,
        length: 5,
        sequence: 0,
    };

    /// The frame, whole.
    fn frame(p: P) -> Option<Vec<u8>> {
        let frame_type = FrameType::from_flags(flag(p.bypass)?, flag(p.control)?)?;
        let fields = FrameFields {
            frame_type,
            spacecraft_id: TcSpacecraftId::new(u16::try_from(p.scid).ok()?)?,
            vc_id: TcVcId::new(u8::try_from(p.vcid).ok()?)?,
            sequence_number: FrameSeqNum::new(u8::try_from(p.sequence).ok()?),
        };
        let len = usize::try_from(p.length).ok()?.checked_add(1)?;
        if len > 1024 {
            return None;
        }
        let user = vec![0_u8; len.saturating_sub(PRIMARY_HEADER_LEN)];
        let data = if matches!(frame_type, FrameType::Bc) {
            FrameData::Control(ControlCommand::Unlock)
        } else {
            FrameData::Fdu {
                segment_header: None,
                user_data: &user,
            }
        };
        let mut out = vec![0_u8; len];
        let n = encode(&AnyChannel, fields, data, &mut out).ok()?;
        out.truncate(n);
        Some(out)
    }

    fn octets(p: P) -> Option<Vec<u8>> {
        frame(p).map(|f| head(f, PRIMARY_HEADER_LEN))
    }

    pub fn primary() -> Header {
        let baseline = frame(BASE).unwrap_or_default();
        let full = baseline.clone();
        Header {
            title: "TC Transfer Frame primary header",
            note: "Probed with `aldis_tc::writer::encode` on a Physical Channel without an \
FECF; the Frame Length is set through the length of the data field",
            bits: PRIMARY_HEADER_LEN * 8,
            baseline: head(baseline, PRIMARY_HEADER_LEN),
            fields: vec![
                rejected(
                    "Transfer Frame Version Number",
                    field::FRAME_VERSION,
                    "232.0 §4.1.2.2",
                ),
                encoded(
                    "Bypass Flag",
                    None,
                    "232.0 §4.1.2.3",
                    (BASE, 1),
                    |p, v| P { bypass: v, ..p },
                    octets,
                ),
                encoded(
                    "Control Command Flag",
                    Some(field::CONTROL_COMMAND_FLAG),
                    "232.0 §4.1.2.3",
                    (BASE, 0),
                    |p, v| P { control: v, ..p },
                    octets,
                ),
                rejected("Reserved Spare", field::TC_HEADER_SPARE, "232.0 §4.1.2.4"),
                encoded(
                    "Spacecraft Identifier",
                    Some(field::SPACECRAFT_ID),
                    "232.0 §4.1.2.5",
                    (BASE, 0),
                    |p, v| P { scid: v, ..p },
                    octets,
                ),
                encoded(
                    "Virtual Channel Identifier",
                    Some(field::VC_ID),
                    "232.0 §4.1.2.6",
                    (BASE, 0),
                    |p, v| P { vcid: v, ..p },
                    octets,
                ),
                encoded(
                    "Frame Length",
                    Some(field::FRAME_LEN),
                    "232.0 §4.1.2.7",
                    (BASE, 5),
                    |p, v| P { length: v, ..p },
                    octets,
                ),
                encoded(
                    "Frame Sequence Number",
                    None,
                    "232.0 §4.1.2.8",
                    (BASE, 0),
                    |p, v| P { sequence: v, ..p },
                    octets,
                ),
            ],
            reject: Box::new(move |h| named(TcFrame::decode(&patch(&full, h), &AnyChannel))),
        }
    }

    /// The Segment Header octet.
    #[derive(Clone, Copy)]
    struct S {
        flags: u64,
        map: u64,
    }

    fn segment_octet(s: S) -> Option<Vec<u8>> {
        let header = SegmentHeader {
            sequence_flags: SequenceFlags::from_bits(small(s.flags, 4)?),
            map_id: TcMapId::new(u8::try_from(s.map).ok()?)?,
        };
        Some(vec![header.octet()])
    }

    pub fn segment() -> Header {
        let base = S { flags: 0, map: 0 };
        Header {
            title: "TC Segment Header",
            note: "Probed with `SegmentHeader::octet`",
            bits: 8,
            baseline: vec![0],
            fields: vec![
                encoded(
                    "Sequence Flags",
                    None,
                    "232.0 §4.1.3.2.2.2",
                    (base, 0),
                    |s, v| S { flags: v, ..s },
                    segment_octet,
                ),
                encoded(
                    "MAP Identifier",
                    Some(field::MAP_ID),
                    "232.0 §4.1.3.2.2.3",
                    (base, 0),
                    |s, v| S { map: v, ..s },
                    segment_octet,
                ),
            ],
            reject: Box::new(|_| None),
        }
    }

    /// The values of the fields of a CLCW.
    #[derive(Clone, Copy)]
    struct C {
        status: u64,
        cop: u64,
        vc: u64,
        no_rf: u64,
        no_lock: u64,
        lockout: u64,
        wait: u64,
        retransmit: u64,
        farm_b: u64,
        report: u64,
    }

    const CLCW_BASE: C = C {
        status: 0,
        cop: 0,
        vc: 0,
        no_rf: 0,
        no_lock: 0,
        lockout: 0,
        wait: 0,
        retransmit: 0,
        farm_b: 0,
        report: 0,
    };

    fn clcw_octets(c: C) -> Option<Vec<u8>> {
        let clcw = Clcw {
            status: ClcwStatus::new(u8::try_from(c.status).ok()?)?,
            cop_in_effect: CopInEffect::new(u8::try_from(c.cop).ok()?)?,
            vc_id: TcVcId::new(u8::try_from(c.vc).ok()?)?,
            no_rf_available: flag(c.no_rf)?,
            no_bit_lock: flag(c.no_lock)?,
            lockout: flag(c.lockout)?,
            wait: flag(c.wait)?,
            retransmit: flag(c.retransmit)?,
            farm_b_counter: FarmBCounter::new(u8::try_from(c.farm_b).ok()?)?,
            report_value: FrameSeqNum::new(u8::try_from(c.report).ok()?),
        };
        Some(clcw.to_octets().to_vec())
    }

    pub fn clcw() -> Header {
        let b = CLCW_BASE;
        let e = |name, id, clause, set: fn(C, u64) -> C| {
            encoded(name, id, clause, (b, 0), set, clcw_octets)
        };
        Header {
            title: "Communications Link Control Word",
            note: "Probed with `Clcw::to_octets`; the decoder is `Clcw::decode`",
            bits: 32,
            baseline: clcw_octets(b).unwrap_or_default(),
            fields: vec![
                rejected(
                    "Control Word Type",
                    field::CONTROL_WORD_TYPE,
                    "232.0 §4.2.1.2",
                ),
                rejected("CLCW Version Number", field::CLCW_VERSION, "232.0 §4.2.1.3"),
                e(
                    "Status Field",
                    Some(field::CLCW_STATUS),
                    "232.0 §4.2.1.4",
                    |c, v| C { status: v, ..c },
                ),
                e(
                    "COP in Effect",
                    Some(field::COP_IN_EFFECT),
                    "232.0 §4.2.1.5",
                    |c, v| C { cop: v, ..c },
                ),
                e(
                    "Virtual Channel Identification",
                    Some(field::VC_ID),
                    "232.0 §4.2.1.6",
                    |c, v| C { vc: v, ..c },
                ),
                rejected(
                    "Reserved Spare, bits 14–15",
                    field::CLCW_SPARE_14_15,
                    "232.0 §4.2.1.7",
                ),
                e("No RF Available Flag", None, "232.0 §4.2.1.8.2", |c, v| {
                    C { no_rf: v, ..c }
                }),
                e("No Bit Lock Flag", None, "232.0 §4.2.1.8.3", |c, v| C {
                    no_lock: v,
                    ..c
                }),
                e("Lockout Flag", None, "232.0 §4.2.1.8.4", |c, v| C {
                    lockout: v,
                    ..c
                }),
                e("Wait Flag", None, "232.0 §4.2.1.8.5", |c, v| C {
                    wait: v,
                    ..c
                }),
                e("Retransmit Flag", None, "232.0 §4.2.1.8.6", |c, v| C {
                    retransmit: v,
                    ..c
                }),
                e(
                    "FARM-B Counter",
                    Some(field::FARM_B_COUNTER),
                    "232.0 §4.2.1.9",
                    |c, v| C { farm_b: v, ..c },
                ),
                rejected(
                    "Reserved Spare, bit 23",
                    field::CLCW_SPARE_23,
                    "232.0 §4.2.1.10",
                ),
                e("Report Value", None, "232.0 §4.2.1.11", |c, v| C {
                    report: v,
                    ..c
                }),
            ],
            reject: Box::new(|h| {
                let octets: Option<[u8; 4]> = h.try_into().ok();
                octets.and_then(|o| named(Clcw::decode(o)))
            }),
        }
    }
}

mod aos {
    //! The AOS Transfer Frame Primary Header (732.0 §4.1.2), in the edition of Issue 5.

    use aldis_aos::config::{AosEdition, AosVirtualChannelConfig, DataFieldContent};
    use aldis_aos::{
        AosFrameContent, AosFrameFields, AosPhysicalChannelConfig, AosPrimaryHeader,
        FrameCountCycle, PduHeader, VcFrameCount, encode,
    };
    use aldis_core::field;
    use aldis_core::ids::{SpacecraftId, VcId};
    use aldis_core::lookup::ChannelLookup;

    use super::{Field, Header, How, encoded, flag, head, named, rejected};

    /// A Physical Channel whose Virtual Channels all carry a `VCA_SDU`.
    struct AnyChannel;

    impl ChannelLookup<SpacecraftId<10>, VcId<6>> for AnyChannel {
        type Vc = AosVirtualChannelConfig;

        fn has_spacecraft(&self, _: SpacecraftId<10>) -> bool {
            true
        }

        fn vc(&self, scid: SpacecraftId<10>, vcid: VcId<6>) -> Option<AosVirtualChannelConfig> {
            AosVirtualChannelConfig::new(scid, vcid, DataFieldContent::VcaSdu).ok()
        }
    }

    /// The values of the fields.
    #[derive(Clone, Copy)]
    struct P {
        fhec: bool,
        scid: u64,
        vcid: u64,
        count: u64,
        replay: u64,
        /// 0 for no cycle (Use Flag 0), else the cycle plus one.
        cycle: u64,
    }

    const FRAME_LEN: usize = 16;

    fn physical(fhec: bool) -> Option<AosPhysicalChannelConfig> {
        AosPhysicalChannelConfig::new(FRAME_LEN, fhec, 0, false, AosEdition::Issue5).ok()
    }

    fn frame(p: P) -> Option<Vec<u8>> {
        let pc = physical(p.fhec)?;
        let cycle = match p.cycle {
            0 => None,
            c => Some(FrameCountCycle::new(u8::try_from(c.checked_sub(1)?).ok()?)?),
        };
        let fields = AosFrameFields {
            replay_flag: flag(p.replay)?,
            cycle,
            ..AosFrameFields::new(
                SpacecraftId::new(u16::try_from(p.scid).ok()?)?,
                VcId::new(u8::try_from(p.vcid).ok()?)?,
                VcFrameCount::new(u32::try_from(p.count).ok()?)?,
                PduHeader::None,
            )
        };
        let zone = vec![0_u8; FRAME_LEN.checked_sub(pc.primary_header_len())?];
        let mut out = vec![0_u8; FRAME_LEN];
        let n = encode(
            &pc,
            &AnyChannel,
            &fields,
            &AosFrameContent::new(&zone),
            &mut out,
        )
        .ok()?;
        out.truncate(n);
        Some(out)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "a table of the fields of one header, with nothing to factor out"
    )]
    pub fn primary(fhec: bool) -> Header {
        let base = P {
            fhec,
            scid: 0,
            vcid: 0,
            count: 0,
            replay: 0,
            cycle: 0,
        };
        let len = if fhec { 8 } else { 6 };
        // `fn` pointers cannot capture `len`: one function for each variant.
        let octets: fn(P) -> Option<Vec<u8>> = if fhec {
            |p| frame(p).map(|f| head(f, 8))
        } else {
            |p| frame(p).map(|f| head(f, 6))
        };
        let mut fields = vec![
            rejected(
                "Transfer Frame Version Number",
                field::FRAME_VERSION,
                "732.0 §4.1.2.2.2",
            ),
            encoded(
                "Spacecraft Identifier",
                Some(field::SPACECRAFT_ID),
                "732.0 §4.1.2.2.3",
                (base, 0),
                |p, v| P { scid: v, ..p },
                octets,
            ),
            encoded(
                "Virtual Channel Identifier",
                Some(field::VC_ID),
                "732.0 §4.1.2.3",
                (base, 0),
                |p, v| P { vcid: v, ..p },
                octets,
            ),
            encoded(
                "Virtual Channel Frame Count",
                Some(field::VCF_COUNT),
                "732.0 §4.1.2.4",
                (base, 0),
                |p, v| P { count: v, ..p },
                octets,
            ),
            encoded(
                "Replay Flag",
                None,
                "732.0 §4.1.2.5.2",
                (base, 0),
                |p, v| P { replay: v, ..p },
                octets,
            ),
            Field {
                name: "VC Frame Count Cycle Use Flag",
                id: None,
                clause: "732.0 §4.1.2.5.3",
                how: How::Encode {
                    base: 0,
                    set: Box::new(move |v| {
                        octets(P {
                            cycle: flag(v)?.into(),
                            ..base
                        })
                    }),
                },
            },
            encoded(
                "VC Frame Count Cycle",
                Some(field::VC_FRAME_COUNT_CYCLE),
                "732.0 §4.1.2.5.5",
                (P { cycle: 1, ..base }, 0),
                |p, v| P {
                    cycle: v.saturating_add(1),
                    ..p
                },
                octets,
            ),
        ];
        if fhec {
            fields.push(Field {
                name: "Frame Header Error Control",
                id: None,
                clause: "732.0 §4.1.2.6",
                how: How::Derived { from_bit: 48 },
            });
        }
        Header {
            title: if fhec {
                "AOS Transfer Frame primary header, with the Frame Header Error Control"
            } else {
                "AOS Transfer Frame primary header"
            },
            note: if fhec {
                "Probed with `aldis_aos::encode` on a Physical Channel with the FHEC, which the \
writer computes from the other fields (Issue 5 edition)"
            } else {
                "Probed with `aldis_aos::encode` on a Physical Channel without the FHEC (Issue 5 \
edition: the two most significant SCID bits are bits 42–43)"
            },
            bits: len * 8,
            baseline: octets(base).unwrap_or_default(),
            fields,
            // The FHEC would correct the flipped bit: the decoder reads the first six octets.
            reject: Box::new(|h| {
                let pc = physical(false)?;
                named(AosPrimaryHeader::decode(h.get(..6)?, &pc))
            }),
        }
    }
}

mod uslp {
    //! The USLP Transfer Frame Primary Header, non-truncated and truncated (732.1 §4.1.2,
    //! Annex D1.3).

    use aldis_core::field;
    use aldis_core::ids::MapId;
    use aldis_core::lookup::{ChannelLookup, MapLookup, PhysicalLookup};
    use aldis_core::ocf::Ocf;
    use aldis_core::security::SecurityLayout;
    use aldis_uslp::config::TransferFrameType;
    use aldis_uslp::writer::{FrameContent, FrameFields, encode, encode_truncated};
    use aldis_uslp::{
        ChannelFields, FrameKind, MapConfig, OcfUse, PhysicalChannelConfig, SecurityScope,
        SourceOrDestination, TfdfHeader, TfdzRule, TruncatedFrameLen, Upid, UslpFrame, UslpMapId,
        UslpSpacecraftId, UslpVcId, VcConfig, VcParams, VcfCountLen,
    };

    use super::{Field, Header, How, encoded, flag, head, named, patch, rejected};

    /// The octets of the longest VCF Count.
    const VCF_LEN: u8 = 7;
    /// The truncated frame length of a truncated Virtual Channel.
    const TRUNCATED_LEN: usize = 8;

    /// A variable-length Physical Channel whose Virtual Channels are all valid.
    struct AnyChannel {
        truncated: bool,
        vcf_len: u8,
    }

    impl PhysicalLookup for AnyChannel {
        type Physical = PhysicalChannelConfig;

        fn physical_channel(&self) -> PhysicalChannelConfig {
            PhysicalChannelConfig::new(TransferFrameType::Variable, 65_536, 0, false)
                .unwrap_or_else(|_| unreachable!("65536 is a valid maximum frame length"))
        }
    }

    impl ChannelLookup<UslpSpacecraftId, UslpVcId> for AnyChannel {
        type Vc = VcConfig;

        fn has_spacecraft(&self, _: UslpSpacecraftId) -> bool {
            true
        }

        fn vc(&self, scid: UslpSpacecraftId, vcid: UslpVcId) -> Option<VcConfig> {
            let len = VcfCountLen::new(self.vcf_len).ok()?;
            let params = VcParams {
                frame_type: TransferFrameType::Variable,
                vcf_count_len_sequence: len,
                vcf_count_len_expedited: len,
                ocf: OcfUse::Allowed,
                truncated_frame_len: if self.truncated {
                    Some(TruncatedFrameLen::new(TRUNCATED_LEN).ok()?)
                } else {
                    None
                },
                security: SecurityScope::Vc(SecurityLayout::NONE),
            };
            VcConfig::new(scid, vcid, params).ok()
        }
    }

    impl MapLookup<UslpSpacecraftId, UslpVcId, UslpMapId> for AnyChannel {
        type Map = MapConfig;

        fn map(&self, scid: UslpSpacecraftId, vcid: UslpVcId, map: UslpMapId) -> Option<MapConfig> {
            Some(MapConfig::new(scid, vcid, map, SecurityLayout::NONE))
        }
    }

    /// The values of the fields.
    #[derive(Clone, Copy)]
    struct P {
        scid: u64,
        destination: u64,
        vcid: u64,
        map: u64,
        /// The Frame Length field: the length of the frame minus one.
        length: u64,
        bypass: u64,
        protocol: u64,
        ocf: u64,
        vcf_len: u64,
        count: u64,
        truncated: u64,
    }

    const BASE: P = P {
        scid: 0,
        destination: 0,
        vcid: 0,
        map: 0,
        length: 19,
        bypass: 1,
        protocol: 0,
        ocf: 0,
        vcf_len: VCF_LEN as u64,
        count: 0,
        truncated: 0,
    };

    /// The frame, whole.
    fn frame(p: P) -> Option<Vec<u8>> {
        let vcf_len = u8::try_from(p.vcf_len).ok()?;
        let channels = AnyChannel {
            truncated: flag(p.truncated)?,
            vcf_len,
        };
        let channel = ChannelFields {
            spacecraft_id: UslpSpacecraftId::new(u16::try_from(p.scid).ok()?)?,
            source_or_destination: SourceOrDestination::from_bit(flag(p.destination)?),
            vc_id: UslpVcId::new(u8::try_from(p.vcid).ok()?)?,
            map_id: MapId::new(u8::try_from(p.map).ok()?)?,
        };
        let tfdf = TfdfHeader {
            rule: TfdzRule::NoSegmentation,
            upid: Upid::OnlyIdleData,
        };
        let mut out = vec![0_u8; 65_536];
        if channels.truncated {
            let room = [0_u8; TRUNCATED_LEN - 5];
            let n = encode_truncated(&channels, channel, tfdf, &room, &mut out).ok()?;
            out.truncate(n);
            return Some(out);
        }
        let ocf = flag(p.ocf)?;
        let len = usize::try_from(p.length).ok()?.checked_add(1)?;
        if len > 65_536 {
            return None;
        }
        // The header, the one-octet TFDF Header, the TFDZ, and the OCF.
        let overhead = 7_usize + usize::from(vcf_len) + 1 + if ocf { 4 } else { 0 };
        let zone = vec![0_u8; len.checked_sub(overhead).filter(|n| *n >= 1)?];
        let fields = FrameFields {
            spacecraft_id: channel.spacecraft_id,
            source_or_destination: channel.source_or_destination,
            vc_id: channel.vc_id,
            map_id: channel.map_id,
            kind: FrameKind::from_flags(flag(p.bypass)?, flag(p.protocol)?)?,
            vcf_count: p.count,
        };
        let content = FrameContent {
            tfdf,
            insert_zone: &[],
            ocf: ocf.then(|| Ocf::decode([0; 4])),
        };
        let n = encode(&channels, fields, content, &zone, &mut out).ok()?;
        out.truncate(n);
        Some(out)
    }

    fn octets(p: P) -> Option<Vec<u8>> {
        frame(p).map(|f| head(f, 7 + usize::from(VCF_LEN)))
    }

    /// The first four octets, which the truncated and the non-truncated header share, then the
    /// rest of `reference`.
    fn first_four(p: P, reference: &[u8]) -> Option<Vec<u8>> {
        let mut out = reference.to_vec();
        for (d, s) in out.iter_mut().zip(frame(p)?.iter().take(4)) {
            *d = *s;
        }
        Some(out)
    }

    #[expect(
        clippy::too_many_lines,
        reason = "a table of the fields of one header, with nothing to factor out"
    )]
    pub fn primary() -> Header {
        let baseline = octets(BASE).unwrap_or_default();
        let full = frame(BASE).unwrap_or_default();
        let reference = baseline.clone();
        let reference2 = baseline.clone();
        let e = |name, id, clause, base: u64, set: fn(P, u64) -> P| {
            encoded(name, id, clause, (BASE, base), set, octets)
        };
        Header {
            title: "USLP Transfer Frame primary header",
            note: "Probed with `aldis_uslp::writer::encode` on a variable-length Physical \
Channel, with the longest VCF Count (7 octets); the Frame Length is set through the length of the \
TFDZ, the OCF Flag through an OCF, and the VCF Count Length through the Virtual Channel",
            bits: (7 + usize::from(VCF_LEN)) * 8,
            baseline,
            fields: vec![
                rejected(
                    "Transfer Frame Version Number",
                    field::FRAME_VERSION,
                    "732.1 §4.1.2.2",
                ),
                e(
                    "Spacecraft Identifier",
                    Some(field::SPACECRAFT_ID),
                    "732.1 §4.1.2.3",
                    0,
                    |p, v| P { scid: v, ..p },
                ),
                e(
                    "Source-or-Destination Identifier",
                    None,
                    "732.1 §4.1.2.3",
                    0,
                    |p, v| P {
                        destination: v,
                        ..p
                    },
                ),
                e(
                    "Virtual Channel Identifier",
                    Some(field::VC_ID),
                    "732.1 §4.1.2.4",
                    0,
                    |p, v| P { vcid: v, ..p },
                ),
                e(
                    "MAP Identifier",
                    Some(field::MAP_ID),
                    "732.1 §4.1.2.5",
                    0,
                    |p, v| P { map: v, ..p },
                ),
                Field {
                    name: "End of Frame Primary Header Flag",
                    id: Some(field::END_OF_FRAME_PRIMARY_HEADER_FLAG),
                    clause: "732.1 §4.1.2.6",
                    how: How::Encode {
                        base: 0,
                        set: Box::new(move |v| {
                            first_four(
                                P {
                                    truncated: flag(v)?.into(),
                                    ..BASE
                                },
                                &reference,
                            )
                        }),
                    },
                },
                e(
                    "Frame Length",
                    Some(field::FRAME_LEN),
                    "732.1 §4.1.2.7",
                    19,
                    |p, v| P { length: v, ..p },
                ),
                e(
                    "Bypass/Sequence Control Flag",
                    None,
                    "732.1 §4.1.2.8.1",
                    1,
                    |p, v| P { bypass: v, ..p },
                ),
                e(
                    "Protocol Control Command Flag",
                    Some(field::PROTOCOL_CONTROL_COMMAND_FLAG),
                    "732.1 §4.1.2.8.2",
                    0,
                    |p, v| P { protocol: v, ..p },
                ),
                rejected("Reserve Spares", field::USLP_HEADER_SPARE, "732.1 §4.1.2.9"),
                e(
                    "Operation Control Field Flag",
                    Some(field::OCF_FLAG),
                    "732.1 §4.1.2.10",
                    0,
                    |p, v| P { ocf: v, ..p },
                ),
                Field {
                    name: "VCF Count Length",
                    id: Some(field::VCF_COUNT_LEN),
                    clause: "732.1 §4.1.2.11",
                    how: How::Encode {
                        base: u64::from(VCF_LEN),
                        set: Box::new(move |v| {
                            let mut out = reference2.clone();
                            let other = frame(P { vcf_len: v, ..BASE })?;
                            *out.get_mut(6)? = *other.get(6)?;
                            Some(out)
                        }),
                    },
                },
                e(
                    "Virtual Channel Frame Count",
                    Some(field::VCF_COUNT),
                    "732.1 §4.1.2.12",
                    0,
                    |p, v| P { count: v, ..p },
                ),
            ],
            reject: Box::new(move |h| {
                let channels = AnyChannel {
                    truncated: false,
                    vcf_len: VCF_LEN,
                };
                named(UslpFrame::decode(&patch(&full, h), &channels))
            }),
        }
    }

    pub fn truncated() -> Header {
        let truncated = P {
            truncated: 1,
            ..BASE
        };
        let baseline = head(frame(truncated).unwrap_or_default(), 4);
        let full = frame(truncated).unwrap_or_default();
        let reference = baseline.clone();
        let octets4: fn(P) -> Option<Vec<u8>> = |p| frame(p).map(|f| head(f, 4));
        let e = |name, id, clause, set: fn(P, u64) -> P| {
            encoded(name, id, clause, (truncated, 0), set, octets4)
        };
        Header {
            title: "USLP Transfer Frame truncated primary header",
            note: "Probed with `aldis_uslp::writer::encode_truncated` on a Virtual Channel with \
a truncated frame length",
            bits: 32,
            baseline,
            fields: vec![
                rejected(
                    "Transfer Frame Version Number",
                    field::FRAME_VERSION,
                    "732.1 §4.1.2.2",
                ),
                e(
                    "Spacecraft Identifier",
                    Some(field::SPACECRAFT_ID),
                    "732.1 §4.1.2.3",
                    |p, v| P { scid: v, ..p },
                ),
                e(
                    "Source-or-Destination Identifier",
                    None,
                    "732.1 §4.1.2.3",
                    |p, v| P {
                        destination: v,
                        ..p
                    },
                ),
                e(
                    "Virtual Channel Identifier",
                    Some(field::VC_ID),
                    "732.1 §4.1.2.4",
                    |p, v| P { vcid: v, ..p },
                ),
                e(
                    "MAP Identifier",
                    Some(field::MAP_ID),
                    "732.1 §4.1.2.5",
                    |p, v| P { map: v, ..p },
                ),
                Field {
                    name: "End of Frame Primary Header Flag",
                    id: Some(field::END_OF_FRAME_PRIMARY_HEADER_FLAG),
                    clause: "732.1 Annex D1.3.1",
                    how: How::Encode {
                        base: 1,
                        set: Box::new(move |v| {
                            first_four(
                                P {
                                    truncated: flag(v)?.into(),
                                    ..BASE
                                },
                                &reference,
                            )
                        }),
                    },
                },
            ],
            reject: Box::new(move |h| {
                let channels = AnyChannel {
                    truncated: true,
                    vcf_len: VCF_LEN,
                };
                named(UslpFrame::decode(&patch(&full, h), &channels))
            }),
        }
    }
}

mod time {
    //! The first octet of the P-field of the binary time codes (301.0 §3.1.1, §3.2.2, §3.3.2,
    //! §3.4.2).

    use aldis_core::field;
    use aldis_time::{
        CcsFormat, CcsVariation, CdsDayLen, CdsEpoch, CdsFormat, CdsSubMs, CucEpoch, CucFormat,
        PField, PFieldTail, TimeCodeId,
    };

    use super::{Header, encoded, flag, rejected, small};

    /// A tail of undefined P-field octets, which sets the extension flag.
    fn tail() -> Option<PFieldTail> {
        PFieldTail::new(&[0x01]).ok()
    }

    /// The decoder reads the identification of the P-field: it names the time code
    /// identification when the flipped bits give another one.
    fn id_reject(baseline: TimeCodeId) -> super::super::probe::Reject {
        Box::new(move |h| {
            PField::parse(h)
                .ok()
                .filter(|p| p.id() != baseline)
                .map(|_| field::TIME_CODE_ID)
        })
    }

    /// The values of the fields of a CUC P-field.
    #[derive(Clone, Copy)]
    struct U {
        extension: u64,
        epoch: u64,
        coarse: u64,
        fine: u64,
    }

    fn cuc(u: U) -> Option<Vec<u8>> {
        let epoch = if flag(u.epoch)? {
            CucEpoch::Agency
        } else {
            CucEpoch::Tai1958
        };
        let mut format = CucFormat::new(
            epoch,
            small(u.coarse, 4)?.checked_add(1)?,
            small(u.fine, 4)?,
        )
        .ok()?;
        if flag(u.extension)? {
            format = format.with_tail(tail()?).ok()?;
        }
        Some(vec![format.pfield().first()])
    }

    fn cuc_header() -> Header {
        let base = U {
            extension: 0,
            epoch: 0,
            coarse: 3,
            fine: 0,
        };
        let baseline = cuc(base).unwrap_or_default();
        Header {
            title: "P-field of the CUC time code, octet 1",
            note: "Probed with `CucFormat::pfield`; the Time Code Identification, which the \
epoch selects, is read by `PField::parse`",
            bits: 8,
            baseline,
            fields: vec![
                encoded(
                    "Extension flag",
                    Some(field::PFIELD),
                    "301.0 §3.1.1",
                    (base, 0),
                    |u, v| U { extension: v, ..u },
                    cuc,
                ),
                rejected(
                    "Time Code Identification",
                    field::TIME_CODE_ID,
                    "301.0 §3.1.1",
                ),
                encoded(
                    "Number of basic time unit octets, less one",
                    Some(field::COARSE_OCTETS),
                    "301.0 §3.2.2",
                    (base, 3),
                    |u, v| U { coarse: v, ..u },
                    cuc,
                ),
                encoded(
                    "Number of fractional time unit octets",
                    Some(field::FINE_OCTETS),
                    "301.0 §3.2.2",
                    (base, 0),
                    |u, v| U { fine: v, ..u },
                    cuc,
                ),
            ],
            reject: id_reject(TimeCodeId::CucLevel1),
        }
    }

    /// The values of the fields of a CDS P-field.
    #[derive(Clone, Copy)]
    struct D {
        extension: u64,
        epoch: u64,
        day: u64,
        subms: u64,
    }

    fn cds(d: D) -> Option<Vec<u8>> {
        let mut format = CdsFormat::new(
            if flag(d.epoch)? {
                CdsEpoch::Agency
            } else {
                CdsEpoch::Ccsds1958
            },
            if flag(d.day)? {
                CdsDayLen::Bits24
            } else {
                CdsDayLen::Bits16
            },
            match d.subms {
                0 => CdsSubMs::None,
                1 => CdsSubMs::Micros,
                2 => CdsSubMs::Picos,
                _ => return None,
            },
        );
        if flag(d.extension)? {
            format = format.with_tail(tail()?);
        }
        Some(vec![format.pfield().first()])
    }

    fn cds_header() -> Header {
        let base = D {
            extension: 0,
            epoch: 0,
            day: 0,
            subms: 0,
        };
        Header {
            title: "P-field of the CDS time code, octet 1",
            note: "Probed with `CdsFormat::pfield`",
            bits: 8,
            baseline: cds(base).unwrap_or_default(),
            fields: vec![
                encoded(
                    "Extension flag",
                    Some(field::PFIELD),
                    "301.0 §3.1.1",
                    (base, 0),
                    |d, v| D { extension: v, ..d },
                    cds,
                ),
                rejected(
                    "Time Code Identification",
                    field::TIME_CODE_ID,
                    "301.0 §3.1.1",
                ),
                encoded(
                    "Epoch identification",
                    None,
                    "301.0 §3.3.2",
                    (base, 0),
                    |d, v| D { epoch: v, ..d },
                    cds,
                ),
                encoded(
                    "Length of day segment",
                    None,
                    "301.0 §3.3.2",
                    (base, 0),
                    |d, v| D { day: v, ..d },
                    cds,
                ),
                encoded(
                    "Length of submillisecond segment",
                    Some(field::SUBMS),
                    "301.0 §3.3.2",
                    (base, 0),
                    |d, v| D { subms: v, ..d },
                    cds,
                ),
            ],
            reject: id_reject(TimeCodeId::Cds),
        }
    }

    /// The values of the fields of a CCS P-field.
    #[derive(Clone, Copy)]
    struct C {
        extension: u64,
        variation: u64,
        subsecond: u64,
    }

    fn ccs(c: C) -> Option<Vec<u8>> {
        let mut format = CcsFormat::new(
            if flag(c.variation)? {
                CcsVariation::DayOfYear
            } else {
                CcsVariation::MonthDay
            },
            small(c.subsecond, 7)?,
        )
        .ok()?;
        if flag(c.extension)? {
            format = format.with_tail(tail()?);
        }
        Some(vec![format.pfield().first()])
    }

    fn ccs_header() -> Header {
        let base = C {
            extension: 0,
            variation: 0,
            subsecond: 0,
        };
        Header {
            title: "P-field of the CCS time code, octet 1",
            note: "Probed with `CcsFormat::pfield`",
            bits: 8,
            baseline: ccs(base).unwrap_or_default(),
            fields: vec![
                encoded(
                    "Extension flag",
                    Some(field::PFIELD),
                    "301.0 §3.1.1",
                    (base, 0),
                    |c, v| C { extension: v, ..c },
                    ccs,
                ),
                rejected(
                    "Time Code Identification",
                    field::TIME_CODE_ID,
                    "301.0 §3.1.1",
                ),
                encoded(
                    "Calendar variation",
                    None,
                    "301.0 §3.4.2",
                    (base, 0),
                    |c, v| C { variation: v, ..c },
                    ccs,
                ),
                encoded(
                    "Number of subsecond octets",
                    Some(field::SUBSECOND_OCTETS),
                    "301.0 §3.4.2",
                    (base, 0),
                    |c, v| C { subsecond: v, ..c },
                    ccs,
                ),
            ],
            reject: id_reject(TimeCodeId::Ccs),
        }
    }

    pub fn all() -> Vec<Header> {
        vec![cuc_header(), cds_header(), ccs_header()]
    }
}
