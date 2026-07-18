#include "llama_engine.h"
#include <cstdio>
#include <string>
#include <type_traits>
#include <vector>

#define CHECK(condition) do { if (!(condition)) { \
    std::fprintf(stderr, "%s:%d failed: %s\n", __FILE__, __LINE__, #condition); return 1; \
} } while (false)

ModelConfig valid_config() {
    ModelConfig config;
    config.path = "model.gguf";
    config.backend_directory = ".";
    config.backend = LLW_BACKEND_CPU;
    config.device_index = 0;
    config.slots = 2;
    config.context_tokens_per_slot = 4096;
    config.logical_batch_tokens = 512;
    config.physical_batch_tokens = 128;
    config.n_threads = 4;
    config.n_threads_batch = 4;
    config.n_gpu_layers = 0;
    config.use_mmap = true;
    return config;
}

int main() {
    static_assert(!std::is_copy_constructible_v<LlamaEngine>);
    static_assert(!std::is_copy_assignable_v<LlamaEngine>);

    std::string error;
    ModelConfig config = valid_config();
    CHECK(validate_model_config(config, error) == LLW_OK);
    config.backend = 99;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.slots = 0;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.slots = 5;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.context_tokens_per_slot = 511;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.physical_batch_tokens = 513;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.n_threads_batch = 257;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);
    config = valid_config(); config.device_index = LLW_MAX_DEVICE_INDEX + 1;
    CHECK(validate_model_config(config, error) == LLW_ERR_INVALID_ARGUMENT);

    const std::vector<DeviceRecord> devices = {
        {LLW_BACKEND_CPU, 0, nullptr, "cpu:0", "CPU", "ggml-cpu"},
        {LLW_BACKEND_CUDA, 0, nullptr, "cuda:0", "CUDA 0", "ggml-cuda"},
        {LLW_BACKEND_CUDA, 1, nullptr, "cuda:1", "CUDA 1", "ggml-cuda"},
    };
    const auto cuda_one = select_device(devices, LLW_BACKEND_CUDA, 1, LLW_BACKEND_CUDA);
    CHECK(cuda_one.has_value());
    CHECK(cuda_one->id == "cuda:1");
    CHECK(!select_device(devices, LLW_BACKEND_VULKAN, 0, LLW_BACKEND_CUDA).has_value());
    CHECK(!select_device(devices, LLW_BACKEND_CUDA, 2, LLW_BACKEND_CUDA).has_value());

    const auto auto_cuda = select_device(devices, LLW_BACKEND_AUTO, 1, LLW_BACKEND_CUDA);
    CHECK(auto_cuda.has_value());
    CHECK(auto_cuda->id == "cuda:1");
    const auto auto_cpu = select_device(devices, LLW_BACKEND_AUTO, 0, LLW_BACKEND_VULKAN);
    CHECK(auto_cpu.has_value());
    CHECK(auto_cpu->id == "cpu:0");

    const auto indexed = assign_device_indices({
        {LLW_BACKEND_CPU, 9, nullptr, "cpu-a", "CPU A", "ggml-cpu"},
        {LLW_BACKEND_CPU, 9, nullptr, "cpu-b", "CPU B", "ggml-cpu"},
        {LLW_BACKEND_CUDA, 9, nullptr, "cuda-a", "CUDA A", "ggml-cuda"},
        {LLW_BACKEND_CUDA, 9, nullptr, "cuda-b", "CUDA B", "ggml-cuda"},
    });
    CHECK(indexed[0].backend_index == 0);
    CHECK(indexed[1].backend_index == 1);
    CHECK(indexed[2].backend_index == 0);
    CHECK(indexed[3].backend_index == 1);

    CHECK(effective_generation_budget(100, 50, 512) == 50);
    CHECK(effective_generation_budget(500, 50, 512) == 12);
    CHECK(effective_generation_budget(512, 50, 512) == 0);
    CHECK(effective_generation_budget(600, 50, 512) == 0);

    const std::vector<std::vector<uint8_t>> stops = {
        {'a', 'b'}, {'a', 'b', 'c'}, {'b', 'c'},
    };
    const auto stop = find_stop_match({'x', 'a', 'b', 'c', 'y'}, stops);
    CHECK(stop.has_value());
    CHECK(stop->position == 1);
    CHECK(stop->stop_index == 1);
    CHECK(stop->length == 3);
    CHECK(safe_output_prefix({'x', 'a'}, stops) == 1);
    CHECK(safe_output_prefix({'x', 'z'}, stops) == 2);

    std::vector<llama_token> accepted;
    accept_history_tokens({11, 22, 33}, [&accepted](llama_token token) {
        accepted.push_back(token);
    });
    CHECK(accepted == std::vector<llama_token>({11, 22, 33}));

    const std::vector<llama_token> prompt_a = {1, 2};
    const std::vector<llama_token> prompt_b = {3};
    const BatchPlan batch = plan_batch({
        {100, 0, &prompt_a, 0, 0, false, 0},
        {200, 1, &prompt_b, 0, 0, false, 0},
    }, 3, 0);
    CHECK(batch.items.size() == 3);
    const auto owners = collect_logit_owners(batch);
    CHECK(owners.size() == 2);
    CHECK(owners[0].handle == 200);
    CHECK(owners[1].handle == 100);
    return 0;
}
