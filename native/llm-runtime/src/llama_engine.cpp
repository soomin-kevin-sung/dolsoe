#include "llama_engine.h"
#include "ggml-backend.h"
#include <algorithm>
#include <cstdint>
#include <cstring>
#include <limits>
#include <new>
#include <stdexcept>
#include <string>
#include <unordered_map>
#include <utility>

namespace {
std::mutex backend_mutex;
std::unordered_map<LlamaApi*, uint32_t> backend_users;

void acquire_backend(LlamaApi& api) {
    std::lock_guard lock(backend_mutex);
    uint32_t& users = backend_users[&api];
    if (users++ == 0) api.llama_backend_init();
}

void release_backend(LlamaApi& api) {
    std::lock_guard lock(backend_mutex);
    const auto found = backend_users.find(&api);
    if (found == backend_users.end()) return;
    if (--found->second == 0) {
        api.llama_backend_free();
        backend_users.erase(found);
    }
}

int32_t compiled_gpu_backend() {
    const std::string pack = LLW_BACKEND_PACK_NAME;
    if (pack == "CUDA") return LLW_BACKEND_CUDA;
    if (pack == "VULKAN") return LLW_BACKEND_VULKAN;
    return LLW_BACKEND_CPU;
}

std::vector<DeviceRecord> enumerate_pack_devices_unlocked(
    LlamaApi& api, const std::string& directory) {
    api.ggml_backend_load_all_from_path(directory.c_str());
    std::vector<DeviceRecord> result;
    for (size_t index = 0; index < api.ggml_backend_dev_count(); ++index) {
        ggml_backend_dev_t device = api.ggml_backend_dev_get(index);
        if (!device) continue;
        const auto type = api.ggml_backend_dev_type(device);
        int32_t backend = LLW_BACKEND_CPU;
        if (type == GGML_BACKEND_DEVICE_TYPE_GPU || type == GGML_BACKEND_DEVICE_TYPE_IGPU) {
            backend = compiled_gpu_backend();
        } else if (type != GGML_BACKEND_DEVICE_TYPE_CPU) {
            continue;
        }
        ggml_backend_dev_props properties{};
        api.ggml_backend_dev_get_props(device, &properties);
        const char* registry = api.ggml_backend_reg_name(
            api.ggml_backend_dev_backend_reg(device));
        DeviceRecord record;
        record.backend = backend;
        record.backend_index = 0;
        record.device = device;
        record.id = properties.device_id ? properties.device_id
                                         : std::to_string(backend) + ":pending";
        record.name = device_display_name(properties, api.ggml_backend_dev_name(device));
        record.vendor = registry ? registry : "ggml";
        result.push_back(std::move(record));
    }
    result = assign_device_indices(std::move(result));
    for (DeviceRecord& record : result) {
        if (record.id == std::to_string(record.backend) + ":pending")
            record.id = std::to_string(record.backend) + ":" +
                        std::to_string(record.backend_index);
    }
    return result;
}

llama_sampler* make_sampler(
    LlamaApi& api, const llama_vocab* vocab, const SamplingConfig& config,
    const std::string& output_grammar, const std::vector<llama_token>& history) {
    llama_sampler* chain = api.llama_sampler_chain_init(
        api.llama_sampler_chain_default_params());
    if (!chain) throw std::bad_alloc();
    const auto add = [&api, chain](llama_sampler* sampler) {
        if (!sampler) { api.llama_sampler_free(chain); throw std::bad_alloc(); }
        api.llama_sampler_chain_add(chain, sampler);
    };
    llama_sampler* penalties = api.llama_sampler_init_penalties(
        config.repeat_last_n, config.repeat_penalty,
        config.frequency_penalty, config.presence_penalty);
    if (!penalties) {
        api.llama_sampler_free(chain);
        throw std::bad_alloc();
    }
    accept_history_tokens(history, [&api, penalties](llama_token token) {
        api.llama_sampler_accept(penalties, token);
    });
    add(penalties);
    if (!output_grammar.empty()) {
        llama_sampler* grammar = api.llama_sampler_init_grammar(
            vocab, output_grammar.c_str(), "root");
        if (!grammar) {
            api.llama_sampler_free(chain);
            throw std::invalid_argument("invalid output grammar");
        }
        add(grammar);
    }
    if (config.top_k > 0) add(api.llama_sampler_init_top_k(config.top_k));
    if (config.top_p < 1.0f) add(api.llama_sampler_init_top_p(config.top_p, 1));
    if (config.min_p > 0.0f) add(api.llama_sampler_init_min_p(config.min_p, 1));
    add(api.llama_sampler_init_temp(config.temperature));
    add(config.temperature == 0.0f ? api.llama_sampler_init_greedy()
                                   : api.llama_sampler_init_dist(config.seed));
    return chain;
}

std::vector<uint8_t> token_piece(LlamaApi& api, const llama_vocab* vocab, llama_token token) {
    char local[256];
    int32_t count = api.llama_token_to_piece(vocab, token, local, sizeof(local), 0, true);
    if (count >= 0) return {reinterpret_cast<uint8_t*>(local),
                            reinterpret_cast<uint8_t*>(local) + count};
    if (count == std::numeric_limits<int32_t>::min())
        throw std::runtime_error("token piece length overflow");
    std::vector<char> storage(static_cast<size_t>(-count));
    count = api.llama_token_to_piece(vocab, token, storage.data(),
                                     static_cast<int32_t>(storage.size()), 0, true);
    if (count < 0) throw std::runtime_error("llama_token_to_piece failed");
    return {reinterpret_cast<uint8_t*>(storage.data()),
            reinterpret_cast<uint8_t*>(storage.data()) + count};
}

} // namespace

