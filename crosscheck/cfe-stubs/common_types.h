/*
 * STUB for the cFE cross-check (docs/verification/cross-checks.md §1); not cFE or OSAL code.
 * Replaces OSAL's common_types.h with the fixed-width types that the vendored cFE MSG sources
 * use, mapped to <stdint.h> as OSAL maps them.
 */
#ifndef ALDIS_CFE_STUB_COMMON_TYPES_H
#define ALDIS_CFE_STUB_COMMON_TYPES_H

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef int8_t   int8;
typedef int16_t  int16;
typedef int32_t  int32;
typedef int64_t  int64;
typedef uint8_t  uint8;
typedef uint16_t uint16;
typedef uint32_t uint32;
typedef uint64_t uint64;

#endif
