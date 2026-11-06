/*
 * C vector harness of the aldis-ffi library (VER-026, GEN-071): runs live cases of the shared
 * vectors of test-vectors/ through the C ABI of include/aldis/aldis.h. `cargo xtask sanitizers`
 * builds it with -fsanitize=address,undefined and runs it with the path of test-vectors/.
 *
 * It covers the codecs CRC-16, Space Packet (decode, peek_length, encode), Encapsulation Packet
 * (decode, peek_length), and TM frame (decode, peek_length), and the engine handles of the
 * Space Packet sequence tracker and FARM-1 (caller-provided storage, CABI-031). Every input is
 * copied to a heap block of exactly its length, so that a read past it is an ASan error. A
 * case with an operation or a field this harness does not handle is counted as skipped and
 * printed; the Rust harness `crates/aldis-ffi/tests/vectors.rs` runs every case.
 *
 * Plain C99: the common subset that the header checks of `cargo xtask header` compile.
 */
#include "aldis/aldis.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* ---- A JSON reader: the values of the vector files, nothing more. ---- */

typedef enum { J_NULL, J_BOOL, J_NUM, J_STR, J_ARR, J_OBJ } jtype;

typedef struct jval {
    jtype type;
    double num;
    char *str;           /* J_STR */
    char *key;           /* a member of a J_OBJ: its key */
    struct jval *items;  /* J_ARR and J_OBJ: `count` values */
    size_t count;
} jval;

static int parse_failed = 0;

static void skip_ws(const char **p) {
    while (**p == ' ' || **p == '\n' || **p == '\t' || **p == '\r') {
        (*p)++;
    }
}

static void *xrealloc(void *p, size_t n) {
    void *q = realloc(p, n);
    if (q == NULL) {
        (void)fputs("out of memory\n", stderr);
        exit(2);
    }
    return q;
}

static char *parse_string(const char **p) {
    size_t cap = 16;
    size_t n = 0;
    char *s = (char *)xrealloc(NULL, cap);
    (*p)++; /* the opening quote */
    while (**p != '"' && **p != '\0') {
        char c = **p;
        if (c == '\\') {
            (*p)++;
            c = **p;
            if (c == 'u') { /* a \uXXXX escape: the harness never compares such a string */
                *p += 4;
                c = '?';
            } else if (c == 'n') {
                c = '\n';
            } else if (c == 't') {
                c = '\t';
            }
        }
        if (n + 2 > cap) {
            cap *= 2;
            s = (char *)xrealloc(s, cap);
        }
        s[n++] = c;
        if (**p != '\0') {
            (*p)++;
        }
    }
    if (**p == '"') {
        (*p)++;
    } else {
        parse_failed = 1;
    }
    s[n] = '\0';
    return s;
}

static jval parse_value(const char **p);

static jval parse_container(const char **p, char close, int is_obj) {
    jval v;
    memset(&v, 0, sizeof v);
    v.type = is_obj ? J_OBJ : J_ARR;
    (*p)++;
    skip_ws(p);
    if (**p == close) {
        (*p)++;
        return v;
    }
    for (;;) {
        char *key = NULL;
        skip_ws(p);
        if (is_obj) {
            if (**p != '"') {
                parse_failed = 1;
                return v;
            }
            key = parse_string(p);
            skip_ws(p);
            if (**p != ':') {
                parse_failed = 1;
                free(key);
                return v;
            }
            (*p)++;
            skip_ws(p);
        }
        jval item = parse_value(p);
        item.key = key;
        v.items = (jval *)xrealloc(v.items, (v.count + 1) * sizeof(jval));
        v.items[v.count++] = item;
        skip_ws(p);
        if (**p == ',') {
            (*p)++;
        } else if (**p == close) {
            (*p)++;
            return v;
        } else {
            parse_failed = 1;
            return v;
        }
    }
}

