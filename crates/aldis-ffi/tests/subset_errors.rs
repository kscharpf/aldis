//! The errors a subset library returns for what it does not build (GEN-009; c-abi.md, after
//! CABI-060): the `_init` of a link engine with a protocol whose feature is off is
//! `ALDIS_ERR_INVALID_CONFIG` on `link_protocol` and leaves the storage as the caller had it, and a
//! Space Packet layout with the typed time secondary header without `spp-time` is
//! `ALDIS_ERR_INVALID_CONFIG` on `time_code`. Each test compiles only where its feature is off, so
//! the full build runs none; the subset of `cargo xtask features` (`std,core,spp,tm,tc,link,cop1`)
//! runs them all. This test has no `required-features`.
#![expect(unsafe_code, reason = "the tests call the C ABI")]
#![expect(
    clippy::borrow_as_ptr,
    clippy::undocumented_unsafe_blocks,
    reason = "test-only harness: the storage a C caller declares (RS-018)"
)]
#![cfg_attr(
    all(
        feature = "link",
        feature = "aos",
        feature = "uslp",
        feature = "spp-time"
    ),
    allow(
        unused_imports,
        dead_code,
        reason = "every test is gated on a feature being off"
    )
)]

#[cfg(feature = "link")]
mod link {
    use aldis_ffi::aldis_error_detail_t;
    use aldis_ffi::error::ALDIS_ERR_INVALID_CONFIG;
    use aldis_ffi::field::ALDIS_FIELD_LINK_PROTOCOL;
    use aldis_ffi::link::*;

    const SENTINEL: u64 = 0xA5A5_A5A5_A5A5_A5A5;

    fn detail() -> aldis_error_detail_t {
        aldis_error_detail_t {
            kind: 999,
            ..aldis_error_detail_t::default()
        }
    }

    /// Asserts the error of a refused protocol: status, kind, and field.
    fn assert_unsupported(status: u32, d: &aldis_error_detail_t) {
        assert_eq!(status, ALDIS_ERR_INVALID_CONFIG);
        assert_eq!(d.kind, ALDIS_ERR_INVALID_CONFIG);
        assert_eq!(d.field, ALDIS_FIELD_LINK_PROTOCOL);
    }

    fn receiver(protocol: u32) {
        use receiver::*;
        let mut s = aldis_frame_receiver_t {
            words: [SENTINEL; ALDIS_FRAME_RECEIVER_WORDS],
        };
        let cfg = aldis_frame_receiver_config_t {
            protocol,
            ..aldis_frame_receiver_config_t::default()
        };
        let mut d = detail();
        let st = unsafe { aldis_frame_receiver_init(&mut s, &cfg, &mut d) };
        assert_unsupported(st, &d);
        assert!(
            s.words.iter().all(|w| *w == SENTINEL),
            "the storage is untouched"
        );
        // The storage holds no engine: a step fails.
        let input = aldis_frame_receiver_input_t {
            kind: 0,
            data: core::ptr::null(),
            data_len: 0,
            reason: 0,
            config: aldis_frame_receiver_config_t::default(),
        };
        let mut actions = aldis_frame_receiver_actions_t::default();
        let mut e = detail();
        let st = unsafe {
            aldis_frame_receiver_step(
                &mut s,
                &input,
                core::ptr::null_mut(),
                0,
                &mut actions,
                &mut e,
            )
        };
        assert_ne!(st, 0);
    }

    #[expect(
        clippy::large_stack_arrays,
        reason = "the storage a C caller declares (RS-018)"
    )]
    fn mux(protocol: u32) {
        use mux::*;
        let mut s = aldis_channel_mux_t {
            words: [SENTINEL; ALDIS_CHANNEL_MUX_WORDS],
        };
        let cfg = aldis_channel_mux_config_t {
            protocol,
            ..aldis_channel_mux_config_t::default()
        };
        let mut d = detail();
        let st = unsafe { aldis_channel_mux_init(&mut s, &cfg, &mut d) };
        assert_unsupported(st, &d);
        assert!(s.words.iter().all(|w| *w == SENTINEL));
    }

    fn access(protocol: u32) {
        use access::*;
        let mut s = aldis_access_generator_t {
            words: [SENTINEL; ALDIS_ACCESS_GENERATOR_WORDS],
        };
        let mut cfg = aldis_access_generator_config_t::default();
        cfg.channel.protocol = protocol;
        let mut d = detail();
        let st = unsafe { aldis_access_generator_init(&mut s, &cfg, &mut d) };
        assert_unsupported(st, &d);
        assert!(s.words.iter().all(|w| *w == SENTINEL));
    }

    #[cfg(not(feature = "aos"))]
    #[test]
    fn aos_engines_are_unsupported_without_aos() {
        // req: GEN-009, LNK-001, CABI-031
        let p = ALDIS_LINK_PROTOCOL_AOS;
        receiver(p);
        mux(p);
        access(p);
    }

    #[cfg(not(feature = "uslp"))]
    #[test]
    fn uslp_engines_are_unsupported_without_uslp() {
        // req: GEN-009, LNK-001, CABI-031
        let p = ALDIS_LINK_PROTOCOL_USLP;
        receiver(p);
        mux(p);
        access(p);
    }

    #[cfg(not(feature = "tm"))]
    #[test]
    fn tm_engines_are_unsupported_without_tm() {
        // req: GEN-009, LNK-001, CABI-031
        let p = ALDIS_LINK_PROTOCOL_TM;
        receiver(p);
        mux(p);
        access(p);
    }

    #[cfg(not(feature = "tc"))]
    #[test]
    fn tc_receiver_is_unsupported_without_tc() {
        // req: GEN-009, LNK-001, CABI-031
        receiver(ALDIS_LINK_PROTOCOL_TC);
    }
}

#[cfg(all(feature = "spp", not(feature = "spp-time")))]
mod spp {
    use aldis_ffi::error::ALDIS_ERR_INVALID_CONFIG;
    use aldis_ffi::field::ALDIS_FIELD_TIME_CODE;
    use aldis_ffi::spp::*;
    use aldis_ffi::{ALDIS_MODE_STRICT, aldis_error_detail_t};

    #[test]
    fn the_time_layout_is_invalid_config_without_spp_time() {
        // req: GEN-009, SPP-008
        let layout = aldis_spp_secondary_layout_t {
            kind: ALDIS_SPP_SECONDARY_TIME,
            ..aldis_spp_secondary_layout_t::default()
        };
        let bytes = [0_u8; 16];
        let mut out = aldis_spp_split_t::default();
        let mut d = aldis_error_detail_t {
            kind: 999,
            ..aldis_error_detail_t::default()
        };
        let st = unsafe {
            aldis_spp_split(
                bytes.as_ptr(),
                bytes.len(),
                ALDIS_MODE_STRICT,
                &layout,
                &mut out,
                &mut d,
            )
        };
        assert_eq!(st, ALDIS_ERR_INVALID_CONFIG);
        assert_eq!(d.kind, ALDIS_ERR_INVALID_CONFIG);
        assert_eq!(d.field, ALDIS_FIELD_TIME_CODE);
    }
}
