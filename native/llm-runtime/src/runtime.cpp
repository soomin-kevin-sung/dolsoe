#include "event_dispatcher.h"
#include "llama_engine.h"
#include "llw_runtime.h"
#include "scheduler.h"
#include <algorithm>
#include <cmath>
#include <cstddef>
#include <cstdint>
#include <cstring>
#include <filesystem>
#include <functional>
#include <iterator>
#include <limits>
#include <memory>
#include <mutex>
#include <new>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <vector>
#ifdef _WIN32
#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <Windows.h>
#endif

struct llw_runtime_t {
    llw_callback_table_t callbacks{};
    llw_scheduler_config_t config{};
    std::unique_ptr<EventDispatcher> dispatcher;
    std::shared_ptr<LlamaApi> llama_api;
    std::unique_ptr<InferenceEngine> engine;
    std::unique_ptr<Scheduler> scheduler;
    llw_handle_t model_handle{};
    llw_handle_t next_model_handle{1};
    bool model_loading{};
    bool model_unloading{};
    std::string backend_directory;
    std::mutex lifecycle_mutex;
    std::mutex mutex;
#ifdef LLW_RUNTIME_TESTING
    void (LLW_CALL *flush_enqueued_hook)(void*){};
    void* flush_enqueued_user_data{};
    void (LLW_CALL *engine_destroy_hook)(void*){};
    void* engine_destroy_user_data{};
    bool fail_next_unload_before_transition{};
#endif
};

struct ModelLoadingReset {
    llw_runtime_t& runtime;
    std::unique_lock<std::mutex>& lock;
    bool active{true};
    ~ModelLoadingReset() noexcept {
        if (!active) return;
        try {
            if (!lock.owns_lock()) lock.lock();
            runtime.model_loading = false;
            lock.unlock();
            runtime.dispatcher->flush();
        } catch (...) {
            try { runtime.dispatcher->stop(); } catch (...) {}
        }
    }
    void release() { active = false; }
};

namespace {
constexpr size_t RUNTIME_CREATE_V1_0_SIZE = offsetof(llw_runtime_create_params_t, scheduler);
int module_anchor{};

#ifdef LLW_RUNTIME_TESTING
class RuntimeTestEngine final : public InferenceEngine {
public:
    uint64_t start(EngineRequest request) override { return request.prompt.size(); }
    std::vector<EngineStep> decode(const std::vector<llw_handle_t>& active) override {
        std::vector<EngineStep> result;
        result.reserve(active.size());
        for (const llw_handle_t handle : active) {
            EngineStep step;
            step.handle = handle;
            step.finished = true;
            result.push_back(std::move(step));
        }
        return result;
    }
    void cleanup(llw_handle_t, uint32_t) override {}
};
#endif

template <size_t N> bool zeroed(const uint64_t (&values)[N]) {
    return std::all_of(values, values + N, [](uint64_t value) { return value == 0; });
}

void clear_error(llw_error_t* error) {
    if (!error || error->struct_size < sizeof(uint32_t) + sizeof(int32_t)) return;
    error->code = LLW_OK;
    if (error->struct_size >= sizeof(llw_error_t)) {
        error->flags = 0;
        error->message[0] = '\0';
        std::fill(std::begin(error->reserved), std::end(error->reserved), uint64_t{0});
    }
}

llw_result_t fail(llw_error_t* error, llw_result_t code, const std::string& message) {
    if (error && error->struct_size >= sizeof(uint32_t) + sizeof(int32_t)) {
        error->code = code;
        if (error->struct_size >= sizeof(llw_error_t)) {
            error->flags = 0;
            std::strncpy(error->message, message.c_str(), sizeof(error->message) - 1);
            error->message[sizeof(error->message) - 1] = '\0';
        }
    }
    return code;
}

template <class F> llw_result_t guarded(llw_error_t* error, F&& body) noexcept {
    try { clear_error(error); return body(); }
    catch (const std::invalid_argument& exception) {
        return fail(error, LLW_ERR_INVALID_ARGUMENT, exception.what());
    } catch (const std::bad_alloc&) {
        return fail(error, LLW_ERR_INTERNAL, "allocation failed");
    } catch (const std::exception& exception) {
        return fail(error, LLW_ERR_INTERNAL, exception.what());
    } catch (...) {
        return fail(error, LLW_ERR_INTERNAL, "unknown native exception");
    }
}

bool valid_utf8(const uint8_t* data, size_t size) {
    size_t index = 0;
    while (index < size) {
        const uint8_t first = data[index++];
        if (first < 0x80) continue;
        uint32_t codepoint = 0;
        size_t continuation = 0;
        if ((first & 0xe0) == 0xc0) { codepoint = first & 0x1f; continuation = 1; }
        else if ((first & 0xf0) == 0xe0) { codepoint = first & 0x0f; continuation = 2; }
        else if ((first & 0xf8) == 0xf0) { codepoint = first & 0x07; continuation = 3; }
        else return false;
        if (index + continuation > size) return false;
        for (size_t offset = 0; offset < continuation; ++offset) {
            const uint8_t next = data[index++];
            if ((next & 0xc0) != 0x80) return false;
            codepoint = (codepoint << 6) | (next & 0x3f);
        }
        if ((continuation == 1 && codepoint < 0x80) ||
            (continuation == 2 && codepoint < 0x800) ||
            (continuation == 3 && codepoint < 0x10000) ||
            codepoint > 0x10ffff || (codepoint >= 0xd800 && codepoint <= 0xdfff)) return false;
    }
    return true;
}

std::string backend_directory() {
#ifdef _WIN32
    HMODULE module{};
    if (!GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS |
                                GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT,
                            reinterpret_cast<LPCWSTR>(&module_anchor), &module)) {
        throw std::runtime_error("GetModuleHandleExW failed");
    }
    std::vector<wchar_t> buffer(32768);
    const DWORD length = GetModuleFileNameW(module, buffer.data(), static_cast<DWORD>(buffer.size()));
    if (length == 0 || length >= buffer.size()) throw std::runtime_error("GetModuleFileNameW failed");
    return std::filesystem::path(std::wstring(buffer.data(), length)).parent_path().u8string();
#else
    return std::filesystem::current_path().u8string();
#endif
}