static jval parse_value(const char **p) {
    jval v;
    memset(&v, 0, sizeof v);
    skip_ws(p);
    if (**p == '{') {
        return parse_container(p, '}', 1);
    }
    if (**p == '[') {
        return parse_container(p, ']', 0);
    }
    if (**p == '"') {
        v.type = J_STR;
        v.str = parse_string(p);
        return v;
    }
    if (strncmp(*p, "true", 4) == 0 || strncmp(*p, "false", 5) == 0) {
        v.type = J_BOOL;
        v.num = **p == 't';
        *p += v.num != 0 ? 4 : 5;
        return v;
    }
    if (strncmp(*p, "null", 4) == 0) {
        *p += 4;
        return v;
    }
    {
        char *end = NULL;
        v.type = J_NUM;
        v.num = strtod(*p, &end);
        if (end == *p) {
            parse_failed = 1;
            end = (char *)*p + 1;
        }
        *p = end;
    }
    return v;
}

static void jfree(jval *v) {
    size_t i;
    for (i = 0; i < v->count; i++) {
        jfree(&v->items[i]);
    }
    free(v->items);
    free(v->str);
    free(v->key);
}

static const jval *jget(const jval *o, const char *key) {
    size_t i;
    if (o == NULL || o->type != J_OBJ) {
        return NULL;
    }
    for (i = 0; i < o->count; i++) {
        if (strcmp(o->items[i].key, key) == 0) {
            return &o->items[i];
        }
    }
    return NULL;
}

static const char *jstr(const jval *o, const char *key) {
    const jval *v = jget(o, key);
    return v != NULL && v->type == J_STR ? v->str : NULL;
}

static int jhas_num(const jval *o, const char *key) {
    const jval *v = jget(o, key);
    return v != NULL && v->type == J_NUM;
}

static unsigned long long jnum(const jval *o, const char *key) {
    const jval *v = jget(o, key);
    return v != NULL && (v->type == J_NUM || v->type == J_BOOL) ? (unsigned long long)v->num : 0;
}

/* ---- Helpers ---- */

static int failures = 0;
static int ran = 0;
static int skipped = 0;
static const char *case_id = "";

static void fail(const char *what) {
    printf("FAIL %s: %s\n", case_id, what);
    failures++;
}

static void expect_eq(unsigned long long got, unsigned long long want, const char *what) {
    if (got != want) {
        printf("FAIL %s: %s is %llu, expected %llu\n", case_id, what, got, want);
        failures++;
    }
}

/* Compares a field of the expectation, when the vector has one, with the decoded value. */
static void expect_field(const jval *ex, const char *key, unsigned long long got) {
    if (jhas_num(ex, key) || (jget(ex, key) != NULL && jget(ex, key)->type == J_BOOL)) {
        expect_eq(got, jnum(ex, key), key);
    }
}

static int hexval(char c) {
    if (c >= '0' && c <= '9') {
        return c - '0';
    }
    if (c >= 'a' && c <= 'f') {
        return c - 'a' + 10;
    }
    if (c >= 'A' && c <= 'F') {
        return c - 'A' + 10;
    }
    return -1;
}

/* Decodes hex into a heap block of exactly the length of the octets (at least 1 byte). */
static uint8_t *hex_to_heap(const char *hex, size_t *len) {
    size_t n = hex == NULL ? 0 : strlen(hex) / 2;
    size_t i;
    uint8_t *buf = (uint8_t *)xrealloc(NULL, n == 0 ? 1 : n);
    for (i = 0; i < n; i++) {
        buf[i] = (uint8_t)(hexval(hex[2 * i]) * 16 + hexval(hex[2 * i + 1]));
    }
    *len = n;
    return buf;
}

static void expect_hex(const uint8_t *got, size_t got_len, const char *want_hex, const char *what) {
    size_t want_len = 0;
    uint8_t *want = hex_to_heap(want_hex, &want_len);
    if (got_len != want_len || (want_len > 0 && memcmp(got, want, want_len) != 0)) {
        printf("FAIL %s: %s differs\n", case_id, what);
        failures++;
    }
    free(want);
}

