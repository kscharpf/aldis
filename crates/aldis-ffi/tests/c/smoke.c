/*
 * C smoke test of the aldis-ffi libraries and the generated header (CABI-001, CABI-003).
 * `cargo xtask header c-smoke <cc> <c++>` compiles it as C99 against the static library and
 * as C++11 against the shared library, with -pedantic -Wall -Wextra -Werror, and runs both.
 * It is written in the common subset of C99 and C++11: no designated initializers.
 */
#include "aldis/aldis.h"

#include <stdio.h>
#include <string.h>

static int failures = 0;

static void check(int ok, const char *what) {
    if (!ok) {
        printf("FAIL: %s\n", what);
        failures++;
    }
}

/* AOS (M2 WP-4 module 8): an M_PDU frame of a 10-bit SCID with the FHEC and the FECF, through
 * the two-phase writer, the one-shot encoder, and the decoder after a corrected header hit. */
static void aos_smoke(void) {
    aldis_error_detail_t err;
    err.struct_size = (uint32_t)sizeof err;
    aldis_aos_mc_config_t mc;
    memset(&mc, 0, sizeof mc);
    mc.struct_size = (uint32_t)sizeof mc;
    mc.scid = 0x2A5;
    mc.pc.struct_size = (uint32_t)sizeof mc.pc;
    mc.pc.fhec = 1;
    mc.pc.fecf = 1;
    mc.pc.edition = ALDIS_AOS_EDITION_ISSUE5;
    mc.pc.frame_len = 16;
    aldis_aos_vc_config_t vc;
    memset(&vc, 0, sizeof vc);
    vc.struct_size = (uint32_t)sizeof vc;
    vc.vcid = 1;
    vc.content = ALDIS_AOS_CONTENT_MPDU;
    check(aldis_aos_mc_config_check(&mc, &vc, 1, &err) == ALDIS_OK, "aldis_aos_mc_config_check");
    aldis_aos_fields_t fields;
    memset(&fields, 0, sizeof fields);
    fields.vcid = 1;
    fields.vc_frame_count = 7;
    fields.pointer = ALDIS_AOS_POINTER_STARTS_AT;
    uint8_t frame[16];
    aldis_aos_layout_t layout;
    check(aldis_aos_writer_begin(&mc, &vc, 1, &fields, frame, sizeof frame, &layout, &err) ==
              ALDIS_OK,
          "aldis_aos_writer_begin");
    check(layout.data_offset == 8 && layout.zone_offset == 10 && layout.zone_len == 4 &&
              layout.fecf_offset == 14,
          "AOS frame layout");
    memset(frame + layout.zone_offset, 0x5A, layout.zone_len);
    size_t len = 0;
    check(aldis_aos_writer_finish(&mc, &vc, 1, &fields, frame, sizeof frame, &len, &err) ==
                  ALDIS_OK &&
              len == 16,
          "aldis_aos_writer_finish");
    uint8_t zone[4];
    memset(zone, 0x5A, sizeof zone);
    aldis_aos_content_t content;
    memset(&content, 0, sizeof content);
    content.zone = zone;
    content.zone_len = sizeof zone;
    uint8_t one_shot[16];
    size_t one_len = 0;
    check(aldis_aos_encode(&mc, &vc, 1, &fields, &content, one_shot, sizeof one_shot, &one_len,
                           &err) == ALDIS_OK &&
              one_len == 16 && memcmp(one_shot, frame, 16) == 0,
          "aldis_aos_encode agrees with the two-phase writer");
    uint16_t fhec = 0;
    check(aldis_aos_fhec_encode(frame, ALDIS_AOS_PRIMARY_HEADER_LEN, &fhec, &err) == ALDIS_OK &&
              fhec == (uint16_t)((frame[6] << 8) | frame[7]),
          "aldis_aos_fhec_encode");
    frame[1] ^= 0x01; /* a bit of the VCID, which the FHEC corrects (AOS-014) */
    aldis_aos_frame_t view;
    check(aldis_aos_decode(frame, len, &mc, &vc, 1, ALDIS_MODE_STRICT, &view, NULL) == ALDIS_OK,
          "aldis_aos_decode");
    check(view.header.scid == 0x2A5 && view.header.vcid == 1 &&
              view.header.corrected_symbols == 1 && view.pdu.pointer == ALDIS_AOS_POINTER_STARTS_AT &&
              view.pdu.pointer_value == 0 && frame[view.layout.zone_offset] == 0x5A,
          "decoded AOS frame (CABI-012)");
}

