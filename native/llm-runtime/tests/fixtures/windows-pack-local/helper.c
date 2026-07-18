#ifdef _WIN32
#define HELPER_EXPORT __declspec(dllexport)
#else
#define HELPER_EXPORT __attribute__((visibility("default")))
#endif

HELPER_EXPORT const char* llw_pack_local_version(void) {
    return "pack-local-helper-sentinel";
}