static const struct {
    const char *name;
    uint32_t code;
} kinds[] = {
    {"BufferTooSmall", ALDIS_ERR_BUFFER_TOO_SMALL},
    {"Incomplete", ALDIS_ERR_INCOMPLETE},
    {"LengthMismatch", ALDIS_ERR_LENGTH_MISMATCH},
    {"UnsupportedVersion", ALDIS_ERR_UNSUPPORTED_VERSION},
    {"OutOfRange", ALDIS_ERR_OUT_OF_RANGE},
    {"InvalidField", ALDIS_ERR_INVALID_FIELD},
    {"ReservedValue", ALDIS_ERR_RESERVED_VALUE},
    {"CrcMismatch", ALDIS_ERR_CRC_MISMATCH},
    {"HeaderUncorrectable", ALDIS_ERR_HEADER_UNCORRECTABLE},
    {"UnknownChannel", ALDIS_ERR_UNKNOWN_CHANNEL},
    {"InvalidConfig", ALDIS_ERR_INVALID_CONFIG},
    {"InvalidState", ALDIS_ERR_INVALID_STATE},
    {"CapacityExceeded", ALDIS_ERR_CAPACITY_EXCEEDED},
    {"Malformed", ALDIS_ERR_MALFORMED},
};

/* The outcome of a call: `ok` of the expectation against the status, the error kind and field
 * against the detail (CABI-020, CABI-021). Returns 1 when the call is to succeed. */
static int check_status(uint32_t status, const aldis_error_detail_t *err, const jval *ex) {
    const jval *ok = jget(ex, "ok");
    int want_ok = ok != NULL && ok->num != 0;
    if (want_ok) {
        expect_eq(status, ALDIS_OK, "status of a successful case");
        return status == ALDIS_OK;
    }
    {
        const char *name = jstr(ex, "error");
        size_t i;
        int found = 0;
        for (i = 0; name != NULL && i < sizeof kinds / sizeof kinds[0]; i++) {
            if (strcmp(kinds[i].name, name) == 0) {
                expect_eq(status, kinds[i].code, "error status");
                found = 1;
            }
        }
        if (!found) {
            fail("an error kind this harness does not know");
        }
        expect_eq(err->kind, status, "detail kind equals the status");
        if (jhas_num(ex, "field")) {
            expect_eq(err->field, jnum(ex, "field"), "field code");
        }
    }
    return 0;
}

/* ---- Operations ---- */

/* Returns 1 when the case ran, 0 when it is of an operation the harness does not handle. */
typedef int (*run_fn)(const jval *c);

static int run_crc16(const jval *c) {
    size_t len = 0;
    uint8_t *in = hex_to_heap(jstr(c, "input_hex"), &len);
    const jval *ex = jget(c, "expect");
    aldis_error_detail_t err;
    uint16_t crc = 0;
    memset(&err, 0, sizeof err);
    if (strcmp(jstr(c, "operation"), "crc16") != 0) {
        free(in);
        return 0;
    }
    if (check_status(aldis_crc16(in, len, &crc, &err), &err, ex)) {
        uint8_t be[2];
        be[0] = (uint8_t)(crc >> 8);
        be[1] = (uint8_t)crc;
        expect_hex(be, 2, jstr(ex, "value_hex"), "crc");
    }
    free(in);
    return 1;
}

static uint32_t mode_of(const jval *c) {
    const char *m = jstr(c, "mode");
    return m != NULL && strcmp(m, "lenient") == 0 ? ALDIS_MODE_LENIENT : ALDIS_MODE_STRICT;
}

