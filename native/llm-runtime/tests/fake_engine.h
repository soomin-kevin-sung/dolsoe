#pragma once
#include "inference_engine.h"
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <map>
#include <mutex>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

class FakeEngine final : public InferenceEngine {
public:
    uint64_t start(EngineRequest request) override {
        std::lock_guard lock(mutex_);
        if (request.prompt == rejected_prompt_)
            throw std::invalid_argument("prompt leaves no generation token in its slot context");
        const uint64_t prompt_tokens = request.prompt.size();
        operation_log_.push_back("start:" + std::to_string(request.handle));
        requests_.emplace(request.handle, Stored{std::move(request), 0});
        changed_.notify_all();
        return prompt_tokens;
    }

    std::vector<EngineStep> decode(const std::vector<llw_handle_t>& active) override {
        std::unique_lock lock(mutex_);
        batches_.push_back(active);
        changed_.notify_all();
        if (!gate_.wait_for(lock, std::chrono::seconds(5), [this] { return released_; }))
            throw std::runtime_error("fake engine release timeout");
        if (decode_failure_) throw std::runtime_error("injected decode failure");
        std::vector<EngineStep> result;
        for (const llw_handle_t handle : active) {
            auto found = requests_.find(handle);
            if (found == requests_.end()) continue;
            Stored& stored = found->second;
            ++stored.steps;
            EngineStep step;
            step.handle = handle;
            step.sampled_tokens = 1;
            if (!empty_token_bytes_)
                step.token_bytes = {static_cast<uint8_t>('A' + stored.request.seq_id)};
            step.finished = stored.steps == 3;
            result.push_back(std::move(step));
        }
        return result;
    }

    void cleanup(llw_handle_t handle, uint32_t seq_id) override {
        std::lock_guard lock(mutex_);
        const auto found = requests_.find(handle);
        if (found == requests_.end() || found->second.request.seq_id != seq_id)
            throw std::runtime_error("cleanup called for unknown sequence");
        cleanup_calls_[handle] += 1;
        operation_log_.push_back("cleanup:" + std::to_string(handle));
        requests_.erase(handle);
        changed_.notify_all();
    }

    void set_decode_failure(bool value) {
        std::lock_guard lock(mutex_);
        decode_failure_ = value;
    }

    void set_empty_token_bytes(bool value) {
        std::lock_guard lock(mutex_);
        empty_token_bytes_ = value;
    }

    void reject_prompt(std::vector<uint8_t> prompt) {
        std::lock_guard lock(mutex_);
        rejected_prompt_ = std::move(prompt);
    }

    void release() {
        std::lock_guard lock(mutex_);
        released_ = true;
        gate_.notify_all();
    }

    void wait_for_started(size_t count) {
        std::unique_lock lock(mutex_);
        if (!changed_.wait_for(lock, std::chrono::seconds(5),
                               [this, count] { return requests_.size() >= count; }))
            throw std::runtime_error("fake engine start timeout");
    }

    void wait_for_batch_size(size_t count) {
        std::unique_lock lock(mutex_);
        if (!changed_.wait_for(lock, std::chrono::seconds(5), [this, count] {
            for (const auto& batch : batches_) if (batch.size() >= count) return true;
            return false;
        })) throw std::runtime_error("fake engine batch timeout");
    }

    void wait_for_cleanup(llw_handle_t handle) {
        std::unique_lock lock(mutex_);
        if (!changed_.wait_for(lock, std::chrono::seconds(5), [this, handle] {
                return cleanup_calls_.count(handle) != 0;
            }))
            throw std::runtime_error("fake engine cleanup timeout");
    }

    std::vector<std::vector<llw_handle_t>> batches() const {
        std::lock_guard lock(mutex_);
        return batches_;
    }

    uint32_t cleanup_count(llw_handle_t handle) const {
        std::lock_guard lock(mutex_);
        const auto found = cleanup_calls_.find(handle);
        return found == cleanup_calls_.end() ? 0 : found->second;
    }

    std::vector<std::string> operation_log() const {
        std::lock_guard lock(mutex_);
        return operation_log_;
    }

private:
    struct Stored { EngineRequest request; uint32_t steps{}; };
    mutable std::mutex mutex_;
    std::condition_variable gate_;
    std::condition_variable changed_;
    std::map<llw_handle_t, Stored> requests_;
    std::vector<std::vector<llw_handle_t>> batches_;
    std::map<llw_handle_t, uint32_t> cleanup_calls_;
    std::vector<std::string> operation_log_;
    bool released_{};
    bool decode_failure_{};
    bool empty_token_bytes_{};
    std::vector<uint8_t> rejected_prompt_;
};