int main(void) {
    /* CABI-030: aldis_limits reports the build capacities. */
    aldis_limits_t limits;
    memset(&limits, 0, sizeof limits);
    limits.struct_size = (uint32_t)sizeof limits;
    check(aldis_limits(&limits) == ALDIS_OK, "aldis_limits");
    check(limits.spp_tracker_max_apids == ALDIS_SPP_TRACKER_MAX_APIDS, "tracker capacity");

    aos_smoke();

    /* A telemetry packet, APID 0x0A0, one octet of data, through encode and decode. */
    aldis_spp_config_t cfg;
    cfg.struct_size = (uint32_t)sizeof cfg;
    cfg.max_packet_len = ALDIS_SPP_MAX_PACKET_LEN;
    aldis_spp_fields_t fields;
    memset(&fields, 0, sizeof fields);
    fields.packet_type = ALDIS_SPP_TELEMETRY;
    fields.sequence_flags = ALDIS_SPP_SEQ_UNSEGMENTED;
    fields.apid = 0x0A0;
    fields.sequence_count = 5;
    const uint8_t user[1] = {0x42};
    uint8_t packet[16];
    size_t len = 0;
    aldis_error_detail_t err;
    memset(&err, 0, sizeof err);
    err.struct_size = (uint32_t)sizeof err; /* CABI-021: the caller sets it */
    check(aldis_spp_encode(&cfg, &fields, NULL, 0, user, sizeof user, packet, sizeof packet,
                           &len, &err) == ALDIS_OK,
          "aldis_spp_encode");
    check(len == 7, "encoded length");

    aldis_spp_packet_t view;
    check(aldis_spp_decode(packet, len, ALDIS_MODE_STRICT, &view, NULL) == ALDIS_OK,
          "aldis_spp_decode");
    check(view.fields.apid == 0x0A0 && view.fields.sequence_count == 5, "decoded fields");
    check(view.data_offset == 6 && view.data_len == 1 && packet[view.data_offset] == 0x42,
          "data field offset and length (CABI-012)");

    /* D14: a prefix too short gives ALDIS_ERR_INCOMPLETE with `needed`. */
    size_t total = 0;
    check(aldis_spp_peek_length(packet, 3, &total, &err) == ALDIS_ERR_INCOMPLETE,
          "peek_length of a short prefix");
    check(err.kind == 2 && err.detail == ALDIS_DETAIL_NEEDED && err.needed == 6,
          "Incomplete detail");

    /* CABI-021: a struct_size below the fixed prefix is rejected; the prefix alone is enough. */
    aldis_error_detail_t small;
    memset(&small, 0xFF, sizeof small);
    small.struct_size = ALDIS_ERROR_DETAIL_MIN_SIZE - 1;
    check(aldis_spp_peek_length(packet, 3, &total, &small) == ALDIS_ERR_INVALID_FIELD,
          "detail struct_size too small");
    small.struct_size = ALDIS_ERROR_DETAIL_MIN_SIZE;
    check(aldis_spp_peek_length(packet, 3, &total, &small) == ALDIS_ERR_INCOMPLETE &&
              small.kind == 2 && small.needed == UINT64_MAX,
          "detail written within its struct_size");

    /* CABI-022: a NULL output is rejected, and NULL detail is allowed. */
    check(aldis_spp_peek_length(packet, len, NULL, NULL) == ALDIS_ERR_INVALID_FIELD,
          "NULL output rejected");
    check(-ALDIS_ERR_INCOMPLETE == 2, "status codes are negated catalogue codes");

    /* A CUC time code with its P-field. */
    aldis_time_decoded_t decoded;
    const uint8_t cuc[4] = {0x15, 0x01, 0x02, 0x80}; /* 2 coarse, 1 fine octet */
    check(aldis_time_decode(cuc, sizeof cuc, ALDIS_MODE_STRICT, &decoded, NULL) == ALDIS_OK,
          "aldis_time_decode");
    check(decoded.format.code == ALDIS_TIME_CUC && decoded.value.coarse == 0x0102 &&
              decoded.value.fine[0] == 0x80 && decoded.value.fine_len == 1,
          "decoded CUC");

    /* An idle Encapsulation Packet of one octet. */
    uint8_t idle[1];
    check(aldis_epp_idle(1, 0, idle, sizeof idle, &len, NULL) == ALDIS_OK && idle[0] == 0xE0,
          "aldis_epp_idle");

    /* The CRC-16, the OCF, and the OID generator of aldis-core (M2 plan D5, D14). */
    uint16_t crc = 0;
    check(aldis_crc16((const uint8_t *)"123456789", 9, &crc, NULL) == ALDIS_OK && crc == 0x29B1,
          "aldis_crc16 check value (CMN-002)");
    const uint8_t ocf[ALDIS_OCF_LEN] = {0x01, 0x14, 0x0C, 0x2A};
    uint32_t report = 99;
    check(aldis_ocf_classify(ocf, sizeof ocf, &report, NULL) == ALDIS_OK &&
              report == ALDIS_OCF_CLCW,
          "aldis_ocf_classify");
    check(aldis_ocf_check(ocf, sizeof ocf, ALDIS_OCF_SDLS_FSR, &err) == ALDIS_ERR_INVALID_FIELD &&
              err.field == ALDIS_FIELD_OCF_TYPE_FLAG,
          "aldis_ocf_check names the Type Flag");
    uint64_t oid_storage[ALDIS_OID_GENERATOR_STORAGE_SIZE / 8];
    aldis_oid_generator_t *oid = NULL;
    uint8_t idle_data[6];
    check(aldis_oid_generator_init(oid_storage, sizeof oid_storage, ALDIS_OID_GALOIS, &oid,
                                   &err) == ALDIS_OK,
          "aldis_oid_generator_init");
    check(aldis_oid_generator_fill(oid, idle_data, sizeof idle_data, &err) == ALDIS_OK &&
              idle_data[0] == 0xFF && idle_data[4] == 0x6D && idle_data[5] == 0xB6,
          "OID sequence of 132.0 Annex D");
    aldis_oid_generator_deinit(oid);
    check(aldis_oid_generator_fill(oid, idle_data, sizeof idle_data, NULL) ==
              ALDIS_ERR_INVALID_HANDLE,
          "a released generator is an invalid handle (CABI-032)");

    /* A 16-octet TM Transfer Frame of VC 5 on SCID 677, through the two-phase writer and the
     * decoder (M2 plan D14): the master channel and an array of one virtual channel. */
    aldis_tm_mc_config_t mc;
    memset(&mc, 0, sizeof mc);
    mc.struct_size = (uint32_t)sizeof mc;
    mc.scid = 677;
    mc.pc.struct_size = (uint32_t)sizeof mc.pc;
    mc.pc.frame_len = 16;
    aldis_tm_vc_config_t vc;
    memset(&vc, 0, sizeof vc);
    vc.struct_size = (uint32_t)sizeof vc;
    vc.vcid = 5;
    vc.content = ALDIS_TM_CONTENT_PACKETS;
    aldis_tm_fields_t tm;
    memset(&tm, 0, sizeof tm);
    tm.vcid = 5;
    tm.mc_frame_count = 1;
    tm.vc_frame_count = 2;
    tm.pointer = ALDIS_TM_POINTER_STARTS_AT;
    uint8_t frame[16];
    aldis_tm_layout_t layout;
    check(aldis_tm_writer_begin(&mc, &vc, 1, &tm, frame, sizeof frame, &layout, &err) ==
              ALDIS_OK,
          "aldis_tm_writer_begin");
    check(layout.data_offset == 6 && layout.data_len == 10 && layout.ocf_len == 0,
          "TM frame layout");
    memset(frame + layout.data_offset, 0xD0, layout.data_len);
    check(aldis_tm_writer_finish(&mc, &vc, 1, &tm, frame, sizeof frame, &len, &err) == ALDIS_OK &&
              len == 16,
          "aldis_tm_writer_finish");
    /* The same frame in one call, its data field given in the content struct. */
    uint8_t tm_data[10];
    memset(tm_data, 0xD0, sizeof tm_data);
    aldis_tm_content_t content;
    memset(&content, 0, sizeof content);
    content.data = tm_data;
    content.data_len = sizeof tm_data;
    uint8_t one_shot[16];
    size_t one_len = 0;
    check(aldis_tm_encode(&mc, &vc, 1, &tm, &content, one_shot, sizeof one_shot, &one_len,
                          &err) == ALDIS_OK &&
              one_len == 16 && memcmp(one_shot, frame, 16) == 0,
          "aldis_tm_encode agrees with the two-phase writer");
    aldis_tm_frame_t tm_view;
    check(aldis_tm_decode(frame, len, &mc.pc, ALDIS_MODE_STRICT, &tm_view, NULL) == ALDIS_OK,
          "aldis_tm_decode");
    check(tm_view.header.scid == 677 && tm_view.header.vcid == 5 &&
              tm_view.pointer == ALDIS_TM_POINTER_STARTS_AT &&
              tm_view.layout.data_len == 10 && frame[tm_view.layout.data_offset] == 0xD0,
          "decoded TM frame (CABI-012)");
    check(aldis_tm_validate(frame, len, &mc, &vc, 1, ALDIS_MODE_STRICT, NULL) == ALDIS_OK,
          "aldis_tm_validate");

    /* --- TC (M2 WP-3 module 8) ---
     * A Type-AD frame of VC 1 on SCID 0x155 through the two-phase writer, followed by fill in
     * one data unit, delimited and validated (M2 plan D14); then a CLCW as a word. */
    check(limits.tc_max_maps == ALDIS_TC_MAX_MAPS, "TC MAP capacity");
    aldis_tc_mc_config_t tc_mc;
    memset(&tc_mc, 0, sizeof tc_mc);
    tc_mc.struct_size = (uint32_t)sizeof tc_mc;
    tc_mc.scid = 0x155;
    tc_mc.pc.struct_size = (uint32_t)sizeof tc_mc.pc;
    tc_mc.pc.fecf = 1;
    tc_mc.pc.max_frame_len = 64;
    aldis_tc_vc_config_t tc_vc;
    memset(&tc_vc, 0, sizeof tc_vc);
    tc_vc.struct_size = (uint32_t)sizeof tc_vc;
    tc_vc.vcid = 1;
    tc_vc.max_frame_len = 64;
    aldis_tc_fields_t tc;
    memset(&tc, 0, sizeof tc);
    tc.frame_type = ALDIS_TC_FRAME_AD;
    tc.scid = 0x155;
    tc.vcid = 1;
    tc.sequence_number = 4;
    uint8_t unit[16];
    memset(unit, 0x55, sizeof unit); /* Fill Data after the frame */
    aldis_tc_layout_t tc_layout;
    check(aldis_tc_writer_begin(&tc_mc, &tc_vc, 1, NULL, 0, &tc, 3, unit, sizeof unit,
                                &tc_layout, &err) == ALDIS_OK,
          "aldis_tc_writer_begin");
    check(tc_layout.data_offset == 5 && tc_layout.data_len == 3 && tc_layout.fecf_len == 2,
          "TC frame layout");
    unit[5] = 1;
    unit[6] = 2;
    unit[7] = 3;
    check(aldis_tc_writer_finish(&tc_mc, &tc_vc, 1, NULL, 0, &tc, 3, 3, unit, sizeof unit, &len,
                                 &err) == ALDIS_OK &&
              len == 10,
          "aldis_tc_writer_finish");
    const uint8_t tc_frame[10] = {0x01, 0x55, 0x04, 0x09, 0x04, 0x01, 0x02, 0x03, 0xE0, 0xC0};
    check(memcmp(unit, tc_frame, sizeof tc_frame) == 0, "TC frame octets (tc/ad.json)");
    size_t offset = 0;
    int tc_frames = 0;
    int tc_fill = 0;
    aldis_tc_delimited_t item;
    while (aldis_tc_next_frame(unit, sizeof unit, offset, &item, &err) == ALDIS_OK &&
           item.kind != ALDIS_TC_DELIMITED_END) {
        if (item.kind == ALDIS_TC_DELIMITED_FRAME) {
            aldis_tc_frame_t tc_view;
            uint32_t reason = 99;
            check(aldis_tc_validate(unit + item.offset, item.len, &tc_mc, &tc_vc, 1, NULL, 0,
                                    ALDIS_MODE_STRICT, &tc_view, &reason, &err) == ALDIS_OK &&
                      reason == ALDIS_TC_REASON_NONE,
                  "aldis_tc_validate");
            check(tc_view.header.sequence_number == 4 && tc_view.layout.data_len == 3 &&
                      unit[item.offset + tc_view.layout.data_offset] == 1,
                  "validated TC frame (CABI-012)");
            tc_frames++;
        } else if (item.kind == ALDIS_TC_DELIMITED_FILL) {
            tc_fill++;
        }
        offset += item.len;
    }
    check(tc_frames == 1 && tc_fill == 1 && offset == sizeof unit, "aldis_tc_next_frame");
    aldis_clcw_t clcw;
    uint32_t clcw_anomalies = 99;
    check(aldis_clcw_decode(0x01140C2Au, ALDIS_MODE_STRICT, &clcw, &clcw_anomalies, &err) ==
                  ALDIS_OK &&
              clcw.vcid == 5 && clcw.retransmit == 1 && clcw.report_value == 42 &&
              clcw_anomalies == 0,
          "aldis_clcw_decode");
    uint32_t clcw_word = 0;
    check(aldis_clcw_encode(&clcw, &clcw_word, &err) == ALDIS_OK && clcw_word == 0x01140C2Au,
          "aldis_clcw_encode");

    /* ---- USLP (M2 WP-5 module 9) ---- */
    /* A variable-length USLP frame of VC 4 MAP 0 on SCID 0x155 with an FECF, through the
     * two-phase writer, the decoder, the validation checks, and the delimiter (M2 plan D14):
     * the physical channel and arrays of one virtual channel and one MAP. */
    aldis_uslp_config_t upc;
    memset(&upc, 0, sizeof upc);
    upc.struct_size = (uint32_t)sizeof upc;
    upc.frame_type = ALDIS_USLP_FRAME_VARIABLE;
    upc.fecf = 1;
    upc.frame_len = 64;
    aldis_uslp_vc_config_t uvc;
    memset(&uvc, 0, sizeof uvc);
    uvc.struct_size = (uint32_t)sizeof uvc;
    uvc.scid = 0x155;
    uvc.vcid = 4;
    uvc.frame_type = ALDIS_USLP_FRAME_VARIABLE;
    uvc.security = ALDIS_USLP_SECURITY_VC;
    aldis_uslp_map_config_t umap;
    memset(&umap, 0, sizeof umap);
    umap.struct_size = (uint32_t)sizeof umap;
    umap.scid = 0x155;
    umap.vcid = 4;
    check(aldis_uslp_vc_table_check(&upc, &uvc, 1, &umap, 1, &err) == ALDIS_OK,
          "aldis_uslp_vc_table_check");
    aldis_uslp_fields_t ufields;
    memset(&ufields, 0, sizeof ufields);
    ufields.channel.scid = 0x155;
    ufields.channel.source_or_destination = ALDIS_USLP_SOURCE;
    ufields.channel.vcid = 4;
    ufields.kind = ALDIS_USLP_KIND_SEQUENCE_CONTROLLED;
    aldis_uslp_content_t ucontent;
    memset(&ucontent, 0, sizeof ucontent);
    ucontent.tfdf.rule = ALDIS_USLP_RULE_OCTET_STREAM;
    ucontent.tfdf.upid = ALDIS_USLP_UPID_OCTET_STREAM;
    uint8_t uframe[64];
    aldis_uslp_layout_t ulayout;
    check(aldis_uslp_writer_begin(&upc, &uvc, 1, &umap, 1, &ufields, &ucontent, 10, uframe,
                                  sizeof uframe, &ulayout, &err) == ALDIS_OK &&
              ulayout.tfdz_offset == 8 && ulayout.tfdz_len == 10,
          "aldis_uslp_writer_begin");
    memset(uframe + ulayout.tfdz_offset, 0xD0, 4);
    size_t ulen = 0;
    check(aldis_uslp_writer_finish(&upc, &uvc, 1, &umap, 1, &ufields, &ucontent, 10, 4, uframe,
                                   sizeof uframe, &ulen, &err) == ALDIS_OK &&
              ulen == 14,
          "aldis_uslp_writer_finish");
    aldis_uslp_frame_t uview;
    check(aldis_uslp_decode(uframe, ulen, &upc, &uvc, 1, &umap, 1, ALDIS_MODE_STRICT, &uview,
                            NULL) == ALDIS_OK,
          "aldis_uslp_decode");
    check(!uview.truncated && uview.channel.scid == 0x155 && uview.channel.vcid == 4 &&
              uview.tfdf.rule == ALDIS_USLP_RULE_OCTET_STREAM && uview.upid_assigned &&
              uview.layout.tfdz_len == 4 && uframe[uview.layout.tfdz_offset] == 0xD0,
          "decoded USLP frame (CABI-012)");
    uint32_t ureason = 0xFFFFFFFFu;
    check(aldis_uslp_validate(uframe, ulen, &upc, &uvc, 1, &umap, 1, ALDIS_MODE_STRICT, &uview,
                              &ureason, NULL) == ALDIS_OK &&
              ureason == ALDIS_USLP_REASON_NONE,
          "aldis_uslp_validate");
    uframe[ulen - 1] ^= 1;
    check(aldis_uslp_validate(uframe, ulen, &upc, &uvc, 1, &umap, 1, ALDIS_MODE_STRICT, &uview,
                              &ureason, NULL) == ALDIS_ERR_CRC_MISMATCH &&
              ureason == ALDIS_USLP_REASON_FECF_MISMATCH,
          "a wrong FECF fails test (e) (USLP-014)");
    aldis_uslp_delimited_t uitem;
    check(aldis_uslp_next_frame(uframe, ulen, 0, &upc, &uvc, 1, &uitem, NULL) == ALDIS_OK &&
              uitem.kind == ALDIS_USLP_DELIMITED_FRAME && uitem.len == ulen,
          "aldis_uslp_next_frame");
    check(aldis_uslp_next_frame(uframe, ulen, ulen, &upc, &uvc, 1, &uitem, NULL) == ALDIS_OK &&
              uitem.kind == ALDIS_USLP_DELIMITED_END,
          "the delimiter ends at the end of the unit");
    check(limits.uslp_max_vcs == ALDIS_USLP_MAX_VCS && limits.uslp_max_maps == ALDIS_USLP_MAX_MAPS,
          "USLP channel table capacities");
    /* ---- end USLP ---- */

    if (failures != 0) {
        return 1;
    }
    printf("aldis-ffi C smoke test passed\n");
    return 0;
}