std::string device_display_name(const ggml_backend_dev_props& properties, const char* fallback) {
    if (properties.description && properties.description[0] != '\0') return properties.description;
    if (properties.name && properties.name[0] != '\0') return properties.name;
    return fallback ? fallback : "unknown";
}

struct LlamaEngine::Sequence {
    LlamaApi* api{};
    llw_handle_t handle{};
    uint32_t seq_id{};
    std::vector<llama_token> prompt_tokens;
    uint32_t prompt_token_count{};
    size_t prompt_cursor{};
    uint32_t next_position{};
    uint32_t generated{};
    uint32_t max_new_tokens{};
    uint32_t effective_generation_budget{};
    std::optional<llama_token> pending_token;
    std::vector<std::vector<uint8_t>> stops;
    std::vector<uint8_t> pending_output;
    llama_sampler* sampler{};
    ~Sequence() { if (sampler) api->llama_sampler_free(sampler); }
};

llw_result_t validate_model_config(const ModelConfig& config, std::string& error) {
    if (config.path.empty() || config.path.size() > LLW_MAX_MODEL_PATH_BYTES ||
        config.backend < LLW_BACKEND_AUTO || config.backend > LLW_BACKEND_VULKAN ||
        config.device_index > LLW_MAX_DEVICE_INDEX || config.slots < 1 ||
        config.slots > LLW_MAX_SLOTS || config.context_tokens_per_slot < 512 ||
        config.context_tokens_per_slot > 262144 || config.logical_batch_tokens < 1 ||
        config.logical_batch_tokens > 8192 || config.physical_batch_tokens < 1 ||
        config.physical_batch_tokens > config.logical_batch_tokens || config.n_threads < 1 ||
        config.n_threads > 256 || config.n_threads_batch < 1 ||
        config.n_threads_batch > 256 || config.n_gpu_layers < -1 ||
        config.n_gpu_layers > 65535) {
        error = "model configuration is outside declared bounds";
        return LLW_ERR_INVALID_ARGUMENT;
    }
    return LLW_OK;
}

std::vector<DeviceRecord> assign_device_indices(std::vector<DeviceRecord> devices) {
    uint32_t cpu_index = 0;
    uint32_t gpu_index = 0;
    for (DeviceRecord& device : devices) {
        if (device.backend == LLW_BACKEND_CPU) {
            device.backend_index = cpu_index++;
        } else if (device.backend == LLW_BACKEND_CUDA ||
                   device.backend == LLW_BACKEND_VULKAN) {
            device.backend_index = gpu_index++;
        }
    }
    return devices;
}

