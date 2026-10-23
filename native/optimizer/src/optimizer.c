#include "endfield_optimizer.h"

#include <stddef.h>
#include <stdlib.h>
#include <string.h>

struct endfield_optimizer_plan_set {
    uint32_t count;
};

static uint32_t
abi_major(uint32_t version)
{
    return ENDFIELD_OPTIMIZER_ABI_MAJOR(version);
}

static int
is_known_platform(uint32_t platform_id)
{
    return platform_id == ENDFIELD_OPTIMIZER_PLATFORM_LINUX
        || platform_id == ENDFIELD_OPTIMIZER_PLATFORM_MACOS
        || platform_id == ENDFIELD_OPTIMIZER_PLATFORM_FREEBSD
        || platform_id == ENDFIELD_OPTIMIZER_PLATFORM_OTHER_UNKNOWN;
}

static int
is_known_capability(uint32_t capability_id)
{
    switch (capability_id) {
    case ENDFIELD_OPTIMIZER_CAPABILITY_CPU_TOPOLOGY:
    case ENDFIELD_OPTIMIZER_CAPABILITY_CPU_SCHEDULING:
    case ENDFIELD_OPTIMIZER_CAPABILITY_CPU_FREQUENCY_POWER:
    case ENDFIELD_OPTIMIZER_CAPABILITY_GPU_DEVICE:
    case ENDFIELD_OPTIMIZER_CAPABILITY_VRAM:
    case ENDFIELD_OPTIMIZER_CAPABILITY_HARDWARE_ACCELERATION:
    case ENDFIELD_OPTIMIZER_CAPABILITY_GRAPHICS_DRIVER:
    case ENDFIELD_OPTIMIZER_CAPABILITY_VULKAN_DEVICE:
    case ENDFIELD_OPTIMIZER_CAPABILITY_RAM:
    case ENDFIELD_OPTIMIZER_CAPABILITY_SWAP:
    case ENDFIELD_OPTIMIZER_CAPABILITY_CPU_THERMAL:
    case ENDFIELD_OPTIMIZER_CAPABILITY_GPU_THERMAL:
    case ENDFIELD_OPTIMIZER_CAPABILITY_POWER_TELEMETRY:
    case ENDFIELD_OPTIMIZER_CAPABILITY_DISPLAY:
    case ENDFIELD_OPTIMIZER_CAPABILITY_COMPOSITOR_SESSION:
    case ENDFIELD_OPTIMIZER_CAPABILITY_RUNTIME_DISCOVERY:
    case ENDFIELD_OPTIMIZER_CAPABILITY_OPTIMIZATION_TOOLS:
        return 1;
    default:
        return 0;
    }
}

static int
is_known_goal(uint32_t goal_id)
{
    return goal_id >= ENDFIELD_OPTIMIZER_GOAL_TARGET_FPS
        && goal_id <= ENDFIELD_OPTIMIZER_GOAL_POWER_EFFICIENCY;
}

static int
is_known_unit(uint32_t unit_id)
{
    return unit_id >= ENDFIELD_OPTIMIZER_UNIT_COUNT
        && unit_id <= ENDFIELD_OPTIMIZER_UNIT_MILLIJOULES;
}

static int
is_continuation(uint8_t byte)
{
    return (byte & UINT8_C(0xc0)) == UINT8_C(0x80);
}

static int
is_valid_utf8(const uint8_t *bytes, uint32_t length)
{
    uint32_t offset = 0;

    while (offset < length) {
        uint8_t first = bytes[offset];
        uint32_t code_point;
        uint32_t minimum;
        uint32_t sequence_length;
        uint32_t index;

        if (first <= UINT8_C(0x7f)) {
            offset += UINT32_C(1);
            continue;
        }
        if (first >= UINT8_C(0xc2) && first <= UINT8_C(0xdf)) {
            code_point = (uint32_t)(first & UINT8_C(0x1f));
            minimum = UINT32_C(0x80);
            sequence_length = UINT32_C(2);
        } else if (first >= UINT8_C(0xe0) && first <= UINT8_C(0xef)) {
            code_point = (uint32_t)(first & UINT8_C(0x0f));
            minimum = UINT32_C(0x800);
            sequence_length = UINT32_C(3);
        } else if (first >= UINT8_C(0xf0) && first <= UINT8_C(0xf4)) {
            code_point = (uint32_t)(first & UINT8_C(0x07));
            minimum = UINT32_C(0x10000);
            sequence_length = UINT32_C(4);
        } else {
            return 0;
        }

        if (sequence_length > length - offset) {
            return 0;
        }
        for (index = UINT32_C(1); index < sequence_length; ++index) {
            uint8_t continuation = bytes[offset + index];
            if (!is_continuation(continuation)) {
                return 0;
            }
            code_point = (code_point << 6) | (uint32_t)(continuation & UINT8_C(0x3f));
        }
        if (code_point < minimum
            || code_point > UINT32_C(0x10ffff)
            || (code_point >= UINT32_C(0xd800) && code_point <= UINT32_C(0xdfff))) {
            return 0;
        }
        offset += sequence_length;
    }
    return 1;
}

