#include <stdint.h>

__declspec(dllimport) const char* llw_pack_local_helper_version(void);

__declspec(dllexport) int32_t llw_get_abi_info(void) {
    return 0;
}

__declspec(dllexport) const char* llw_runtime_version(void) {
    return llw_pack_local_helper_version();
}

__declspec(dllexport) const char* llw_llama_cpp_commit(void) {
    return "fixture";
}

__declspec(dllexport) int32_t llw_runtime_create(void) {
    return 0;
}

__declspec(dllexport) void llw_runtime_destroy(void) {}

__declspec(dllexport) int32_t llw_runtime_get_capabilities(void) {
    return 0;
}

__declspec(dllexport) int32_t llw_runtime_list_devices(void) {
    return 0;
}
