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
 * the one-shot encoder and the decoder after a corrected header hit. */
static void aos_smoke(void) {
    aldis_error_detail_t err;
    aldis_aos_mc_config_t mc;
    memset(&mc, 0, sizeof mc);
    mc.scid = 0x2A5;
    mc.pc.fhec = 1;
    mc.pc.fecf = 1;
    mc.pc.edition = ALDIS_AOS_EDITION_ISSUE5;
    mc.pc.frame_len = 16;
    aldis_aos_vc_config_t vc;
    memset(&vc, 0, sizeof vc);
    vc.vcid = 1;
    vc.content = ALDIS_AOS_CONTENT_MPDU;
    check(aldis_aos_mc_config_check(&mc, &vc, 1, &err) == ALDIS_OK, "aldis_aos_mc_config_check");
    aldis_aos_fields_t fields;
    memset(&fields, 0, sizeof fields);
    fields.vcid = 1;
    fields.vc_frame_count = 7;
    fields.pointer = ALDIS_AOS_POINTER_STARTS_AT;
    uint8_t frame[16];
    uint8_t zone[4];
    memset(zone, 0x5A, sizeof zone);
    aldis_aos_content_t content;
    memset(&content, 0, sizeof content);
    content.zone = zone;
    content.zone_len = sizeof zone;
    size_t len = 0;
    check(aldis_aos_encode(&mc, &vc, 1, &fields, &content, frame, sizeof frame, &len, &err) ==
                  ALDIS_OK &&
              len == 16 && frame[10] == 0x5A,
          "aldis_aos_encode");
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
              view.pdu.pointer_value == 0 && frame[view.layout.zone.offset] == 0x5A,
          "decoded AOS frame (CABI-012)");
}

