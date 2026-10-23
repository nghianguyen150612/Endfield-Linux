#include "endfield_optimizer.h"

#include <assert.h>
#include <stddef.h>
#include <stdint.h>

static endfield_optimizer_request
valid_request(void)
{
    endfield_optimizer_request request = {0};
    request.abi_version = ENDFIELD_OPTIMIZER_ABI_VERSION;
    request.struct_size = (uint32_t)sizeof(request);
    request.platform_id = ENDFIELD_OPTIMIZER_PLATFORM_LINUX;
    request.intensity = ENDFIELD_OPTIMIZER_INTENSITY_L0;
    return request;
}

static endfield_optimizer_capability
valid_capability(void)
{
    endfield_optimizer_capability capability = {0};
    capability.abi_version = ENDFIELD_OPTIMIZER_ABI_VERSION;
    capability.struct_size = (uint32_t)sizeof(capability);
    capability.capability_id = ENDFIELD_OPTIMIZER_CAPABILITY_RAM;
    capability.availability = ENDFIELD_OPTIMIZER_AVAILABILITY_UNKNOWN;
    capability.value_kind = ENDFIELD_OPTIMIZER_VALUE_NONE;
    capability.unit_id = ENDFIELD_OPTIMIZER_UNIT_NONE;
    return capability;
}

static void
test_abi_version(void)
{
    assert(endfield_optimizer_abi_version() == UINT32_C(0x00010000));
}

static void
test_valid_request_and_empty_set(void)
{
    endfield_optimizer_request request = valid_request();
    endfield_optimizer_plan_set *plan_set = NULL;
    uint32_t count = UINT32_MAX;

    assert(endfield_optimizer_plan(&request, &plan_set) == ENDFIELD_OPTIMIZER_STATUS_SUCCESS);
    assert(plan_set != NULL);
    assert(endfield_optimizer_plan_count(plan_set, &count) == ENDFIELD_OPTIMIZER_STATUS_SUCCESS);
    assert(count == UINT32_C(0));
    assert(endfield_optimizer_plan_set_destroy(plan_set) == ENDFIELD_OPTIMIZER_STATUS_SUCCESS);
}

static void
test_null_inputs(void)
{
    endfield_optimizer_request request = valid_request();
    endfield_optimizer_plan_set *plan_set = (endfield_optimizer_plan_set *)(uintptr_t)1;

    assert(endfield_optimizer_plan(NULL, &plan_set)
        == ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT);
    assert(plan_set == NULL);
    assert(endfield_optimizer_plan(&request, NULL)
        == ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT);
}

static void
test_unsupported_major(void)
{
    endfield_optimizer_request request = valid_request();
    endfield_optimizer_plan_set *plan_set = (endfield_optimizer_plan_set *)(uintptr_t)1;

    request.abi_version = UINT32_C(0x00020000);
    assert(endfield_optimizer_plan(&request, &plan_set)
        == ENDFIELD_OPTIMIZER_STATUS_UNSUPPORTED_ABI);
    assert(plan_set == NULL);
}

static void
test_undersized_request(void)
{
    endfield_optimizer_request request = valid_request();
    endfield_optimizer_plan_set *plan_set = NULL;

    request.struct_size = (uint32_t)sizeof(request) - UINT32_C(1);
    assert(endfield_optimizer_plan(&request, &plan_set)
        == ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT);
    assert(plan_set == NULL);
}

static void
test_unsupported_identifier(void)
{
    endfield_optimizer_request request = valid_request();
    endfield_optimizer_capability capability = valid_capability();
    endfield_optimizer_plan_set *plan_set = NULL;

    capability.capability_id = UINT32_C(999);
    request.capabilities = &capability;
    request.capability_count = UINT32_C(1);
    assert(endfield_optimizer_plan(&request, &plan_set)
        == ENDFIELD_OPTIMIZER_STATUS_UNSUPPORTED_IDENTIFIER);
    assert(plan_set == NULL);
}

static void
test_malformed_pointer_and_count_pairs(void)
{
    endfield_optimizer_request request = valid_request();
    endfield_optimizer_plan_set *plan_set = NULL;
    endfield_optimizer_capability capability = valid_capability();

    request.capability_count = UINT32_C(1);
    assert(endfield_optimizer_plan(&request, &plan_set)
        == ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT);
    assert(plan_set == NULL);

    request = valid_request();
    capability.provenance_len = UINT32_C(1);
    capability.provenance_utf8 = NULL;
    request.capabilities = &capability;
    request.capability_count = UINT32_C(1);
    assert(endfield_optimizer_plan(&request, &plan_set)
        == ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT);
    assert(plan_set == NULL);
}

static void
test_index_bounds_and_destruction(void)
{
    endfield_optimizer_request request = valid_request();
    endfield_optimizer_plan_set *plan_set = NULL;
    endfield_optimizer_plan_view view = {0};

    assert(endfield_optimizer_plan(&request, &plan_set) == ENDFIELD_OPTIMIZER_STATUS_SUCCESS);
    view.abi_version = ENDFIELD_OPTIMIZER_ABI_VERSION;
    view.struct_size = (uint32_t)sizeof(view);
    assert(endfield_optimizer_plan_get(plan_set, UINT32_C(0), &view)
        == ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT);
    assert(view.abi_version == ENDFIELD_OPTIMIZER_ABI_VERSION);
    assert(view.struct_size == sizeof(view));
    assert(endfield_optimizer_plan_set_destroy(plan_set) == ENDFIELD_OPTIMIZER_STATUS_SUCCESS);
    assert(endfield_optimizer_plan_set_destroy(NULL)
        == ENDFIELD_OPTIMIZER_STATUS_INVALID_ARGUMENT);
}

int
main(void)
{
    test_abi_version();
    test_valid_request_and_empty_set();
    test_null_inputs();
    test_unsupported_major();
    test_undersized_request();
    test_unsupported_identifier();
    test_malformed_pointer_and_count_pairs();
    test_index_bounds_and_destruction();
    return 0;
}