std::string pack_name() { return LLW_BACKEND_PACK_NAME; }

int32_t pack_backend() {
    const std::string pack = pack_name();
    if (pack == "CUDA") return LLW_BACKEND_CUDA;
    if (pack == "VULKAN") return LLW_BACKEND_VULKAN;
    return LLW_BACKEND_CPU;
}

void copy_text(char* destination, size_t capacity, const std::string& source) {
    if (capacity == 0) return;
    const size_t count = std::min(capacity - 1, source.size());
    std::memcpy(destination, source.data(), count);
    destination[count] = '\0';
}

bool try_publish_runtime_event(llw_runtime_t& runtime, int32_t type, uint32_t format,
                               llw_handle_t model, std::string payload) {
    OwnedEvent event;
    event.type = type;
    event.data_format = format;
    event.model = model;
    event.sequence = 0;
    event.data.assign(payload.begin(), payload.end());
    return runtime.dispatcher->publish(std::move(event));
}

void publish_runtime_event(llw_runtime_t& runtime, int32_t type, uint32_t format,
                           llw_handle_t model, std::string payload) {
    if (!try_publish_runtime_event(runtime, type, format, model, std::move(payload)))
        throw std::runtime_error("event dispatcher stopped");
}

llw_scheduler_config_t scheduler_config(const llw_runtime_create_params_t& params) {
    llw_scheduler_config_t config{};
    config.struct_size = sizeof(config);
    config.slot_count = 1;
    config.request_queue_capacity = 16;
    config.event_queue_capacity = 1024;
    if (params.struct_size >= sizeof(llw_runtime_create_params_t)) config = params.scheduler;
    if (config.struct_size < sizeof(config) || config.flags != 0 || config.reserved0 != 0 ||
        !zeroed(config.reserved) || config.slot_count < 1 || config.slot_count > LLW_MAX_SLOTS ||
        config.request_queue_capacity < 1 ||
        config.request_queue_capacity > LLW_MAX_QUEUE_CAPACITY ||
        config.event_queue_capacity < 16 ||
        config.event_queue_capacity > LLW_MAX_EVENT_QUEUE_CAPACITY) {
        throw std::invalid_argument("invalid scheduler configuration");
    }
    return config;
}