std::optional<DeviceRecord> select_device(const std::vector<DeviceRecord>& devices,
                                          int32_t backend, uint32_t index,
                                          int32_t pack_backend) {
    int32_t selected_backend = backend;
    if (backend == LLW_BACKEND_AUTO) {
        selected_backend = pack_backend;
        if (std::none_of(devices.begin(), devices.end(), [selected_backend](const DeviceRecord& d) {
                return d.backend == selected_backend;
            })) selected_backend = LLW_BACKEND_CPU;
    }
    for (const DeviceRecord& device : devices) {
        if (device.backend == selected_backend && device.backend_index == index) return device;
    }
    return std::nullopt;
}

std::vector<DeviceRecord> enumerate_pack_devices(LlamaApi& api, const std::string& directory) {
    std::lock_guard lock(backend_mutex);
    return enumerate_pack_devices_unlocked(api, directory);
}

std::vector<uint8_t> format_chat_prompt(
    LlamaApi& api, const llama_model* model, const std::vector<ChatMessage>& messages) {
    const char* chat_template = api.llama_model_chat_template(model, nullptr);
    if (!chat_template)
        throw std::runtime_error("model does not provide a default GGUF chat template");
    std::vector<llama_chat_message> native_messages;
    native_messages.reserve(messages.size());
    size_t content_bytes = 0;
    for (const ChatMessage& message : messages) {
        native_messages.push_back({message.role.c_str(), message.content.c_str()});
        content_bytes += message.role.size() + message.content.size();
    }
    const size_t initial_capacity = std::min<size_t>(
        static_cast<size_t>(std::numeric_limits<int32_t>::max()), content_bytes * 2 + 1024);
    std::vector<char> formatted(std::max<size_t>(initial_capacity, 1024));
    int32_t count = api.llama_chat_apply_template(
        chat_template, native_messages.data(), native_messages.size(), true,
        formatted.data(), static_cast<int32_t>(formatted.size()));
    if (count < 0) {
        const auto fallback = format_turn_token_chat_prompt(chat_template, messages);
        if (!fallback)
            throw std::runtime_error("model chat template is not supported by llama.cpp");
        return *fallback;
    }
    if (static_cast<size_t>(count) > formatted.size()) {
        formatted.resize(static_cast<size_t>(count) + 1);
        count = api.llama_chat_apply_template(
            chat_template, native_messages.data(), native_messages.size(), true,
            formatted.data(), static_cast<int32_t>(formatted.size()));
        if (count < 0 || static_cast<size_t>(count) > formatted.size())
            throw std::runtime_error("failed to apply model chat template");
    }
    return {reinterpret_cast<const uint8_t*>(formatted.data()),
            reinterpret_cast<const uint8_t*>(formatted.data()) + count};
}

std::optional<std::vector<uint8_t>> format_turn_token_chat_prompt(
    std::string_view chat_template, const std::vector<ChatMessage>& messages) {
    const bool supported =
        chat_template.find("<|turn>") != std::string_view::npos &&
        chat_template.find("<turn|>") != std::string_view::npos &&
        chat_template.find("add_generation_prompt") != std::string_view::npos &&
        chat_template.find("'model' if message['role'] == 'assistant'") != std::string_view::npos;
    if (!supported) return std::nullopt;

    std::string formatted;
    for (const ChatMessage& message : messages) {
        std::string_view content(message.content);
        const size_t first = content.find_first_not_of(" \t\r\n");
        const size_t last = content.find_last_not_of(" \t\r\n");
        content = first == std::string_view::npos
            ? std::string_view{}
            : content.substr(first, last - first + 1);
        std::string_view role(message.role);
        if (message.role == "assistant") role = "model";
        formatted.append("<|turn>");
        formatted.append(role.data(), role.size());
        formatted.push_back('\n');
        formatted.append(content.data(), content.size());
        formatted.append("<turn|>\n");
    }
    formatted.append("<|turn>model\n");
    return std::vector<uint8_t>(formatted.begin(), formatted.end());
}