static int run_spp(const jval *c) {
    const char *op = jstr(c, "operation");
    const jval *ex = jget(c, "expect");
    aldis_error_detail_t err;
    memset(&err, 0, sizeof err);
    if (strcmp(op, "decode") == 0) {
        size_t len = 0;
        uint8_t *in = hex_to_heap(jstr(c, "input_hex"), &len);
        aldis_spp_packet_t p;
        memset(&p, 0, sizeof p);
        if (check_status(aldis_spp_decode(in, len, mode_of(c), &p, &err), &err, ex)) {
            expect_field(ex, "packet_type", p.fields.packet_type);
            expect_field(ex, "secondary_header_flag", p.secondary_header_flag);
            expect_field(ex, "apid", p.fields.apid);
            expect_field(ex, "sequence_flags", p.fields.sequence_flags);
            expect_field(ex, "sequence_count", p.fields.sequence_count);
            expect_field(ex, "data_len", p.data.len);
            expect_field(ex, "packet_len", p.packet_len);
            expect_field(ex, "consumed", p.packet_len);
            expect_field(ex, "idle", p.idle);
            if (p.data.offset > len || p.data.len > len - p.data.offset) {
                fail("data range outside the input");
            } else {
                expect_hex(in + p.data.offset, p.data.len, jstr(ex, "data_hex"), "data");
            }
        }
        free(in);
        return 1;
    }
    if (strcmp(op, "peek_length") == 0) {
        size_t len = 0;
        size_t total = 0;
        uint8_t *in = hex_to_heap(jstr(c, "input_hex"), &len);
        if (check_status(aldis_spp_peek_length(in, len, &total, &err), &err, ex)) {
            expect_eq(total, jnum(ex, "len"), "len");
        }
        free(in);
        return 1;
    }
    if (strcmp(op, "encode") == 0) {
        const jval *p = jget(c, "params");
        const jval *cfgj = jget(c, "config");
        aldis_spp_config_t cfg;
        aldis_spp_fields_t f;
        size_t sec_len = 0;
        size_t user_len = 0;
        size_t n = 0;
        uint8_t *sec = hex_to_heap(jstr(p, "secondary_header_hex"), &sec_len);
        uint8_t *user = hex_to_heap(jstr(p, "user_data_hex"), &user_len);
        uint8_t out[ALDIS_SPP_MAX_PACKET_LEN];
        cfg.max_packet_len = cfgj != NULL ? (uint32_t)jnum(cfgj, "max_packet_len")
                                          : (uint32_t)ALDIS_SPP_MAX_PACKET_LEN;
        f.packet_type = (uint8_t)jnum(p, "packet_type");
        f.sequence_flags = (uint8_t)jnum(p, "sequence_flags");
        f.apid = (uint16_t)jnum(p, "apid");
        f.sequence_count = (uint16_t)jnum(p, "sequence_count");
        if (check_status(aldis_spp_encode(&cfg, &f, sec, sec_len, user, user_len, out, sizeof out,
                                          &n, &err),
                         &err, ex)) {
            expect_hex(out, n, jstr(ex, "output_hex"), "output");
        }
        free(sec);
        free(user);
        return 1;
    }
    if (strcmp(op, "track") == 0) {
        const jval *p = jget(c, "params");
        const jval *events = jget(p, "events");
        const jval *results = jget(jget(c, "expect"), "results");
        aldis_spp_tracker_t storage;
        aldis_spp_tracker_config_t cfg;
        size_t i;
        memset(&storage, 0xA5, sizeof storage);
        cfg.capacity = (uint32_t)jnum(p, "capacity");
        if (aldis_spp_tracker_init(&storage, &cfg, &err) != ALDIS_OK) {
            fail("aldis_spp_tracker_init");
            return 1;
        }
        for (i = 0; events != NULL && results != NULL && i < events->count && i < results->count;
             i++) {
            const jval *ev = &events->items[i];
            const jval *want = &results->items[i];
            if (strcmp(jstr(ev, "op"), "track") == 0) {
                aldis_spp_tracked_t t;
                const char *w = jstr(want, "continuity");
                uint32_t status;
                memset(&t, 0, sizeof t);
                status = aldis_spp_tracker_track(&storage, (uint16_t)jnum(ev, "apid"),
                                                 (uint16_t)jnum(ev, "count"), &t, &err);
                if (jstr(want, "error") != NULL) {
                    expect_eq(check_status(status, &err, want), 0, "an error step");
                } else if (status != ALDIS_OK) {
                    fail("aldis_spp_tracker_track");
                } else if (w == NULL) {
                    fail("tracker step without a continuity");
                } else if (strcmp(w, "First") == 0) {
                    expect_eq(t.continuity, ALDIS_SPP_TRACK_FIRST, "continuity");
                } else if (strcmp(w, "InSequence") == 0) {
                    expect_eq(t.continuity, ALDIS_SPP_TRACK_IN_SEQUENCE, "continuity");
                } else if (strcmp(w, "Repeat") == 0) {
                    expect_eq(t.continuity, ALDIS_SPP_TRACK_REPEAT, "continuity");
                } else if (strcmp(w, "Backwards") == 0) {
                    expect_eq(t.continuity, ALDIS_SPP_TRACK_BACKWARDS, "continuity");
                } else {
                    expect_eq(t.continuity, ALDIS_SPP_TRACK_GAP, "continuity");
                    expect_eq(t.missing, jnum(want, "missing"), "missing");
                }
            } else {
                bool forgot = false;
                if (aldis_spp_tracker_forget(&storage, (uint16_t)jnum(ev, "apid"), &forgot,
                                             &err) != ALDIS_OK) {
                    fail("aldis_spp_tracker_forget");
                } else {
                    expect_eq(forgot, jnum(want, "forgot"), "forgot");
                }
            }
        }
        aldis_spp_tracker_deinit(&storage);
        return 1;
    }
    return 0;
}