static uint32_t
validate_capability(const endfield_optimizer_capability *capability)
{
    if (abi_major(capability->abi_version)
        != abi_major(ENDFIELD_OPTIMIZER_ABI_VERSION)) {
        return ENDFIELD_OPTIMIZER_STATUS_UNSUPPORTED_ABI;
    }
    if (capability->struct_size < sizeof(*capability)) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    if (!is_known_capability(capability->capability_id)
        || capability->availability > ENDFIELD_OPTIMIZER_AVAILABILITY_DEGRADED
        || capability->value_kind > ENDFIELD_OPTIMIZER_VALUE_UINT64) {
        return ENDFIELD_OPTIMIZER_STATUS_UNSUPPORTED_IDENTIFIER;
    }
    if (capability->provenance_len > ENDFIELD_OPTIMIZER_MAX_PROVENANCE_BYTES
        || (capability->provenance_len > 0 && capability->provenance_utf8 == NULL)) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    if (capability->provenance_len > 0
        && !is_valid_utf8(capability->provenance_utf8, capability->provenance_len)) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }

    if (capability->value_kind == ENDFIELD_OPTIMIZER_VALUE_NONE) {
        if (capability->unit_id != ENDFIELD_OPTIMIZER_UNIT_NONE
            || capability->value_u64 != UINT64_C(0)) {
            return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
        }
    } else {
        if (!is_known_unit(capability->unit_id)) {
            return ENDFIELD_OPTIMIZER_STATUS_UNSUPPORTED_IDENTIFIER;
        }
        if (capability->availability == ENDFIELD_OPTIMIZER_AVAILABILITY_UNKNOWN
            || capability->availability == ENDFIELD_OPTIMIZER_AVAILABILITY_UNAVAILABLE) {
            return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
        }
    }
    return ENDFIELD_OPTIMIZER_STATUS_SUCCESS;
}

static uint32_t
validate_goal(const endfield_optimizer_goal *goal)
{
    if (abi_major(goal->abi_version) != abi_major(ENDFIELD_OPTIMIZER_ABI_VERSION)) {
        return ENDFIELD_OPTIMIZER_STATUS_UNSUPPORTED_ABI;
    }
    if (goal->struct_size < sizeof(*goal)) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    if (!is_known_goal(goal->goal_id)) {
        return ENDFIELD_OPTIMIZER_STATUS_UNSUPPORTED_IDENTIFIER;
    }
    if (goal->present != ENDFIELD_OPTIMIZER_GOAL_ABSENT
        && goal->present != ENDFIELD_OPTIMIZER_GOAL_PRESENT) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    if (goal->present == ENDFIELD_OPTIMIZER_GOAL_ABSENT) {
        return goal->value_primary == UINT64_C(0)
                && goal->value_secondary == UINT64_C(0)
            ? ENDFIELD_OPTIMIZER_STATUS_SUCCESS
            : ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }

    if (goal->goal_id == ENDFIELD_OPTIMIZER_GOAL_TARGET_FPS) {
        return goal->value_secondary == UINT64_C(0)
            ? ENDFIELD_OPTIMIZER_STATUS_SUCCESS
            : ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    if (goal->goal_id == ENDFIELD_OPTIMIZER_GOAL_TARGET_RESOLUTION) {
        return goal->value_primary > UINT32_MAX
                || goal->value_secondary > UINT32_MAX
                || goal->value_primary == UINT64_C(0)
                || goal->value_secondary == UINT64_C(0)
            ? ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT
            : ENDFIELD_OPTIMIZER_STATUS_SUCCESS;
    }
    return goal->value_primary <= UINT64_C(1000)
            && goal->value_secondary == UINT64_C(0)
        ? ENDFIELD_OPTIMIZER_STATUS_SUCCESS
        : ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
}

static uint32_t
validate_request(const endfield_optimizer_request *request)
{
    uint32_t index;

    if (abi_major(request->abi_version) != abi_major(ENDFIELD_OPTIMIZER_ABI_VERSION)) {
        return ENDFIELD_OPTIMIZER_STATUS_UNSUPPORTED_ABI;
    }
    if (request->struct_size < sizeof(*request)) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    if (!is_known_platform(request->platform_id)
        || request->intensity > ENDFIELD_OPTIMIZER_INTENSITY_L4) {
        return ENDFIELD_OPTIMIZER_STATUS_UNSUPPORTED_IDENTIFIER;
    }
    if (request->capability_count > ENDFIELD_OPTIMIZER_MAX_CAPABILITIES
        || request->goal_count > ENDFIELD_OPTIMIZER_MAX_GOALS
        || (request->capability_count > 0 && request->capabilities == NULL)
        || (request->goal_count > 0 && request->goals == NULL)) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }

    for (index = UINT32_C(0); index < request->capability_count; ++index) {
        uint32_t previous;
        uint32_t status = validate_capability(&request->capabilities[index]);
        if (status != ENDFIELD_OPTIMIZER_STATUS_SUCCESS) {
            return status;
        }
        for (previous = UINT32_C(0); previous < index; ++previous) {
            if (request->capabilities[previous].capability_id
                    == request->capabilities[index].capability_id
                && request->capabilities[previous].instance_id
                    == request->capabilities[index].instance_id) {
                return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
            }
        }
    }

    for (index = UINT32_C(0); index < request->goal_count; ++index) {
        uint32_t previous;
        uint32_t status = validate_goal(&request->goals[index]);
        if (status != ENDFIELD_OPTIMIZER_STATUS_SUCCESS) {
            return status;
        }
        for (previous = UINT32_C(0); previous < index; ++previous) {
            if (request->goals[previous].goal_id == request->goals[index].goal_id) {
                return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
            }
        }
    }
    return ENDFIELD_OPTIMIZER_STATUS_SUCCESS;
}