#ifdef LLW_RUNTIME_TESTING
void run_with_backend_lock_for_test(const std::function<void()>& operation) {
    std::lock_guard lock(backend_mutex);
    operation();
}
#endif

BatchPlan plan_batch(const std::vector<SequenceView>& sequences, size_t capacity,
                     size_t start_index) {
    BatchPlan result;
    if (sequences.empty() || capacity == 0) return result;
    const size_t count = sequences.size();
    size_t start = start_index % count;
    std::vector<size_t> cursors;
    std::vector<bool> pending_consumed(count, false);
    cursors.reserve(count);
    for (const SequenceView& sequence : sequences) cursors.push_back(sequence.prompt_cursor);
    const auto eligible = [&](size_t index) {
        const SequenceView& sequence = sequences[index];
        return (sequence.prompt_tokens && cursors[index] < sequence.prompt_tokens->size()) ||
               (sequence.has_pending_token && !pending_consumed[index]);
    };
    while (result.items.size() < capacity) {
        size_t eligible_count = 0;
        for (size_t index = 0; index < count; ++index)
            if (eligible(index)) ++eligible_count;
        if (eligible_count == 0) break;
        const size_t quota = std::max<size_t>(
            1, (capacity - result.items.size()) / eligible_count);
        bool made_progress = false;
        size_t next_start = start;
        for (size_t offset = 0; offset < count && result.items.size() < capacity; ++offset) {
            const size_t index = (start + offset) % count;
            if (!eligible(index)) continue;
            const SequenceView& sequence = sequences[index];
            size_t emitted = 0;
            if (sequence.prompt_tokens && cursors[index] < sequence.prompt_tokens->size()) {
                const size_t available = sequence.prompt_tokens->size() - cursors[index];
                const size_t take = std::min({quota, available,
                    capacity - result.items.size()});
                for (size_t item = 0; item < take; ++item) {
                    const size_t token_index = cursors[index]++;
                    result.items.push_back(BatchItem{sequence.handle, sequence.seq_id,
                        (*sequence.prompt_tokens)[token_index],
                        static_cast<llama_pos>(sequence.next_position +
                            token_index - sequence.prompt_cursor),
                        token_index + 1 == sequence.prompt_tokens->size()});
                }
                emitted = take;
            } else if (sequence.has_pending_token && !pending_consumed[index]) {
                pending_consumed[index] = true;
                result.items.push_back(BatchItem{sequence.handle, sequence.seq_id,
                    sequence.pending_token, static_cast<llama_pos>(sequence.next_position), true});
                emitted = 1;
            }
            if (emitted != 0) {
                made_progress = true;
                next_start = (index + 1) % count;
            }
        }
        if (!made_progress) break;
        start = next_start;
    }
    result.next_start = start;
    return result;
}

std::vector<LogitOwner> collect_logit_owners(const BatchPlan& plan) {
    std::vector<LogitOwner> owners;
    for (size_t index = 0; index < plan.items.size(); ++index) {
        if (plan.items[index].logits) {
            owners.push_back(LogitOwner{
                plan.items[index].handle, static_cast<int32_t>(index)});
        }
    }
    return owners;
}

uint32_t effective_generation_budget(size_t prompt_tokens, uint32_t requested_tokens,
                                     uint32_t context_tokens_per_slot) {
    if (prompt_tokens >= context_tokens_per_slot) return 0;
    const uint64_t available = static_cast<uint64_t>(context_tokens_per_slot) - prompt_tokens;
    return static_cast<uint32_t>(std::min<uint64_t>(requested_tokens, available));
}

std::optional<StopMatch> find_stop_match(
    const std::vector<uint8_t>& output, const std::vector<std::vector<uint8_t>>& stops) {
    std::optional<StopMatch> best;
    for (size_t index = 0; index < stops.size(); ++index) {
        if (stops[index].empty()) continue;
        const auto found = std::search(output.begin(), output.end(),
                                       stops[index].begin(), stops[index].end());
        if (found == output.end()) continue;
        const size_t position = static_cast<size_t>(found - output.begin());
        const StopMatch candidate{position, index, stops[index].size()};
        if (!best || candidate.position < best->position ||
            (candidate.position == best->position && candidate.length > best->length)) {
            best = candidate;
        }
    }
    return best;
}