static int run_epp(const jval *c) {
    const char *op = jstr(c, "operation");
    const jval *ex = jget(c, "expect");
    aldis_error_detail_t err;
    size_t len = 0;
    uint8_t *in;
    memset(&err, 0, sizeof err);
    if (strcmp(op, "decode") != 0 && strcmp(op, "peek_length") != 0) {
        return 0;
    }
    in = hex_to_heap(jstr(c, "input_hex"), &len);
    if (strcmp(op, "decode") == 0) {
        aldis_epp_packet_t p;
        memset(&p, 0, sizeof p);
        if (check_status(aldis_epp_decode(in, len, mode_of(c), &p, &err), &err, ex)) {
            expect_field(ex, "epi", p.epi);
            expect_field(ex, "header_len", p.header_len);
            expect_field(ex, "packet_len", p.packet_len);
            expect_field(ex, "consumed", p.packet_len);
            if (p.data.offset > len || p.data.len > len - p.data.offset) {
                fail("data range outside the input");
            } else {
                expect_hex(in + p.data.offset, p.data.len, jstr(ex, "data_hex"), "data");
            }
        }
    } else {
        size_t total = 0;
        if (check_status(aldis_epp_peek_length(in, len, &total, &err), &err, ex)) {
            expect_eq(total, jnum(ex, "len"), "len");
        }
    }
    free(in);
    return 1;
}

static int run_tm(const jval *c) {
    const char *op = jstr(c, "operation");
    const jval *ex = jget(c, "expect");
    const jval *cfgj = jget(c, "config");
    aldis_error_detail_t err;
    aldis_tm_config_t cfg;
    size_t len = 0;
    uint8_t *in;
    memset(&err, 0, sizeof err);
    if ((strcmp(op, "decode") != 0 && strcmp(op, "peek_length") != 0) || cfgj == NULL ||
        jget(cfgj, "scid") != NULL) {
        return 0;
    }
    cfg.fecf = (uint8_t)jnum(cfgj, "fecf");
    cfg.frame_len = (size_t)jnum(cfgj, "frame_len");
    in = hex_to_heap(jstr(c, "input_hex"), &len);
    if (strcmp(op, "decode") == 0) {
        aldis_tm_frame_t f;
        memset(&f, 0, sizeof f);
        if (check_status(aldis_tm_decode(in, len, &cfg, mode_of(c), &f, &err), &err, ex)) {
            expect_field(ex, "scid", f.header.scid);
            expect_field(ex, "vcid", f.header.vcid);
            expect_field(ex, "ocf_flag", f.header.ocf_flag);
            expect_field(ex, "mc_frame_count", f.header.mc_frame_count);
            expect_field(ex, "vc_frame_count", f.header.vc_frame_count);
            expect_field(ex, "first_header_pointer", f.header.first_header_pointer);
            expect_field(ex, "consumed", f.layout.frame_len);
            if (f.layout.data.offset > len || f.layout.data.len > len - f.layout.data.offset) {
                fail("data range outside the input");
            } else {
                expect_hex(in + f.layout.data.offset, f.layout.data.len, jstr(ex, "data_hex"),
                           "data");
            }
        }
    } else {
        size_t total = 0;
        if (check_status(aldis_tm_peek_length(in, len, &cfg, &total, &err), &err, ex)) {
            expect_eq(total, jnum(ex, "len"), "len");
        }
    }
    free(in);
    return 1;
}

