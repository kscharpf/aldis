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

    if (failures != 0) {
        return 1;
    }
    printf("aldis-ffi C smoke test passed\n");
    return 0;
}
