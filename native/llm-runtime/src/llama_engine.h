#pragma once
#include "inference_engine.h"
#include "llama.h"
#include <cstddef>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <unordered_map>
#include <vector>

struct DeviceRecord {
    int32_t backend{};
    uint32_t backend_index{};
    ggml_backend_dev_t device{};
    std::string id;
    std::string name;
    std::string vendor;
};

struct ModelConfig {
    std::string path;
    std::string backend_directory;
    int32_t backend{};
    uint32_t device_index{};
    uint32_t slots{};
    uint32_t context_tokens_per_slot{};
    uint32_t logical_batch_tokens{};
    uint32_t physical_batch_tokens{};
    int32_t n_threads{};
    int32_t n_threads_batch{};
    int32_t n_gpu_layers{};
    bool use_mmap{};
    bool use_mlock{};
    bool check_tensors{};
};

struct SequenceView {
    llw_handle_t handle{};
    uint32_t seq_id{};
    const std::vector<llama_token>* prompt_tokens{};
    size_t prompt_cursor{};
    uint32_t next_position{};
    bool has_pending_token{};
    llama_token pending_token{};
};
struct BatchItem {
    llw_handle_t handle{};
    uint32_t seq_id{};
    llama_token token{};
    llama_pos position{};
    bool logits{};
};
struct BatchPlan {
    std::vector<BatchItem> items;
    size_t next_start{};
};
struct LogitOwner {
    llw_handle_t handle{};
    int32_t batch_index{};
};
struct StopMatch {
    size_t position{};
    size_t stop_index{};
    size_t length{};
};

llw_result_t validate_model_config(const ModelConfig&, std::string&);
std::vector<DeviceRecord> assign_device_indices(std::vector<DeviceRecord>);
std::string device_display_name(const ggml_backend_dev_props&, const char* fallback);
std::optional<DeviceRecord> select_device(
    const std::vector<DeviceRecord>&, int32_t, uint32_t, int32_t);
std::vector<DeviceRecord> enumerate_pack_devices(const std::string& backend_directory);
BatchPlan plan_batch(
    const std::vector<SequenceView>& sequences, size_t capacity, size_t start_index);
std::vector<LogitOwner> collect_logit_owners(const BatchPlan& plan);
uint32_t effective_generation_budget(
    size_t prompt_tokens, uint32_t requested_tokens, uint32_t context_tokens_per_slot);
std::optional<StopMatch> find_stop_match(
    const std::vector<uint8_t>& output, const std::vector<std::vector<uint8_t>>& stops);
size_t safe_output_prefix(
    const std::vector<uint8_t>& output, const std::vector<std::vector<uint8_t>>& stops);
void accept_history_tokens(const std::vector<llama_token>& tokens,
                           const std::function<void(llama_token)>& accept);
bool invoke_progress_callback_noexcept(
    const std::function<bool(float)>& callback, float value) noexcept;
#ifdef LLW_RUNTIME_TESTING
void run_with_backend_lock_for_test(const std::function<void()>& operation);
#endif

class LlamaEngine final : public InferenceEngine {
public:
    LlamaEngine(ModelConfig config, std::function<bool(float)> progress);
    ~LlamaEngine() override;
    uint64_t start(EngineRequest request) override;
    std::vector<EngineStep> decode(const std::vector<llw_handle_t>& active) override;
    void cleanup(llw_handle_t handle, uint32_t seq_id) override;
private:
    struct Sequence;
    ModelConfig config_;
    llama_model* model_{};
    llama_context* context_{};
    const llama_vocab* vocab_{};
    llama_batch batch_{};
    std::unordered_map<llw_handle_t, std::unique_ptr<Sequence>> sequences_;
    size_t batch_start_{};
    std::mutex mutex_;
    bool backend_acquired_{};
};