size_t safe_output_prefix(const std::vector<uint8_t>& output,
                          const std::vector<std::vector<uint8_t>>& stops) {
    size_t retained = 0;
    for (const auto& stop : stops) {
        if (stop.empty()) continue;
        const size_t limit = std::min(output.size(), stop.size() - 1);
        for (size_t length = limit; length > retained; --length) {
            if (std::equal(output.end() - static_cast<std::ptrdiff_t>(length), output.end(),
                           stop.begin(), stop.begin() + static_cast<std::ptrdiff_t>(length))) {
                retained = length;
                break;
            }
        }
    }
    return output.size() - retained;
}

void accept_history_tokens(const std::vector<llama_token>& tokens,
                           const std::function<void(llama_token)>& accept) {
    for (const llama_token token : tokens) accept(token);
}

bool invoke_progress_callback_noexcept(
    const std::function<bool(float)>& callback, float value) noexcept {
    try {
        return callback(value);
    } catch (...) {
        return false;
    }
}

LlamaEngine::LlamaEngine(std::shared_ptr<LlamaApi> api, ModelConfig config,
                         std::function<bool(float)> progress)
    : api_(std::move(api)), config_(std::move(config)) {
    if (!api_) throw std::invalid_argument("llama API is required");
    std::string error;
    if (validate_model_config(config_, error) != LLW_OK) throw std::invalid_argument(error);
    acquire_backend(*api_);
    backend_acquired_ = true;
    try {
        std::unique_lock backend_operation_lock(backend_mutex);
        const std::vector<DeviceRecord> devices =
            enumerate_pack_devices_unlocked(*api_, config_.backend_directory);
        const auto selected = select_device(
            devices, config_.backend, config_.device_index, compiled_gpu_backend());
        if (!selected) throw std::invalid_argument("selected backend device was not found");
        struct ProgressState { std::function<bool(float)>* callback; } state{&progress};
        const auto progress_bridge = [](float value, void* user_data) noexcept -> bool {
            auto& context = *static_cast<ProgressState*>(user_data);
            return invoke_progress_callback_noexcept(*context.callback, value);
        };
        ggml_backend_dev_t selected_devices[2] = {selected->device, nullptr};
        llama_model_params model_params = api_->llama_model_default_params();
        model_params.devices = selected_devices;
        model_params.n_gpu_layers = config_.n_gpu_layers;
        model_params.main_gpu = 0;
        model_params.use_mmap = config_.use_mmap;
        model_params.use_mlock = config_.use_mlock;
        model_params.check_tensors = config_.check_tensors;
        model_params.progress_callback = progress_bridge;
        model_params.progress_callback_user_data = &state;
        model_ = api_->llama_model_load_from_file(config_.path.c_str(), model_params);
        if (!model_) throw std::runtime_error("llama_model_load_from_file failed");
        backend_operation_lock.unlock();

        llama_context_params context_params = api_->llama_context_default_params();
        context_params.n_ctx = config_.context_tokens_per_slot * config_.slots;
        context_params.n_batch = config_.logical_batch_tokens;
        context_params.n_ubatch = config_.physical_batch_tokens;
        context_params.n_seq_max = config_.slots;
        context_params.n_threads = config_.n_threads;
        context_params.n_threads_batch = config_.n_threads_batch;
        context_params.embeddings = false;
        context_params.no_perf = false;
        context_ = api_->llama_init_from_model(model_, context_params);
        if (!context_) throw std::runtime_error("llama_init_from_model failed");
        vocab_ = api_->llama_model_get_vocab(model_);
        if (!vocab_) throw std::runtime_error("llama_model_get_vocab failed");
        batch_ = api_->llama_batch_init(
            static_cast<int32_t>(config_.logical_batch_tokens), 0, 1);
        if (!batch_.token || !batch_.pos || !batch_.n_seq_id || !batch_.seq_id || !batch_.logits)
            throw std::bad_alloc();
    } catch (...) {
        if (batch_.token || batch_.embd) api_->llama_batch_free(batch_);
        if (context_) api_->llama_free(context_);
        if (model_) api_->llama_model_free(model_);
        context_ = nullptr;
        model_ = nullptr;
        release_backend(*api_);
        backend_acquired_ = false;
        throw;
    }
}

