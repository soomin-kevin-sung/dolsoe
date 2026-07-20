#include "llama_api.h"

#include <algorithm>
#include <cwctype>
#include <filesystem>
#include <mutex>
#include <stdexcept>
#include <unordered_map>
#include <utility>

#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <Windows.h>
#else
#error local_llm_runtime currently supports dynamic llama.cpp loading only on Windows
#endif

namespace {

class UniqueModule {
public:
    explicit UniqueModule(HMODULE value = nullptr) : value_(value) {}
    ~UniqueModule() { if (value_) FreeLibrary(value_); }
    UniqueModule(const UniqueModule&) = delete;
    UniqueModule& operator=(const UniqueModule&) = delete;
    HMODULE get() const { return value_; }
    HMODULE release() { return std::exchange(value_, nullptr); }
private:
    HMODULE value_{};
};

std::filesystem::path canonical_runtime_directory(const std::string& directory) {
    const std::filesystem::path path = std::filesystem::u8path(directory);
    if (!std::filesystem::is_directory(path))
        throw std::runtime_error("runtime pack directory is missing");
    return std::filesystem::canonical(path);
}

UniqueModule load_module(const std::filesystem::path& directory, const wchar_t* name) {
    const std::filesystem::path path = directory / name;
    const std::string filename = path.filename().u8string();
    if (!std::filesystem::is_regular_file(path))
        throw std::runtime_error("runtime pack is missing required DLL: " + filename);
    HMODULE module = LoadLibraryExW(path.c_str(), nullptr,
        LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32);
    if (!module)
        throw std::runtime_error("failed to load runtime DLL " + filename + " (Windows error " +
                                 std::to_string(GetLastError()) + ")");
    return UniqueModule(module);
}

template <typename Function>
Function require_symbol(HMODULE module, const char* module_name, const char* symbol_name) {
    const FARPROC symbol = GetProcAddress(module, symbol_name);
    if (!symbol)
        throw std::runtime_error(std::string("runtime DLL is missing required export ") +
                                 module_name + "!" + symbol_name);
    return reinterpret_cast<Function>(symbol);
}

} // namespace

std::shared_ptr<LlamaApi> LlamaApi::load(const std::string& directory) {
    const std::filesystem::path canonical = canonical_runtime_directory(directory);
    std::wstring key = canonical.native();
    std::transform(key.begin(), key.end(), key.begin(), [](wchar_t value) {
        return static_cast<wchar_t>(std::towlower(value));
    });

    static std::mutex cache_mutex;
    static std::unordered_map<std::wstring, std::weak_ptr<LlamaApi>> cache;
    std::lock_guard lock(cache_mutex);
    if (const auto found = cache.find(key); found != cache.end()) {
        if (std::shared_ptr<LlamaApi> existing = found->second.lock()) return existing;
        cache.erase(found);
    }
    std::shared_ptr<LlamaApi> loaded(new LlamaApi(canonical.u8string()));
    cache.emplace(std::move(key), loaded);
    return loaded;
}

LlamaApi::LlamaApi(const std::string& canonical_directory) {
    const std::filesystem::path directory = std::filesystem::u8path(canonical_directory);
    UniqueModule ggml_base = load_module(directory, L"ggml-base.dll");
    UniqueModule ggml = load_module(directory, L"ggml.dll");
    UniqueModule llama = load_module(directory, L"llama.dll");

#define LLW_LOAD_LLAMA(name) name = require_symbol<decltype(name)>(llama.get(), "llama.dll", #name);
    LLW_LLAMA_DLL_SYMBOLS(LLW_LOAD_LLAMA)
#undef LLW_LOAD_LLAMA
#define LLW_LOAD_GGML(name) name = require_symbol<decltype(name)>(ggml.get(), "ggml.dll", #name);
    LLW_GGML_DLL_SYMBOLS(LLW_LOAD_GGML)
#undef LLW_LOAD_GGML
#define LLW_LOAD_GGML_BASE(name) name = require_symbol<decltype(name)>(ggml_base.get(), "ggml-base.dll", #name);
    LLW_GGML_BASE_DLL_SYMBOLS(LLW_LOAD_GGML_BASE)
#undef LLW_LOAD_GGML_BASE

    ggml_base_module_ = ggml_base.release();
    ggml_module_ = ggml.release();
    llama_module_ = llama.release();
}

LlamaApi::~LlamaApi() {
    if (llama_module_) FreeLibrary(static_cast<HMODULE>(llama_module_));
    if (ggml_module_) FreeLibrary(static_cast<HMODULE>(ggml_module_));
    if (ggml_base_module_) FreeLibrary(static_cast<HMODULE>(ggml_base_module_));
}