void validate_model(const llw_model_load_params_t& params) {
    if (params.struct_size < sizeof(params) || params.flags != 0 || params.reserved0 != 0 ||
        !zeroed(params.reserved)) throw std::invalid_argument("invalid model structure");
    if (!params.path_utf8 || params.path_len < 1 || params.path_len > LLW_MAX_MODEL_PATH_BYTES ||
        std::find(params.path_utf8, params.path_utf8 + params.path_len, uint8_t{0}) !=
            params.path_utf8 + params.path_len || !valid_utf8(params.path_utf8, params.path_len))
        throw std::invalid_argument("invalid UTF-8 model path");
    if (params.backend < LLW_BACKEND_AUTO || params.backend > LLW_BACKEND_VULKAN ||
        params.device_index > LLW_MAX_DEVICE_INDEX || params.context_tokens_per_slot < 512 ||
        params.context_tokens_per_slot > 262144 || params.logical_batch_tokens < 1 ||
        params.logical_batch_tokens > 8192 || params.physical_batch_tokens < 1 ||
        params.physical_batch_tokens > params.logical_batch_tokens || params.n_threads < 1 ||
        params.n_threads > 256 || params.n_threads_batch < 1 || params.n_threads_batch > 256 ||
        params.n_gpu_layers < -1 || params.n_gpu_layers > 65535 || params.use_mmap > 1 ||
        params.use_mlock > 1 || params.check_tensors > 1)
        throw std::invalid_argument("model option is outside its declared bounds");
}

void validate_request(const llw_request_params_t& params) {
    if (params.struct_size < sizeof(params) || params.flags != 0 || params.reserved0 != 0 ||
        params.reserved1 != 0 ||
        !zeroed(params.reserved)) throw std::invalid_argument("invalid request structure");
    if (!params.prompt || params.prompt_len < 1 || params.prompt_len > LLW_MAX_PROMPT_BYTES ||
        params.max_new_tokens < 1 || params.max_new_tokens > 1048576 ||
        !std::isfinite(params.temperature) || params.temperature < 0 || params.temperature > 10 ||
        params.top_k < 0 || params.top_k > 100000 || !std::isfinite(params.top_p) ||
        params.top_p < 0 || params.top_p > 1 || !std::isfinite(params.min_p) ||
        params.min_p < 0 || params.min_p > 1 || params.repeat_last_n < 0 ||
        params.repeat_last_n > 262144 || !std::isfinite(params.repeat_penalty) ||
        params.repeat_penalty < 0 || params.repeat_penalty > 10 ||
        !std::isfinite(params.frequency_penalty) || params.frequency_penalty < -2 ||
        params.frequency_penalty > 2 || !std::isfinite(params.presence_penalty) ||
        params.presence_penalty < -2 || params.presence_penalty > 2 ||
        params.stop_count > LLW_MAX_STOP_SEQUENCES ||
        (params.stop_count != 0 && !params.stop_sequences) ||
        params.chat_message_count > LLW_MAX_CHAT_MESSAGES ||
        (params.chat_message_count != 0 && !params.chat_messages))
        throw std::invalid_argument("request option is outside its declared bounds");
    uint64_t chat_total = 0;
    for (uint32_t index = 0; index < params.chat_message_count; ++index) {
        const llw_chat_message_t& message = params.chat_messages[index];
        const llw_bytes_t& role = message.role;
        const llw_bytes_t& content = message.content;
        if (message.struct_size < sizeof(message) || message.flags != 0 ||
            !zeroed(message.reserved) || role.struct_size < sizeof(role) || role.flags != 0 ||
            !zeroed(role.reserved) || !role.data || role.len < 1 ||
            role.len > LLW_MAX_CHAT_ROLE_BYTES || !valid_utf8(role.data, role.len) ||
            std::find(role.data, role.data + role.len, uint8_t{0}) != role.data + role.len ||
            content.struct_size < sizeof(content) || content.flags != 0 ||
            !zeroed(content.reserved) || !content.data || content.len < 1 ||
            content.len > LLW_MAX_PROMPT_BYTES || !valid_utf8(content.data, content.len) ||
            std::find(content.data, content.data + content.len, uint8_t{0}) !=
                content.data + content.len)
            throw std::invalid_argument("invalid chat message");
        const std::string_view role_name(reinterpret_cast<const char*>(role.data), role.len);
        if (role_name != "system" && role_name != "user" && role_name != "assistant")
            throw std::invalid_argument("unsupported chat message role");
        chat_total += role.len + content.len;
        if (chat_total > LLW_MAX_PROMPT_BYTES)
            throw std::invalid_argument("chat message bytes exceed total bound");
    }
    uint64_t total = 0;
    for (uint32_t index = 0; index < params.stop_count; ++index) {
        const llw_bytes_t& stop = params.stop_sequences[index];
        if (stop.struct_size < sizeof(stop) || stop.flags != 0 || !zeroed(stop.reserved) ||
            !stop.data || stop.len < 1 || stop.len > LLW_MAX_STOP_BYTES)
            throw std::invalid_argument("invalid stop sequence");
        total += stop.len;
        if (total > LLW_MAX_STOP_TOTAL_BYTES)
            throw std::invalid_argument("stop sequence bytes exceed total bound");
    }
}