LlamaEngine::~LlamaEngine() {
    std::lock_guard lock(mutex_);
    for (const auto& [handle, sequence] : sequences_) {
        (void)handle;
        api_->llama_memory_seq_rm(api_->llama_get_memory(context_),
                                  static_cast<llama_seq_id>(sequence->seq_id), -1, -1);
    }
    sequences_.clear();
    if (batch_.token || batch_.embd) api_->llama_batch_free(batch_);
    if (context_) api_->llama_free(context_);
    if (model_) api_->llama_model_free(model_);
    if (backend_acquired_) release_backend(*api_);
}

uint64_t LlamaEngine::start(EngineRequest request) {
    std::lock_guard lock(mutex_);
    if (sequences_.count(request.handle) != 0) throw std::invalid_argument("duplicate request handle");
    std::vector<uint8_t> prompt = request.chat_messages.empty()
        ? std::move(request.prompt)
        : format_chat_prompt(*api_, model_, request.chat_messages);
    if (prompt.size() > static_cast<size_t>(std::numeric_limits<int32_t>::max()))
        throw std::invalid_argument("prompt is too large for llama_tokenize");
    int32_t count = api_->llama_tokenize(
        vocab_, reinterpret_cast<const char*>(prompt.data()),
        static_cast<int32_t>(prompt.size()), nullptr, 0, true, true);
    if (count == std::numeric_limits<int32_t>::min() || count >= 0)
        throw std::runtime_error("token count query failed");
    auto sequence = std::make_unique<Sequence>();
    sequence->api = api_.get();
    sequence->prompt_tokens.resize(static_cast<size_t>(-count));
    count = api_->llama_tokenize(vocab_, reinterpret_cast<const char*>(prompt.data()),
        static_cast<int32_t>(prompt.size()), sequence->prompt_tokens.data(),
        static_cast<int32_t>(sequence->prompt_tokens.size()), true, true);
    if (count < 0) throw std::runtime_error("prompt tokenization failed");
    sequence->prompt_tokens.resize(static_cast<size_t>(count));
    const uint32_t budget = effective_generation_budget(
        sequence->prompt_tokens.size(), request.max_new_tokens,
        config_.context_tokens_per_slot);
    if (budget == 0)
        throw std::invalid_argument("prompt leaves no generation token in its slot context");
    sequence->handle = request.handle;
    sequence->seq_id = request.seq_id;
    sequence->max_new_tokens = request.max_new_tokens;
    sequence->prompt_token_count = static_cast<uint32_t>(sequence->prompt_tokens.size());
    sequence->effective_generation_budget = budget;
    sequence->stops = std::move(request.stops);
    sequence->sampler = make_sampler(
        *api_, vocab_, request.sampling, request.output_grammar,
        sequence->prompt_tokens);
    const uint64_t prompt_tokens = sequence->prompt_tokens.size();
    sequences_.emplace(request.handle, std::move(sequence));
    return prompt_tokens;
}

std::vector<uint8_t> LlamaEngine::format_chat(const std::vector<ChatMessage>& messages) {
    std::lock_guard lock(mutex_);
    return format_chat_prompt(*api_, model_, messages);
}