int main(void) {
    /* CABI-030: the build capacities are constants of the header. */
    check(ALDIS_SPP_TRACKER_MAX_APIDS > 0, "tracker capacity");

    aos_smoke();

    /* A telemetry packet, APID 0x0A0, one octet of data, through encode and decode. */
    aldis_spp_config_t cfg;
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
    check(aldis_spp_encode(&cfg, &fields, NULL, 0, user, sizeof user, packet, sizeof packet,
                           &len, &err) == ALDIS_OK,
          "aldis_spp_encode");
    check(len == 7, "encoded length");

    aldis_spp_packet_t view;
    check(aldis_spp_decode(packet, len, ALDIS_MODE_STRICT, &view, NULL) == ALDIS_OK,
          "aldis_spp_decode");
    check(view.fields.apid == 0x0A0 && view.fields.sequence_count == 5, "decoded fields");
    check(view.data.offset == 6 && view.data.len == 1 && packet[view.data.offset] == 0x42,
          "data field offset and length (CABI-012)");

    /* D14: a prefix too short gives ALDIS_ERR_INCOMPLETE with `needed`. */
    size_t total = 0;
    check(aldis_spp_peek_length(packet, 3, &total, &err) == ALDIS_ERR_INCOMPLETE,
          "peek_length of a short prefix");
    check(err.kind == 2 && err.detail == ALDIS_DETAIL_NEEDED && err.needed == 6,
          "Incomplete detail");

    /* CABI-022: a NULL output is rejected, and NULL detail is allowed. */
    check(aldis_spp_peek_length(packet, len, NULL, NULL) == ALDIS_ERR_INVALID_FIELD,
          "NULL output rejected");
    check(ALDIS_ERR_INCOMPLETE == 2, "status codes are the catalogue codes");

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
    aldis_oid_generator_t oid_storage;
    aldis_oid_generator_t *oid = &oid_storage;
    uint8_t idle_data[6];
    check(aldis_oid_generator_init(oid, ALDIS_OID_GALOIS, &err) == ALDIS_OK,
          "aldis_oid_generator_init");
    check(aldis_oid_generator_fill(oid, idle_data, sizeof idle_data, &err) == ALDIS_OK &&
              idle_data[0] == 0xFF && idle_data[4] == 0x6D && idle_data[5] == 0xB6,
          "OID sequence of 132.0 Annex D");
    aldis_oid_generator_deinit(oid);
    check(aldis_oid_generator_fill(oid, idle_data, sizeof idle_data, NULL) ==
              ALDIS_ERR_INVALID_HANDLE,
          "a released generator is an invalid handle (CABI-032)");

    /* A 16-octet TM Transfer Frame of VC 5 on SCID 677, through the one-shot encoder and the
     * decoder (M2 plan D14): the master channel and an array of one virtual channel. */
    aldis_tm_mc_config_t mc;
    memset(&mc, 0, sizeof mc);
    mc.scid = 677;
    mc.pc.frame_len = 16;
    aldis_tm_vc_config_t vc;
    memset(&vc, 0, sizeof vc);
    vc.vcid = 5;
    vc.content = ALDIS_TM_CONTENT_PACKETS;
    aldis_tm_fields_t tm;
    memset(&tm, 0, sizeof tm);
    tm.vcid = 5;
    tm.mc_frame_count = 1;
    tm.vc_frame_count = 2;
    tm.pointer = ALDIS_TM_POINTER_STARTS_AT;
    uint8_t frame[16];
    uint8_t tm_data[10];
    memset(tm_data, 0xD0, sizeof tm_data);
    aldis_tm_content_t content;
    memset(&content, 0, sizeof content);
    content.data = tm_data;
    content.data_len = sizeof tm_data;
    check(aldis_tm_encode(&mc, &vc, 1, &tm, &content, frame, sizeof frame, &len, &err) ==
                  ALDIS_OK &&
              len == 16,
          "aldis_tm_encode");
    aldis_tm_frame_t tm_view;
    check(aldis_tm_decode(frame, len, &mc.pc, ALDIS_MODE_STRICT, &tm_view, NULL) == ALDIS_OK,
          "aldis_tm_decode");
    check(tm_view.header.scid == 677 && tm_view.header.vcid == 5 &&
              tm_view.pointer == ALDIS_TM_POINTER_STARTS_AT &&
              tm_view.layout.data.len == 10 && frame[tm_view.layout.data.offset] == 0xD0,
          "decoded TM frame (CABI-012)");
    check(aldis_tm_validate(frame, len, &mc, &vc, 1, ALDIS_MODE_STRICT, NULL) == ALDIS_OK,
          "aldis_tm_validate");
    /* An Only Idle Data frame in one call, its data field from the OID generator. */
    aldis_oid_generator_t tm_oid_storage;
    aldis_oid_generator_t *tm_oid = &tm_oid_storage;
    aldis_tm_content_t no_content;
    memset(&no_content, 0, sizeof no_content);
    check(aldis_oid_generator_init(tm_oid, ALDIS_OID_FIBONACCI, &err) == ALDIS_OK &&
              aldis_tm_encode_oid(&mc, &vc, 1, 5, 0, 0, &no_content, tm_oid, frame, sizeof frame,
                                  &len, &err) == ALDIS_OK &&
              len == 16 && frame[6] == 0xFF && frame[7] == 0xFF,
          "aldis_tm_encode_oid");
    aldis_oid_generator_deinit(tm_oid);

    /* --- TC (M2 WP-3 module 8) ---
     * A Type-AD frame of VC 1 on SCID 0x155 through the one-shot encoder, followed by fill in
     * one data unit, delimited and validated (M2 plan D14); then a CLCW as a word. */
    check(ALDIS_TC_MAX_MAPS > 0, "TC MAP capacity");
    aldis_tc_mc_config_t tc_mc;
    memset(&tc_mc, 0, sizeof tc_mc);
    tc_mc.scid = 0x155;
    tc_mc.pc.fecf = 1;
    tc_mc.pc.max_frame_len = 64;
    aldis_tc_vc_config_t tc_vc;
    memset(&tc_vc, 0, sizeof tc_vc);
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
    const uint8_t tc_data[3] = {1, 2, 3};
    aldis_tc_content_t tc_content;
    memset(&tc_content, 0, sizeof tc_content);
    tc_content.user_data = tc_data;
    tc_content.user_data_len = sizeof tc_data;
    check(aldis_tc_encode(&tc_mc, &tc_vc, 1, NULL, 0, &tc, &tc_content, unit, sizeof unit, &len,
                          &err) == ALDIS_OK &&
              len == 10,
          "aldis_tc_encode");
    const uint8_t tc_frame[10] = {0x01, 0x55, 0x04, 0x09, 0x04, 0x01, 0x02, 0x03, 0xE0, 0xC0};
    check(memcmp(unit, tc_frame, sizeof tc_frame) == 0, "TC frame octets (tc/ad.json)");
    size_t stream_offset = 0;
    int stream_frames = 0;
    int stream_fill = 0;
    aldis_tc_stream_item_t s_item;
    while (aldis_tc_stream_next(unit, sizeof unit, &stream_offset, &tc_mc, &tc_vc, 1, NULL, 0,
                                ALDIS_MODE_STRICT, &s_item, &err) == ALDIS_OK &&
           s_item.kind != ALDIS_TC_STREAM_END) {
        if (s_item.kind == ALDIS_TC_STREAM_ACCEPTED) {
            check(s_item.frame.header.sequence_number == 4 && s_item.frame.layout.data.len == 3 &&
                      unit[s_item.range.offset + s_item.frame.layout.data.offset] == 1,
                  "stream TC frame (CABI-012)");
            stream_frames++;
        } else if (s_item.kind == ALDIS_TC_STREAM_DISCARDED) {
            if (s_item.discard_reason == ALDIS_TC_DISCARD_FILL) {
                stream_fill++;
            }
        }
    }
    check(stream_frames == 1 && stream_fill == 1 && stream_offset == sizeof unit, "aldis_tc_stream_next");
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
     * one-shot encoder, the decoder, the validation checks, and the delimiter (M2 plan D14):
     * the physical channel and arrays of one virtual channel and one MAP. */
    aldis_uslp_config_t upc;
    memset(&upc, 0, sizeof upc);
    upc.frame_type = ALDIS_USLP_FRAME_VARIABLE;
    upc.fecf = 1;
    upc.frame_len = 64;
    aldis_uslp_vc_config_t uvc;
    memset(&uvc, 0, sizeof uvc);
    uvc.scid = 0x155;
    uvc.vcid = 4;
    uvc.frame_type = ALDIS_USLP_FRAME_VARIABLE;
    uvc.security = ALDIS_USLP_SECURITY_VC;
    aldis_uslp_map_config_t umap;
    memset(&umap, 0, sizeof umap);
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
    uint8_t utfdz[4];
    memset(utfdz, 0xD0, sizeof utfdz);
    ucontent.tfdz = utfdz;
    ucontent.tfdz_len = sizeof utfdz;
    size_t ulen = 0;
    check(aldis_uslp_encode(&upc, &uvc, 1, &umap, 1, &ufields, &ucontent, uframe, sizeof uframe,
                            &ulen, &err) == ALDIS_OK &&
              ulen == 14,
          "aldis_uslp_encode");
    aldis_uslp_frame_t uview;
    check(aldis_uslp_decode(uframe, ulen, &upc, &uvc, 1, &umap, 1, ALDIS_MODE_STRICT, &uview,
                            NULL) == ALDIS_OK,
          "aldis_uslp_decode");
    check(!uview.truncated && uview.channel.scid == 0x155 && uview.channel.vcid == 4 &&
              uview.tfdf.rule == ALDIS_USLP_RULE_OCTET_STREAM && uview.upid_assigned &&
              uview.layout.tfdz.len == 4 && uframe[uview.layout.tfdz.offset] == 0xD0,
          "decoded USLP frame (CABI-012)");
    uframe[ulen - 1] ^= 1;
    size_t ustream_offset = 0;
    aldis_uslp_stream_item_t us_item;
    check(aldis_uslp_stream_next(uframe, ulen, &ustream_offset, &upc, &uvc, 1, &umap, 1,
                                 ALDIS_MODE_STRICT, &us_item, NULL) == ALDIS_OK &&
              us_item.kind == ALDIS_USLP_STREAM_DISCARDED &&
              us_item.discard_reason == ALDIS_USLP_DISCARD_FECF_MISMATCH &&
              us_item.error_code == ALDIS_ERR_CRC_MISMATCH &&
              ustream_offset == ulen,
          "aldis_uslp_stream_next discards frame with bad FECF (USLP-014)");
    uframe[ulen - 1] ^= 1;
    ustream_offset = 0;
    check(aldis_uslp_stream_next(uframe, ulen, &ustream_offset, &upc, &uvc, 1, &umap, 1,
                                 ALDIS_MODE_STRICT, &us_item, NULL) == ALDIS_OK &&
              us_item.kind == ALDIS_USLP_STREAM_ACCEPTED &&
              us_item.range.len == ulen &&
              ustream_offset == ulen,
          "aldis_uslp_stream_next accepts valid frame");
    check(ALDIS_USLP_MAX_VCS > 0 && ALDIS_USLP_MAX_MAPS > 0,
          "USLP channel table capacities");
    /* ---- end USLP ---- */

    if (failures != 0) {
        return 1;
    }
    printf("aldis-ffi C smoke test passed\n");
    return 0;
}