/* FARM-1 (232.1 §6): the scenario "cop1-farm1-clcw-fields" only. An AD frame is driven by its
 * sequence number N(S) (octet 4 of the frame) and the data its expectation delivers; a query
 * compares the CLCW the engine reports (COP-035) with the vector's clcw_hex. */
static int run_farm1(const jval *file) {
    const jval *scenarios = jget(file, "scenarios");
    const jval *cfgj;
    size_t s;
    for (s = 0; scenarios != NULL && s < scenarios->count; s++) {
        const jval *sc = &scenarios->items[s];
        const jval *steps = jget(sc, "steps");
        aldis_farm1_t storage;
        aldis_farm1_config_t cfg;
        aldis_error_detail_t err;
        size_t i;
        if (strcmp(jstr(sc, "id"), "cop1-farm1-clcw-fields") != 0) {
            continue;
        }
        case_id = jstr(sc, "id");
        cfgj = jget(sc, "config");
        memset(&err, 0, sizeof err);
        memset(&storage, 0xA5, sizeof storage);
        cfg.vc_id = (uint8_t)jnum(cfgj, "vcid");
        cfg.status = (uint8_t)jnum(cfgj, "status_field");
        cfg.retransmission_allowed = (uint8_t)jnum(jget(cfgj, "window"), "retransmission_allowed");
        cfg.window = (uint16_t)jnum(jget(cfgj, "window"), "w");
        cfg.positive_window = 0;
        cfg.v_r = (uint8_t)jnum(cfgj, "v_r");
        if (aldis_farm1_init(&storage, &cfg, &err) != ALDIS_OK) {
            fail("aldis_farm1_init");
            return 1;
        }
        for (i = 0; steps != NULL && i < steps->count; i++) {
            const jval *st = &steps->items[i];
            const jval *in = jget(st, "input");
            const jval *list = jget(st, "expect");
            const jval *e0 = list != NULL && list->count > 0 ? &list->items[0] : NULL;
            uint64_t now = jnum(st, "at_us");
            if (jget(in, "query") != NULL) {
                aldis_farm1_status_t status;
                uint8_t be[4];
                memset(&status, 0, sizeof status);
                if (aldis_farm1_status(&storage, &status, &err) != ALDIS_OK) {
                    fail("aldis_farm1_status");
                    continue;
                }
                be[0] = (uint8_t)(status.clcw >> 24);
                be[1] = (uint8_t)(status.clcw >> 16);
                be[2] = (uint8_t)(status.clcw >> 8);
                be[3] = (uint8_t)status.clcw;
                expect_hex(be, 4, jstr(e0, "clcw_hex"), "clcw");
            } else if (jget(in, "frame") != NULL) {
                const jval *frame = jget(in, "frame");
                const jval *dl = jget(e0, "deliver");
                size_t flen = 0;
                size_t dlen = 0;
                uint8_t *fr = hex_to_heap(jstr(frame, "frame_hex"), &flen);
                uint8_t *data = hex_to_heap(jstr(dl, "data_hex"), &dlen);
                uint8_t out[16];
                aldis_farm1_input_t input;
                aldis_farm1_actions_t actions;
                memset(&input, 0, sizeof input);
                memset(&actions, 0, sizeof actions);
                input.kind = ALDIS_FARM1_INPUT_FRAME_AD;
                input.sequence_number = flen > 4 ? fr[4] : 0;
                input.vc_id = cfg.vc_id;
                input.buffer_available = (uint8_t)jnum(frame, "buffer_available");
                input.data = data;
                input.data_len = dlen;
                if (aldis_farm1_step(&storage, &input, now, out, sizeof out, &actions, &err) !=
                    ALDIS_OK) {
                    fail("aldis_farm1_step");
                } else {
                    expect_eq(actions.count, 1, "one action (Deliver)");
                    if (actions.count == 1) {
                        expect_eq(actions.items[0].kind, ALDIS_FARM1_ACTION_DELIVER, "action");
                        if (actions.items[0].data.offset > sizeof out ||
                            actions.items[0].data.len > sizeof out - actions.items[0].data.offset) {
                            fail("delivered range outside the output");
                        } else {
                            expect_hex(out + actions.items[0].data.offset,
                                       actions.items[0].data.len, jstr(dl, "data_hex"),
                                       "delivered data");
                        }
                    }
                }
                free(fr);
                free(data);
            } else {
                fail("a farm1 input this harness does not handle");
            }
        }
        aldis_farm1_deinit(&storage);
        ran++;
    }
    return 1;
}