std::vector<EngineStep> LlamaEngine::decode(const std::vector<llw_handle_t>& active) {
    std::lock_guard lock(mutex_);
    std::vector<SequenceView> views;
    for (const llw_handle_t handle : active) {
        const auto found = sequences_.find(handle);
        if (found == sequences_.end()) continue;
        const Sequence& sequence = *found->second;
        views.push_back(SequenceView{handle, sequence.seq_id, &sequence.prompt_tokens,
            sequence.prompt_cursor, sequence.next_position, sequence.pending_token.has_value(),
            sequence.pending_token.value_or(0)});
    }
    const BatchPlan plan = plan_batch(views, config_.logical_batch_tokens, batch_start_);
    batch_start_ = plan.next_start;
    if (plan.items.empty()) return {};
    batch_.n_tokens = static_cast<int32_t>(plan.items.size());
    const std::vector<LogitOwner> logit_owners = collect_logit_owners(plan);
    for (size_t index = 0; index < plan.items.size(); ++index) {
        const BatchItem& item = plan.items[index];
        batch_.token[index] = item.token;
        batch_.pos[index] = item.position;
        batch_.n_seq_id[index] = 1;
        batch_.seq_id[index][0] = static_cast<llama_seq_id>(item.seq_id);
        batch_.logits[index] = item.logits ? 1 : 0;
        Sequence& sequence = *sequences_.at(item.handle);
        if (sequence.prompt_cursor < sequence.prompt_tokens.size()) ++sequence.prompt_cursor;
        else sequence.pending_token.reset();
        ++sequence.next_position;
    }
    const int32_t decode_result = api_->llama_decode(context_, batch_);
    if (decode_result != 0) {
        std::vector<EngineStep> failed;
        for (const llw_handle_t handle : active)
            failed.push_back(EngineStep{handle, {}, 0, false, true,
                "llama_decode returned " + std::to_string(decode_result)});
        return failed;
    }

    std::vector<EngineStep> result;
    for (const LogitOwner& owner : logit_owners) {
        Sequence& sequence = *sequences_.at(owner.handle);
        // At the pinned commit llama_sampler_sample samples and accepts exactly once.
        const llama_token token = api_->llama_sampler_sample(
            sequence.sampler, context_, owner.batch_index);
        ++sequence.generated;
        EngineStep step;
        step.handle = owner.handle;
        step.sampled_tokens = 1;
        bool done = api_->llama_vocab_is_eog(vocab_, token) ||
                    sequence.generated >= sequence.effective_generation_budget;
        step.finish_reason = api_->llama_vocab_is_eog(vocab_, token) ? "stop" :
            (sequence.generated >= sequence.effective_generation_budget ? "length" : "");
        if (!api_->llama_vocab_is_eog(vocab_, token)) {
            const std::vector<uint8_t> piece = token_piece(*api_, vocab_, token);
            sequence.pending_output.insert(sequence.pending_output.end(), piece.begin(), piece.end());
            if (const auto match = find_stop_match(sequence.pending_output, sequence.stops)) {
                step.token_bytes.assign(sequence.pending_output.begin(),
                    sequence.pending_output.begin() + static_cast<std::ptrdiff_t>(match->position));
                sequence.pending_output.clear();
                step.finish_reason = "stop";
                done = true;
            }
            if (!done) {
                const size_t emit = safe_output_prefix(sequence.pending_output, sequence.stops);
                if (emit != 0) {
                    step.token_bytes.assign(sequence.pending_output.begin(),
                                            sequence.pending_output.begin() + emit);
                    sequence.pending_output.erase(sequence.pending_output.begin(),
                                                  sequence.pending_output.begin() + emit);
                }
            }
        }
        if (done) {
            step.token_bytes.insert(step.token_bytes.end(), sequence.pending_output.begin(),
                                    sequence.pending_output.end());
            sequence.pending_output.clear();
            step.finished = true;
        } else {
            sequence.pending_token = token;
        }
        result.push_back(std::move(step));
    }
    return result;
}

void LlamaEngine::cleanup(llw_handle_t handle, uint32_t seq_id) {
    std::lock_guard lock(mutex_);
    const auto found = sequences_.find(handle);
    if (found == sequences_.end()) return;
    if (found->second->seq_id != seq_id) throw std::invalid_argument("sequence ID mismatch");
    const bool removed = api_->llama_memory_seq_rm(
        api_->llama_get_memory(context_), static_cast<llama_seq_id>(seq_id), -1, -1);
    sequences_.erase(found);
    if (!removed)
        throw std::runtime_error("failed to clear sequence memory");
}
