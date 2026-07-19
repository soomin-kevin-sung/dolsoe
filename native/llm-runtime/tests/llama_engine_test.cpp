#include "llama_engine.h"
#include <atomic>
#include <chrono>
#include <cstdio>
#include <future>
#include <string>
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
    using namespace std::chrono_literals;
    bool progress_exception_escaped = false;
    bool progress_result = true;
    try {
        progress_result = invoke_progress_callback_noexcept(
            [](float) -> bool { throw std::runtime_error("injected progress failure"); }, 0.5f);
    } catch (...) {
        progress_exception_escaped = true;
    }
    CHECK(!progress_exception_escaped);
    CHECK(!progress_result);

    std::promise<void> backend_lock_entered;
    std::promise<void> release_backend_lock;
    std::shared_future<void> release = release_backend_lock.get_future().share();
    auto holder = std::async(std::launch::async, [&] {
        run_with_backend_lock_for_test([&] {
            backend_lock_entered.set_value();
            release.wait();
        });
    });
    backend_lock_entered.get_future().wait();
    auto enumeration = std::async(std::launch::async, [] {
        return enumerate_pack_devices(".");
    });
    const bool enumeration_was_serialized =
        enumeration.wait_for(50ms) == std::future_status::timeout;
    release_backend_lock.set_value();
    holder.get();
    CHECK(enumeration.wait_for(5s) == std::future_status::ready);
    (void)enumeration.get();
    CHECK(enumeration_was_serialized);

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

    const std::vector<DeviceRecord> devices = assign_device_indices({
        {LLW_BACKEND_CUDA, 0, nullptr, "cuda:a", "CUDA A", "ggml-cuda"},
        {LLW_BACKEND_CPU, 99, nullptr, "cpu:a", "CPU A", "ggml-cpu"},
        {LLW_BACKEND_CUDA, 0, nullptr, "cuda:b", "CUDA B", "ggml-cuda"},
        {LLW_BACKEND_CPU, 99, nullptr, "cpu:b", "CPU B", "ggml-cpu"},
    });
    CHECK(devices[0].backend_index == 0);
    CHECK(devices[1].backend_index == 0);
    CHECK(devices[2].backend_index == 1);
    CHECK(devices[3].backend_index == 1);
    CHECK(select_device(devices, LLW_BACKEND_CUDA, 1, LLW_BACKEND_CUDA)->id == "cuda:b");
    CHECK(select_device(devices, LLW_BACKEND_AUTO, 0, LLW_BACKEND_CUDA)->id == "cuda:a");
    const std::vector<DeviceRecord> cpu_only = assign_device_indices({
        {LLW_BACKEND_CPU, 77, nullptr, "cpu:only", "CPU", "ggml-cpu"},
    });
    CHECK(select_device(cpu_only, LLW_BACKEND_AUTO, 0, LLW_BACKEND_CUDA)->id == "cpu:only");
    CHECK(!select_device(devices, LLW_BACKEND_VULKAN, 0, LLW_BACKEND_CUDA).has_value());

    ggml_backend_dev_props display_props{};
    display_props.name = "CPU";
    display_props.description = "AMD Ryzen Test CPU";
    CHECK(device_display_name(display_props, "unknown") == "AMD Ryzen Test CPU");
    display_props.description = "";
    CHECK(device_display_name(display_props, "unknown") == "CPU");

    const std::vector<llama_token> first_tokens = {10, 11, 12};
    const std::vector<llama_token> second_tokens = {20, 21};
    const std::vector<SequenceView> prompt_views = {
        {101, 0, &first_tokens, 0, 0, false, 0},
        {202, 1, &second_tokens, 0, 0, false, 0},
    };
    const BatchPlan prompt_plan = plan_batch(prompt_views, 5, 0);
    CHECK(prompt_plan.items.size() == 5);
    CHECK(prompt_plan.items[0].handle == 101 && prompt_plan.items[0].position == 0);
    CHECK(prompt_plan.items[1].handle == 101 && prompt_plan.items[1].position == 1);
    CHECK(prompt_plan.items[2].handle == 202 && prompt_plan.items[2].position == 0);
    CHECK(prompt_plan.items[3].handle == 202 && prompt_plan.items[3].position == 1);
    CHECK(prompt_plan.items[3].logits);
    CHECK(prompt_plan.items[4].handle == 101 && prompt_plan.items[4].position == 2);
    CHECK(prompt_plan.items[4].logits);
    const std::vector<LogitOwner> prompt_owners = collect_logit_owners(prompt_plan);
    CHECK(prompt_owners.size() == 2);
    CHECK(prompt_owners[0].handle == 202 && prompt_owners[0].batch_index == 3);
    CHECK(prompt_owners[1].handle == 101 && prompt_owners[1].batch_index == 4);

    const std::vector<SequenceView> capacity_views = {
        {101, 0, &first_tokens, 1, 8, false, 0},
        {202, 1, &second_tokens, 1, 4, false, 0},
    };
    const BatchPlan capacity_plan = plan_batch(capacity_views, 2, 0);
    CHECK(capacity_plan.items.size() == 2);
    CHECK(capacity_plan.items[0].handle == 101 && capacity_plan.items[0].token == 11);
    CHECK(capacity_plan.items[1].handle == 202 && capacity_plan.items[1].token == 21);

    const std::vector<llama_token> third_tokens = {30, 31, 32};
    const std::vector<SequenceView> small_capacity_views = {
        {101, 0, &first_tokens, 0, 0, false, 0},
        {202, 1, &second_tokens, 0, 0, false, 0},
        {303, 2, &third_tokens, 0, 0, false, 0},
    };
    const BatchPlan first_small = plan_batch(small_capacity_views, 2, 0);
    CHECK(first_small.items.size() == 2);
    CHECK(first_small.items[0].handle == 101 && first_small.items[1].handle == 202);
    const BatchPlan second_small = plan_batch(small_capacity_views, 2, first_small.next_start);
    CHECK(second_small.items.size() == 2);
    CHECK(second_small.items[0].handle == 303);
    CHECK(second_small.items[1].handle == 101);

    const std::vector<SequenceView> exhausted_views = {
        {101, 0, &first_tokens, first_tokens.size(), 3, false, 0},
        {202, 1, &second_tokens, 1, 4, false, 0},
    };
    const BatchPlan larger_than_work = plan_batch(exhausted_views, 8, 0);
    CHECK(larger_than_work.items.size() == 1);
    CHECK(larger_than_work.items[0].handle == 202 && larger_than_work.items[0].token == 21);

    const std::vector<SequenceView> generation_views = {
        {101, 0, &first_tokens, first_tokens.size(), 3, true, 31},
        {202, 1, &second_tokens, second_tokens.size(), 2, true, 41},
    };
    const BatchPlan generation_plan = plan_batch(generation_views, 2, 0);
    CHECK(generation_plan.items.size() == 2);
    CHECK(generation_plan.items[0].token == 31 && generation_plan.items[0].logits);
    CHECK(generation_plan.items[1].token == 41 && generation_plan.items[1].logits);
    CHECK(generation_plan.items[0].seq_id != generation_plan.items[1].seq_id);

    CHECK(effective_generation_budget(510, 1000, 512) == 2);
    CHECK(effective_generation_budget(512, 1, 512) == 0);
    std::vector<llama_token> accepted;
    accept_history_tokens(first_tokens, [&accepted](llama_token token) { accepted.push_back(token); });
    CHECK(accepted == first_tokens);

    const std::vector<std::vector<uint8_t>> stops = {
        {'a', 'b'}, {'a', 'b', 'c'}, {'b', 'c'}, {'a', 'b', 'c'},
    };
    const std::vector<uint8_t> overlapping = {'z', 'a', 'b', 'c', 'q'};
    const auto stop = find_stop_match(overlapping, stops);
    CHECK(stop.has_value());
    CHECK(stop->position == 1 && stop->length == 3 && stop->stop_index == 1);
    const std::vector<uint8_t> partial = {'x', 'a'};
    CHECK(safe_output_prefix(partial, stops) == 1);
    const std::vector<uint8_t> no_prefix = {'x', 'y'};
    CHECK(safe_output_prefix(no_prefix, stops) == 2);
    return 0;
}
