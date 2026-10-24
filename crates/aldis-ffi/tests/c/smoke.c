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

int main(void) {
    /* CABI-030: aldis_limits reports the build capacities. */
    aldis_limits_t limits;
    memset(&limits, 0, sizeof limits);
    limits.struct_size = (uint32_t)sizeof limits;
    check(aldis_limits(&limits) == ALDIS_OK, "aldis_limits");
    check(limits.spp_tracker_max_apids == ALDIS_SPP_TRACKER_MAX_APIDS, "tracker capacity");

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
    aldis_tm_frame_t tm_view;
    check(aldis_tm_decode(frame, len, &mc.pc, ALDIS_MODE_STRICT, &tm_view, NULL) == ALDIS_OK,
          "aldis_tm_decode");
    check(tm_view.header.scid == 677 && tm_view.header.vcid == 5 &&
              tm_view.pointer == ALDIS_TM_POINTER_STARTS_AT &&
              tm_view.layout.data_len == 10 && frame[tm_view.layout.data_offset] == 0xD0,
          "decoded TM frame (CABI-012)");
    check(aldis_tm_validate_with(frame, len, &mc, &vc, 1, ALDIS_MODE_STRICT, NULL) == ALDIS_OK,
          "aldis_tm_validate_with");

    if (failures != 0) {
        return 1;
    }
    printf("aldis-ffi C smoke test passed\n");
    return 0;
}
