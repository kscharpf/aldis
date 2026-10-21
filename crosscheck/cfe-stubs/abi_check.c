/*
 * Part of the cFE cross-check stubs (docs/verification/cross-checks.md §1); not cFE code.
 * Fails the build if the types and values that crosscheck/src/cfe_msg.rs declares in Rust differ
 * from the ones the vendored cFE headers define. It defines no function.
 */
#include <stddef.h>

#include "cfe_msg.h"

_Static_assert(sizeof(CFE_Status_t) == 4, "CFE_Status_t is i32");
_Static_assert(CFE_SUCCESS == 0, "CFE_SUCCESS");
_Static_assert(CFE_MSG_BAD_ARGUMENT == (CFE_Status_t)0xca000003, "CFE_MSG_BAD_ARGUMENT");
_Static_assert(CFE_MSG_WRONG_MSG_TYPE == (CFE_Status_t)0xca00000d, "CFE_MSG_WRONG_MSG_TYPE");
_Static_assert(sizeof(CFE_MSG_Type_t) == sizeof(int), "CFE_MSG_Type_t is c_int");
_Static_assert(CFE_MSG_Type_Cmd == 1 && CFE_MSG_Type_Tlm == 2, "CFE_MSG_Type_t values");
_Static_assert(sizeof(CFE_MSG_SegmentationFlag_t) == sizeof(int), "SegmentationFlag is c_int");
_Static_assert(CFE_MSG_SegFlag_Continue == 1 && CFE_MSG_SegFlag_First == 2 &&
                   CFE_MSG_SegFlag_Last == 3 && CFE_MSG_SegFlag_Unsegmented == 4,
               "CFE_MSG_SegmentationFlag_t values");
_Static_assert(sizeof(CFE_MSG_Size_t) == sizeof(size_t), "CFE_MSG_Size_t is usize");
_Static_assert(sizeof(CFE_MSG_HeaderVersion_t) == 2 && sizeof(CFE_MSG_ApId_t) == 2 &&
                   sizeof(CFE_MSG_SequenceCount_t) == 2,
               "header field types are u16");
_Static_assert(sizeof(CFE_SB_MsgId_t) == 4, "CFE_SB_MsgId_t is a struct of one u32");
_Static_assert(sizeof(CFE_TIME_SysTime_t) == 8, "CFE_TIME_SysTime_t is two u32");
_Static_assert(sizeof(CFE_MSG_Message_t) == 6, "the primary header only");
_Static_assert(offsetof(CFE_MSG_TelemetryHeader_t, Sec) == 6, "time right after the header");
_Static_assert(sizeof(CFE_MSG_TelemetryHeader_t) == 16, "the telemetry header with its spare");