uint32_t
endfield_optimizer_abi_version(void)
{
    return ENDFIELD_OPTIMIZER_ABI_VERSION;
}

uint32_t
endfield_optimizer_plan(
    const endfield_optimizer_request *request,
    endfield_optimizer_plan_set **out_plan_set)
{
    uint32_t status;
    endfield_optimizer_plan_set *plan_set;

    if (out_plan_set == NULL) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    *out_plan_set = NULL;
    if (request == NULL) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }

    status = validate_request(request);
    if (status != ENDFIELD_OPTIMIZER_STATUS_SUCCESS) {
        return status;
    }

    plan_set = malloc(sizeof(*plan_set));
    if (plan_set == NULL) {
        return ENDFIELD_OPTIMIZER_STATUS_RESOURCE_EXHAUSTION;
    }
    plan_set->count = UINT32_C(0);
    *out_plan_set = plan_set;
    return ENDFIELD_OPTIMIZER_STATUS_SUCCESS;
}

uint32_t
endfield_optimizer_plan_count(
    const endfield_optimizer_plan_set *plan_set,
    uint32_t *out_count)
{
    if (out_count == NULL) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    *out_count = UINT32_C(0);
    if (plan_set == NULL) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    *out_count = plan_set->count;
    return ENDFIELD_OPTIMIZER_STATUS_SUCCESS;
}

uint32_t
endfield_optimizer_plan_get(
    const endfield_optimizer_plan_set *plan_set,
    uint32_t index,
    endfield_optimizer_plan_view *out_view)
{
    if (out_view == NULL) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    if (abi_major(out_view->abi_version) != abi_major(ENDFIELD_OPTIMIZER_ABI_VERSION)) {
        return ENDFIELD_OPTIMIZER_STATUS_UNSUPPORTED_ABI;
    }
    if (out_view->struct_size < sizeof(*out_view)) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }

    memset(out_view, 0, sizeof(*out_view));
    out_view->abi_version = ENDFIELD_OPTIMIZER_ABI_VERSION;
    out_view->struct_size = (uint32_t)sizeof(*out_view);
    if (plan_set == NULL) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    if (index >= plan_set->count) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }

    return ENDFIELD_OPTIMIZER_STATUS_INTERNAL_FAILURE;
}

uint32_t
endfield_optimizer_plan_set_destroy(endfield_optimizer_plan_set *plan_set)
{
    if (plan_set == NULL) {
        return ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT;
    }
    free(plan_set);
    return ENDFIELD_OPTIMIZER_STATUS_SUCCESS;
}
