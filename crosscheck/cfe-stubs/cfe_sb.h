/*
 * STUB for the cFE cross-check (docs/verification/cross-checks.md §1); not cFE code.
 * The three Software Bus helpers that the vendored cfe_msg_init.c and cfe_msg_msgid_v1.c call,
 * with cFE's semantics (modules/core_api/fsw/inc/cfe_sb.h and
 * modules/sb/fsw/src/cfe_sb_msg_id_util.c, with the default highest valid identifier 0x1FFF of
 * modules/sb/fsw/inc/cfe_sb_internal_cfg.h). The harness passes only CFE_SB_INVALID_MSG_ID to
 * CFE_MSG_Init, so CFE_MSG_SetMsgId is never reached; it is linked because CFE_MSG_Init calls it.
 */
#ifndef ALDIS_CFE_STUB_CFE_SB_H
#define ALDIS_CFE_STUB_CFE_SB_H

#include "cfe_sb_api_typedefs.h"

static inline CFE_SB_MsgId_Atom_t CFE_SB_MsgIdToValue(CFE_SB_MsgId_t MsgId)
{
    return CFE_SB_MSGID_UNWRAP_VALUE(MsgId);
}

static inline CFE_SB_MsgId_t CFE_SB_ValueToMsgId(CFE_SB_MsgId_Atom_t MsgIdValue)
{
    CFE_SB_MsgId_t Result = CFE_SB_MSGID_WRAP_VALUE(MsgIdValue);
    return Result;
}

static inline bool CFE_SB_IsValidMsgId(CFE_SB_MsgId_t MsgId)
{
    return CFE_SB_MsgIdToValue(MsgId) != 0 && CFE_SB_MsgIdToValue(MsgId) <= 0x1FFF;
}

#endif
