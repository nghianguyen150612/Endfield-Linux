#include "endfield_optimizer.h"

#include <stddef.h>

_Static_assert(ENDFIELD_OPTIMIZER_ABI_VERSION == UINT32_C(0x00010000), "ABI version");
_Static_assert(ENDFIELD_OPTIMIZER_STATUS_INTERNAL_FAILURE == UINT32_C(6), "status IDs");
_Static_assert(ENDFIELD_OPTIMIZER_GOAL_POWER_EFFICIENCY == UINT32_C(8), "goal IDs");
_Static_assert(offsetof(endfield_optimizer_capability, abi_version) == 0, "capability ABI");
_Static_assert(offsetof(endfield_optimizer_goal, abi_version) == 0, "goal ABI");
_Static_assert(offsetof(endfield_optimizer_goal, value_primary) == 16, "goal values");
_Static_assert(offsetof(endfield_optimizer_request, abi_version) == 0, "request ABI");
_Static_assert(offsetof(endfield_optimizer_request, capabilities) == 16, "request capabilities");
_Static_assert(offsetof(endfield_optimizer_plan_view, abi_version) == 0, "plan ABI");

#if UINTPTR_MAX == UINT64_MAX
_Static_assert(sizeof(endfield_optimizer_capability) == 56, "64-bit capability layout");
_Static_assert(_Alignof(endfield_optimizer_capability) == 8, "64-bit capability alignment");
_Static_assert(offsetof(endfield_optimizer_capability, value_u64) == 32, "capability value");
_Static_assert(offsetof(endfield_optimizer_capability, provenance_utf8) == 40,
    "capability provenance");
_Static_assert(sizeof(endfield_optimizer_goal) == 32, "64-bit goal layout");
_Static_assert(_Alignof(endfield_optimizer_goal) == 8, "64-bit goal alignment");
_Static_assert(sizeof(endfield_optimizer_request) == 48, "64-bit request layout");
_Static_assert(_Alignof(endfield_optimizer_request) == 8, "64-bit request alignment");
_Static_assert(offsetof(endfield_optimizer_request, goals) == 32, "request goals");
_Static_assert(sizeof(endfield_optimizer_action_view) == 32, "64-bit action layout");
_Static_assert(_Alignof(endfield_optimizer_action_view) == 8, "64-bit action alignment");
_Static_assert(sizeof(endfield_optimizer_impact_view) == 16, "64-bit impact layout");
_Static_assert(_Alignof(endfield_optimizer_impact_view) == 4, "64-bit impact alignment");
_Static_assert(sizeof(endfield_optimizer_plan_view) == 88, "64-bit plan layout");
_Static_assert(_Alignof(endfield_optimizer_plan_view) == 8, "64-bit plan alignment");
_Static_assert(offsetof(endfield_optimizer_plan_view, actions) == 24, "plan actions");
_Static_assert(offsetof(endfield_optimizer_plan_view, evidence_ids) == 56, "plan evidence");
_Static_assert(offsetof(endfield_optimizer_plan_view, uncertainty) == 80, "plan uncertainty");
#endif