/* ---- Files ---- */

static char *read_file(const char *path) {
    FILE *f = fopen(path, "rb");
    char *buf = NULL;
    size_t n = 0;
    size_t cap = 0;
    if (f == NULL) {
        return NULL;
    }
    for (;;) {
        size_t got;
        if (n + 4097 > cap) {
            cap = cap == 0 ? 8192 : cap * 2;
            buf = (char *)xrealloc(buf, cap);
        }
        got = fread(buf + n, 1, 4096, f);
        n += got;
        if (got == 0) {
            break;
        }
    }
    (void)fclose(f);
    buf[n] = '\0';
    return buf;
}

static void run_file(const char *root, const char *rel, run_fn run) {
    char path[512];
    char *text;
    const char *p;
    jval file;
    size_t i;
    int file_ran = 0;
    snprintf(path, sizeof path, "%s/%s", root, rel);
    text = read_file(path);
    if (text == NULL) {
        printf("FAIL: cannot read %s\n", path);
        failures++;
        return;
    }
    p = text;
    parse_failed = 0;
    file = parse_value(&p);
    if (parse_failed) {
        printf("FAIL: cannot parse %s\n", path);
        failures++;
    } else if (run == NULL) {
        run_farm1(&file);
    } else {
        const jval *cases = jget(&file, "cases");
        for (i = 0; cases != NULL && i < cases->count; i++) {
            const jval *c = &cases->items[i];
            if (jget(c, "superseded") != NULL) {
                continue;
            }
            case_id = jstr(c, "id");
            if (run(c)) {
                ran++;
                file_ran++;
            } else {
                skipped++;
            }
        }
        printf("%-28s %d cases\n", rel, file_ran);
    }
    jfree(&file);
    free(text);
}

int main(int argc, char **argv) {
    const char *root = argc > 1 ? argv[1] : "test-vectors";
    run_file(root, "core/crc16.json", run_crc16);
    run_file(root, "spp/decode_valid.json", run_spp);
    run_file(root, "spp/decode_invalid.json", run_spp);
    run_file(root, "spp/encode.json", run_spp);
    run_file(root, "spp/sequence_tracker.json", run_spp);
    run_file(root, "epp/header_1.json", run_epp);
    run_file(root, "epp/header_2.json", run_epp);
    run_file(root, "epp/invalid.json", run_epp);
    run_file(root, "tm/minimal.json", run_tm);
    run_file(root, "tm/invalid.json", run_tm);
    run_file(root, "cop1/scenarios/farm1_flow_clcw.json", NULL);
    printf("vectors: %d run, %d skipped (operation not handled by the C harness), %d failures\n",
           ran, skipped, failures);
    return failures == 0 && ran > 0 ? 0 : 1;
}