std::string option_schema() {
    std::string schema = std::string(R"json({"abiMinor":2,"backendPack":")json") + pack_name() +
        R"json(","model":{"modelPath":{"type":"utf8Bytes","minBytes":1,"maxBytes":32768,"default":null,"apply":"modelReload"},"backend":{"type":"enum","values":{"auto":0,"cpu":1,"cuda":2,"vulkan":3},"default":0,"apply":"modelReload"},"deviceIndex":{"type":"uint32","min":0,"max":255,"default":0,"apply":"modelReload"},"contextTokensPerSlot":{"type":"uint32","min":512,"max":262144,"default":4096,"apply":"modelReload"},"logicalBatchTokens":{"type":"uint32","min":1,"max":8192,"default":512,"apply":"modelReload"},"physicalBatchTokens":{"type":"uint32","min":1,"maxField":"logicalBatchTokens","default":128,"apply":"modelReload"},"nThreads":{"type":"int32","min":1,"max":256,"default":8,"apply":"modelReload"},"nThreadsBatch":{"type":"int32","min":1,"max":256,"default":8,"apply":"modelReload"},"nGpuLayers":{"type":"int32","min":-1,"max":65535,"default":0,"apply":"modelReload"},"useMmap":{"type":"boolean","default":true,"apply":"modelReload"},"useMlock":{"type":"boolean","default":false,"apply":"modelReload"},"checkTensors":{"type":"boolean","default":false,"apply":"modelReload"}},"scheduler":{"slotCount":{"type":"uint32","min":1,"max":4,"default":1,"apply":"runtimeRestart"},"requestQueueCapacity":{"type":"uint32","min":1,"max":1024,"default":16,"apply":"runtimeRestart"},"eventQueueCapacity":{"type":"uint32","min":16,"max":65536,"default":1024,"apply":"runtimeRestart"}},"request":{"promptBytes":{"type":"bytes","minBytes":1,"maxBytes":16777216,"default":null,"apply":"nextRequest"},"chatMessages":{"type":"messageArray","minCount":0,"maxCount":128,"roles":["system","user","assistant"],"maxTotalBytes":16777216,"default":[],"apply":"nextRequest"},"maxNewTokens":{"type":"uint32","min":1,"max":1048576,"default":256,"apply":"nextRequest"},"seed":{"type":"uint32","min":0,"max":4294967295,"default":4294967295,"apply":"nextRequest"},"temperature":{"type":"float32","min":0.0,"max":10.0,"default":0.8,"apply":"nextRequest"},"topK":{"type":"int32","min":0,"max":100000,"default":40,"apply":"nextRequest"},"topP":{"type":"float32","min":0.0,"max":1.0,"default":0.95,"apply":"nextRequest"},"minP":{"type":"float32","min":0.0,"max":1.0,"default":0.05,"apply":"nextRequest"},"repeatLastN":{"type":"int32","min":0,"max":262144,"default":64,"apply":"nextRequest"},"repeatPenalty":{"type":"float32","min":0.0,"max":10.0,"default":1.1,"apply":"nextRequest"},"frequencyPenalty":{"type":"float32","min":-2.0,"max":2.0,"default":0.0,"apply":"nextRequest"},"presencePenalty":{"type":"float32","min":-2.0,"max":2.0,"default":0.0,"apply":"nextRequest"},"stopSequences":{"type":"bytesArray","minCount":0,"maxCount":8,"minBytesEach":1,"maxBytesEach":256,"maxTotalBytes":2048,"default":[],"apply":"nextRequest"}}})json";
    return schema;
}
} // namespace

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_abi_info(
    const llw_abi_query_t* query, llw_abi_info_t* out_info, llw_error_t* out_error) {
    return guarded(out_error, [&] {
        if (!query || !out_info || query->struct_size < sizeof(*query) || query->flags != 0 ||
            !zeroed(query->reserved) || out_info->struct_size < sizeof(*out_info) ||
            out_info->flags != 0 || !zeroed(out_info->reserved))
            throw std::invalid_argument("invalid ABI query");
        if (query->requested_major != LLW_ABI_MAJOR)
            return fail(out_error, LLW_ERR_ABI_MISMATCH, "unsupported ABI major");
        *out_info = {};
        out_info->struct_size = sizeof(*out_info);
        out_info->abi_major = LLW_ABI_MAJOR;
        out_info->abi_minor = LLW_ABI_MINOR;
        out_info->min_supported_major = LLW_ABI_MAJOR;
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_runtime_version(void) { return "0.2.0"; }
LLW_EXTERN_C LLW_EXPORT const char* LLW_CALL llw_llama_cpp_commit(void) {
    return LLW_LLAMA_CPP_COMMIT;
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_create(
    const llw_runtime_create_params_t* params, llw_runtime_t** out_runtime,
    llw_error_t* out_error) {
    if (out_runtime) *out_runtime = nullptr;
    return guarded(out_error, [&] {
        if (!params || !out_runtime || params->struct_size < RUNTIME_CREATE_V1_0_SIZE ||
            params->flags != 0 || !zeroed(params->reserved) ||
            params->callbacks.struct_size < sizeof(llw_callback_table_t) ||
            params->callbacks.flags != 0 || !zeroed(params->callbacks.reserved))
            throw std::invalid_argument("invalid runtime create parameters");
        if (params->struct_size >= sizeof(*params) && !zeroed(params->reserved_v1))
            throw std::invalid_argument("runtime reserved fields must be zero");
        auto runtime = std::make_unique<llw_runtime_t>();
        runtime->callbacks = params->callbacks;
        runtime->config = scheduler_config(*params);
        runtime->backend_directory = backend_directory();
        runtime->llama_api = LlamaApi::load(runtime->backend_directory);
        runtime->dispatcher = std::make_unique<EventDispatcher>(runtime->callbacks,
            runtime->config.event_queue_capacity);
        publish_runtime_event(*runtime, LLW_EVENT_LOG, LLW_EVENT_DATA_UTF8, 0,
                              "runtime pack initialized: " + pack_name());
        *out_runtime = runtime.release();
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT void LLW_CALL llw_runtime_destroy(llw_runtime_t* runtime) {
    if (!runtime) return;
    try {
        std::unique_ptr<Scheduler> scheduler;
        std::unique_ptr<InferenceEngine> engine;
        {
            std::lock_guard lock(runtime->mutex);
            scheduler = std::move(runtime->scheduler);
            engine = std::move(runtime->engine);
            runtime->model_handle = 0;
        }
        if (scheduler) scheduler->cancel_all_and_wait();
        scheduler.reset();
        engine.reset();
        runtime->dispatcher->stop();
    } catch (...) {}
    delete runtime;
}

#ifdef LLW_RUNTIME_TESTING
LLW_EXTERN_C LLW_EXPORT void LLW_CALL LLWTestSetFlushEnqueuedHook(
    llw_runtime_t* runtime, void (LLW_CALL *hook)(void*), void* user_data) {
    if (!runtime) return;
    std::lock_guard lock(runtime->mutex);
    runtime->flush_enqueued_hook = hook;
    runtime->flush_enqueued_user_data = user_data;
}

LLW_EXTERN_C LLW_EXPORT void LLW_CALL LLWTestFailNextPublishOfType(
    llw_runtime_t* runtime, int32_t event_type) {
    if (!runtime) return;
    runtime->dispatcher->fail_next_publish_of_type_for_test(event_type);
}

LLW_EXTERN_C LLW_EXPORT void LLW_CALL LLWTestSetEngineDestroyHook(
    llw_runtime_t* runtime, void (LLW_CALL *hook)(void*), void* user_data) {
    if (!runtime) return;
    std::lock_guard lock(runtime->mutex);
    runtime->engine_destroy_hook = hook;
    runtime->engine_destroy_user_data = user_data;
}

LLW_EXTERN_C LLW_EXPORT void LLW_CALL LLWTestFailNextUnloadBeforeTransition(
    llw_runtime_t* runtime) {
    if (!runtime) return;
    std::lock_guard lock(runtime->mutex);
    runtime->fail_next_unload_before_transition = true;
}
#endif

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_capabilities(
    llw_runtime_t* runtime, llw_capabilities_t* out, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || !out || out->struct_size < sizeof(*out) || out->flags != 0 ||
            !zeroed(out->reserved))
            throw std::invalid_argument("invalid capabilities output");
        *out = {};
        out->struct_size = sizeof(*out);
        out->supports_cpu = 1;
        out->supports_cuda = pack_backend() == LLW_BACKEND_CUDA;
        out->supports_vulkan = pack_backend() == LLW_BACKEND_VULKAN;
        out->supports_streaming = 1;
        out->supports_cancellation = 1;
        out->max_parallel_slots = LLW_MAX_SLOTS;
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_list_devices(
    llw_runtime_t* runtime, int32_t backend, llw_device_list_t* out, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || !out || out->struct_size < sizeof(*out) || out->flags != 0 ||
            out->reserved0 != 0 || !zeroed(out->reserved) ||
            backend < LLW_BACKEND_AUTO || backend > LLW_BACKEND_VULKAN)
            throw std::invalid_argument("invalid device list output");
        std::vector<DeviceRecord> devices = enumerate_pack_devices(
            *runtime->llama_api, runtime->backend_directory);
        devices.erase(std::remove_if(devices.begin(), devices.end(), [backend](const DeviceRecord& d) {
            return backend != LLW_BACKEND_AUTO && d.backend != backend;
        }), devices.end());
        out->count = 0;
        out->required_count = devices.size();
        if (devices.empty()) return LLW_OK;
        if (!out->devices || out->capacity < devices.size() ||
            out->element_size < sizeof(llw_device_info_t))
            return fail(error, LLW_ERR_BUFFER_TOO_SMALL, "device buffer is too small");
        for (size_t index = 0; index < devices.size(); ++index) {
            if (out->devices[index].struct_size < sizeof(llw_device_info_t))
                throw std::invalid_argument("device element is undersized");
            if (out->devices[index].flags != 0 || !zeroed(out->devices[index].reserved))
                throw std::invalid_argument("invalid device element");
        }
        for (size_t index = 0; index < devices.size(); ++index) {
            llw_device_info_t value{};
            value.struct_size = sizeof(value);
            value.backend = devices[index].backend;
            value.device_index = devices[index].backend_index;
            copy_text(value.id, sizeof(value.id), devices[index].id);
            copy_text(value.name, sizeof(value.name), devices[index].name);
            copy_text(value.vendor, sizeof(value.vendor), devices[index].vendor);
            out->devices[index] = value;
        }
        out->count = static_cast<uint32_t>(devices.size());
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_runtime_get_option_schema(
    llw_runtime_t* runtime, llw_buffer_t* out, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || !out || out->struct_size < sizeof(*out) || out->flags != 0 ||
            !zeroed(out->reserved)) throw std::invalid_argument("invalid schema output");
        const std::string schema = option_schema();
        out->len = schema.size();
        if (!out->data || out->capacity < schema.size())
            return fail(error, LLW_ERR_BUFFER_TOO_SMALL, "schema buffer is too small");
        std::memcpy(out->data, schema.data(), schema.size());
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_model_load(
    llw_runtime_t* runtime, const llw_model_load_params_t* params, llw_handle_t* out_model,
    llw_error_t* error) {
    if (out_model) *out_model = 0;
    return guarded(error, [&] {
        if (!runtime || !params || !out_model) throw std::invalid_argument("invalid model load call");
        validate_model(*params);
        std::unique_lock lifecycle_lock(runtime->lifecycle_mutex);
        std::unique_lock lock(runtime->mutex);
        if (runtime->model_handle != 0 || runtime->model_loading)
            return fail(error, LLW_ERR_BUSY, "a model is already loaded or loading");
        runtime->model_loading = true;
        ModelLoadingReset loading_reset{*runtime, lock};
        llw_handle_t handle = runtime->next_model_handle++;
        if (handle == 0) handle = runtime->next_model_handle++;
        const std::string path(reinterpret_cast<const char*>(params->path_utf8), params->path_len);
        const auto publish_progress = [runtime, handle](float progress) noexcept {
            try {
                return try_publish_runtime_event(*runtime, LLW_EVENT_MODEL_PROGRESS,
                    LLW_EVENT_DATA_JSON_UTF8, handle,
                    "{\"progress\":" + std::to_string(progress) + "}");
            } catch (...) {
                return false;
            }
        };
#ifdef LLW_RUNTIME_TESTING
        if (path == "llw-test-bad-alloc.gguf") throw std::bad_alloc();
        if (path == "llw-test-progress-then-bad-alloc.gguf") {
            if (!publish_progress(0.5f))
                throw std::runtime_error("model progress event queue is full");
            throw std::bad_alloc();
        }
        if (path == "llw-test-saturate-progress.gguf") {
            for (size_t index = 0;; ++index) {
                if (!publish_progress(static_cast<float>(index) / 100.0f))
                    throw std::runtime_error("model progress event queue is full");
            }
        }
#endif
        ModelConfig config;
        config.path = path;
        config.backend_directory = runtime->backend_directory;
        config.backend = params->backend;
        config.device_index = params->device_index;
        config.slots = runtime->config.slot_count;
        config.context_tokens_per_slot = params->context_tokens_per_slot;
        config.logical_batch_tokens = params->logical_batch_tokens;
        config.physical_batch_tokens = params->physical_batch_tokens;
        config.n_threads = params->n_threads;
        config.n_threads_batch = params->n_threads_batch;
        config.n_gpu_layers = params->n_gpu_layers;
        config.use_mmap = params->use_mmap != 0;
        config.use_mlock = params->use_mlock != 0;
        config.check_tensors = params->check_tensors != 0;
        lock.unlock();
        std::unique_ptr<InferenceEngine> engine;
        std::unique_ptr<Scheduler> scheduler;
#ifdef LLW_RUNTIME_TESTING
        if (path == "llw-test-model.gguf") {
            engine = std::make_unique<RuntimeTestEngine>();
        } else
#endif
        {
            engine = std::make_unique<LlamaEngine>(
                runtime->llama_api, config, publish_progress);
        }
        scheduler = std::make_unique<Scheduler>(runtime->config.slot_count,
            runtime->config.request_queue_capacity, *engine, *runtime->dispatcher);
        lock.lock();
        publish_runtime_event(*runtime, LLW_EVENT_LOG, LLW_EVENT_DATA_UTF8, handle,
                              "model loaded on backend " + std::to_string(config.backend) +
                                  " device " + std::to_string(config.device_index));
        runtime->engine = std::move(engine);
        runtime->scheduler = std::move(scheduler);
        runtime->model_handle = handle;
        runtime->model_loading = false;
        loading_reset.release();
        *out_model = handle;
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_model_unload(
    llw_runtime_t* runtime, llw_handle_t model, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || model == 0) throw std::invalid_argument("invalid model unload call");
        std::unique_lock lifecycle_lock(runtime->lifecycle_mutex);
        if (runtime->dispatcher->is_callback_thread())
            return fail(error, LLW_ERR_INVALID_STATE,
                        "model unload is not callback-reentrant");
        std::unique_ptr<Scheduler> scheduler;
        std::unique_ptr<InferenceEngine> engine;
#ifdef LLW_RUNTIME_TESTING
        void (LLW_CALL *flush_enqueued)(void*){};
        void* flush_enqueued_user_data{};
        void (LLW_CALL *before_engine_destroy)(void*){};
        void* before_engine_destroy_user_data{};
#endif
        {
            std::lock_guard lock(runtime->mutex);
            if (runtime->model_handle != model)
                return fail(error, LLW_ERR_NOT_FOUND, "model handle was not found");
#ifdef LLW_RUNTIME_TESTING
            if (runtime->fail_next_unload_before_transition) {
                runtime->fail_next_unload_before_transition = false;
                return fail(error, LLW_ERR_INTERNAL, "injected unload pre-transition failure");
            }
#endif
            runtime->model_unloading = true;
            scheduler = std::move(runtime->scheduler);
            engine = std::move(runtime->engine);
#ifdef LLW_RUNTIME_TESTING
            if (runtime->flush_enqueued_hook) {
                flush_enqueued = runtime->flush_enqueued_hook;
                flush_enqueued_user_data = runtime->flush_enqueued_user_data;
            }
            if (runtime->engine_destroy_hook) {
                before_engine_destroy = runtime->engine_destroy_hook;
                before_engine_destroy_user_data = runtime->engine_destroy_user_data;
            }
#endif
        }
        try {
            scheduler->cancel_all_and_wait();
#ifdef LLW_RUNTIME_TESTING
            if (flush_enqueued)
                runtime->dispatcher->flush_for_test(
                    flush_enqueued, flush_enqueued_user_data);
            else
                runtime->dispatcher->flush();
#else
            runtime->dispatcher->flush();
#endif
        } catch (...) {
            std::lock_guard lock(runtime->mutex);
            runtime->scheduler = std::move(scheduler);
            runtime->engine = std::move(engine);
            runtime->model_unloading = false;
            throw;
        }
#ifdef LLW_RUNTIME_TESTING
        if (before_engine_destroy) {
            try { before_engine_destroy(before_engine_destroy_user_data); } catch (...) {}
        }
#endif
        scheduler.reset();
        engine.reset();
        {
            std::lock_guard lock(runtime->mutex);
            runtime->model_handle = 0;
            runtime->model_unloading = false;
        }
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_request_submit(
    llw_runtime_t* runtime, const llw_request_params_t* params, llw_handle_t* out_request,
    llw_error_t* error) {
    if (out_request) *out_request = 0;
    return guarded(error, [&] {
        if (!runtime || !params || !out_request)
            throw std::invalid_argument("invalid request submit call");
        validate_request(*params);
        std::lock_guard lock(runtime->mutex);
        if (runtime->model_unloading || !runtime->scheduler ||
            runtime->model_handle != params->model_handle)
            return fail(error, LLW_ERR_INVALID_STATE, "requested model is not loaded");
        std::string message;
        const llw_result_t result = runtime->scheduler->submit(*params, *out_request, message);
        return result == LLW_OK ? LLW_OK : fail(error, result, message);
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_request_cancel(
    llw_runtime_t* runtime, llw_handle_t request, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || request == 0) throw std::invalid_argument("invalid request cancel call");
        std::lock_guard lock(runtime->mutex);
        if (runtime->model_unloading || !runtime->scheduler)
            return fail(error, LLW_ERR_INVALID_STATE, "no model is loaded");
        std::string message;
        const llw_result_t result = runtime->scheduler->cancel(request, message);
        return result == LLW_OK ? LLW_OK : fail(error, result, message);
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_scheduler_snapshot(
    llw_runtime_t* runtime, llw_scheduler_snapshot_t* out, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || !out || out->struct_size < sizeof(*out) || out->flags != 0 ||
            !zeroed(out->reserved))
            throw std::invalid_argument("invalid scheduler snapshot output");
        std::lock_guard lock(runtime->mutex);
        if (runtime->model_unloading || !runtime->scheduler)
            return fail(error, LLW_ERR_INVALID_STATE, "no model is loaded");
        *out = runtime->scheduler->snapshot();
        return LLW_OK;
    });
}

LLW_EXTERN_C LLW_EXPORT llw_result_t LLW_CALL llw_get_metrics(
    llw_runtime_t* runtime, llw_metrics_t* out, llw_error_t* error) {
    return guarded(error, [&] {
        if (!runtime || !out || out->struct_size < sizeof(*out) || out->flags != 0 ||
            !zeroed(out->reserved))
            throw std::invalid_argument("invalid metrics output");
        std::lock_guard lock(runtime->mutex);
        if (runtime->model_unloading || !runtime->scheduler)
            return fail(error, LLW_ERR_INVALID_STATE, "no model is loaded");
        *out = runtime->scheduler->metrics();
        return LLW_OK;
    });
}
