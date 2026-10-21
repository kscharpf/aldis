/*
 * STUB for the cFE cross-check (docs/verification/cross-checks.md §1); not cFE code.
 * The Software Bus message identifier, as cFE defines it without EDS
 * (modules/sb/config/default_cfe_sb_extern_typedefs.h and
 * modules/core_api/fsw/inc/cfe_sb_api_typedefs.h): a struct holding a uint32, where 0 is the
 * invalid identifier.
 */
#ifndef ALDIS_CFE_STUB_CFE_SB_API_TYPEDEFS_H
#define ALDIS_CFE_STUB_CFE_SB_API_TYPEDEFS_H

#include "common_types.h"

typedef uint32 CFE_SB_MsgId_Atom_t;

typedef struct
{
    CFE_SB_MsgId_Atom_t Value;
} CFE_SB_MsgId_t;

#define CFE_SB_MSGID_WRAP_VALUE(val) {val}
#define CFE_SB_MSGID_C(val)          ((CFE_SB_MsgId_t)CFE_SB_MSGID_WRAP_VALUE(val))
#define CFE_SB_MSGID_UNWRAP_VALUE(mid) ((mid).Value)
#define CFE_SB_INVALID_MSG_ID        CFE_SB_MSGID_C(0)

#endif
