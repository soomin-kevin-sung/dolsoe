#pragma once
#include "llw_runtime.h"
#include <cstdint>
#include <memory>
#include <string>
#include <vector>

struct SamplingConfig {
    uint32_t seed{}; float temperature{}; int32_t top_k{}; float top_p{}; float min_p{};
    int32_t repeat_last_n{}; float repeat_penalty{}; float frequency_penalty{}; float presence_penalty{};
};
struct ChatMessage { std::string role; std::string content; };
struct EngineRequest {
    llw_handle_t handle{}; uint32_t seq_id{}; std::vector<uint8_t> prompt;
    std::vector<ChatMessage> chat_messages; uint32_t max_new_tokens{};
    SamplingConfig sampling; std::vector<std::vector<uint8_t>> stops;
};
struct EngineStep {
    llw_handle_t handle{}; std::vector<uint8_t> token_bytes; uint32_t sampled_tokens{};
    bool finished{}; bool failed{}; std::string error; std::string finish_reason;
};
class InferenceEngine {
public:
    virtual ~InferenceEngine() = default;
    virtual uint64_t start(EngineRequest request) = 0;
    virtual std::vector<EngineStep> decode(const std::vector<llw_handle_t>& active) = 0;
    virtual void cleanup(llw_handle_t handle, uint32_t seq_id) = 0;
};
