#include "llw_runtime.h"

#include <cassert>
#include <cstddef>
#include <cstdint>

int main() {
    static_assert(sizeof(void*) == 8);
    static_assert(LLW_ABI_MAJOR == 1u);
    static_assert(LLW_ABI_MINOR == 0u);
    static_assert(offsetof(llw_abi_info_t, struct_size) == 0u);
    static_assert(offsetof(llw_event_t, data_len) % 8u == 0u);
    static_assert(sizeof(llw_handle_t) == sizeof(std::uint64_t));

    llw_abi_info_t info{};
    info.struct_size = sizeof(info);
    assert(info.struct_size >= sizeof(std::uint32_t));
    return 0;
}
