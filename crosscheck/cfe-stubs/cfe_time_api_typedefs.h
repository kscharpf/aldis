/*
 * STUB for the cFE cross-check (docs/verification/cross-checks.md §1); not cFE code.
 * The cFE system time, as cFE defines it without EDS
 * (modules/time/config/default_cfe_time_extern_typedefs.h): seconds, and subseconds in units of
 * 2^-32 s.
 */
#ifndef ALDIS_CFE_STUB_CFE_TIME_API_TYPEDEFS_H
#define ALDIS_CFE_STUB_CFE_TIME_API_TYPEDEFS_H

#include "common_types.h"

typedef struct CFE_TIME_SysTime
{
    uint32 Seconds;
    uint32 Subseconds;
} CFE_TIME_SysTime_t;

#endif
