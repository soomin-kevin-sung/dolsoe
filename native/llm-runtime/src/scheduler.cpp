#include "scheduler.h"
#include <algorithm>
#include <chrono>
#include <cstdio>
#include <exception>
#include <sstream>
#include <stdexcept>
#include <tuple>
#include <utility>

namespace {
std::vector<uint8_t> bytes(std::string value) {
    return {value.begin(), value.end()};
}

std::string json_escape(const std::string& value) {
    std::ostringstream out;
    for (const unsigned char ch : value) {
        switch (ch) {
        case '"': out << "\\\""; break;
        case '\\': out << "\\\\"; break;
        case '\b': out << "\\b"; break;
        case '\f': out << "\\f"; break;
        case '\n': out << "\\n"; break;
        case '\r': out << "\\r"; break;
        case '\t': out << "\\t"; break;
        default:
            if (ch < 0x20) {
                char escaped[7]{};
                std::snprintf(escaped, sizeof(escaped), "\\u%04x", ch);
                out << escaped;
            } else {
                out << static_cast<char>(ch);
            }
        }
    }
    return out.str();
}

bool terminal(RequestState state) {
    return state == RequestState::Done || state == RequestState::Cancelled ||
           state == RequestState::Error;
}
} // namespace

Scheduler::Scheduler(uint32_t slots, uint32_t queue_capacity, InferenceEngine& engine,
                     EventDispatcher& events, NowFn now)
    : slots_count_(slots), queue_capacity_(queue_capacity), engine_(engine), events_(events),
      now_(std::move(now)) {
    if (slots < 1 || slots > LLW_MAX_SLOTS || queue_capacity < 1 ||
        queue_capacity > LLW_MAX_QUEUE_CAPACITY) {
        throw std::invalid_argument("invalid scheduler bounds");
    }
    metrics_ = {};
    metrics_.struct_size = sizeof(metrics_);
    for (uint32_t index = 0; index < slots; ++index) slots_.push_back(Slot{index, 0});
    worker_ = std::thread([this] { run(); });
}

Scheduler::~Scheduler() {
    cancel_all_and_wait();
    {
        std::lock_guard lock(mutex_);
        stopping_ = true;
    }
    wake_.notify_all();
    if (worker_.joinable()) worker_.join();
}

llw_result_t Scheduler::submit(const llw_request_params_t& params, llw_handle_t& out,
                               std::string& error) {
    out = 0;
    Request request;
    request.model = params.model_handle;
    request.prompt.assign(params.prompt, params.prompt + params.prompt_len);
    request.max_new_tokens = params.max_new_tokens;
    request.sampling = SamplingConfig{params.seed, params.temperature, params.top_k, params.top_p,
        params.min_p, params.repeat_last_n, params.repeat_penalty, params.frequency_penalty,
        params.presence_penalty};
    request.user_data = params.request_user_data;
    request.enqueued_at = now_();
    request.stops.reserve(params.stop_count);
    for (uint32_t index = 0; index < params.stop_count; ++index) {
        const llw_bytes_t& stop = params.stop_sequences[index];
        request.stops.emplace_back(stop.data, stop.data + stop.len);
    }

    std::lock_guard lock(mutex_);
    if (queued_.size() >= queue_capacity_) {
        error = "request queue is full";
        return LLW_ERR_QUEUE_FULL;
    }
    request.handle = next_handle_++;
    if (request.handle == 0) request.handle = next_handle_++;
    const llw_handle_t handle = request.handle;
    auto payload = bytes("{\"state\":\"queued\",\"queuePosition\":" +
                         std::to_string(queued_.size() + 1) + "}");
    if (!events_.reserve_terminal(handle)) {
        error = "request terminal capacity is full";
        return LLW_ERR_QUEUE_FULL;
    }
    decltype(requests_)::iterator it;
    try {
#ifdef LLW_RUNTIME_TESTING
        if (submit_failure_ == SubmitFailurePoint::RequestInsert) {
            submit_failure_.reset();
            throw std::runtime_error("injected request insertion failure");
        }
#endif
        bool inserted = false;
        std::tie(it, inserted) = requests_.emplace(handle, std::move(request));
        if (!inserted) throw std::runtime_error("request handle collision");
        try {
#ifdef LLW_RUNTIME_TESTING
            if (submit_failure_ == SubmitFailurePoint::QueueInsert) {
                submit_failure_.reset();
                throw std::runtime_error("injected queue insertion failure");
            }
#endif
            queued_.push_back(handle);
        } catch (...) {
            requests_.erase(it);
            throw;
        }
    } catch (...) {
        events_.release_terminal(handle);
        throw;
    }
    try {
        if (!publish_locked(it->second, LLW_EVENT_QUEUED, UINT32_MAX, 0, std::move(payload),
                            LLW_EVENT_DATA_JSON_UTF8)) {
            queued_.erase(std::remove(queued_.begin(), queued_.end(), handle), queued_.end());
            requests_.erase(it);
            events_.release_terminal(handle);
            error = "request event queue is full";
            return LLW_ERR_QUEUE_FULL;
        }
    } catch (...) {
        queued_.erase(std::remove(queued_.begin(), queued_.end(), handle), queued_.end());
        requests_.erase(it);
        events_.release_terminal(handle);
        throw;
    }
    ++accepted_;
    out = handle;
    wake_.notify_one();
    return LLW_OK;
}

llw_result_t Scheduler::cancel(llw_handle_t handle, std::string& error) {
    std::lock_guard lock(mutex_);
    const auto found = requests_.find(handle);
    if (found == requests_.end()) {
        error = "request handle was not found";
        return LLW_ERR_NOT_FOUND;
    }
    Request& request = found->second;
    if (terminal(request.state) || request.cancel_requested) return LLW_OK;
    request.cancel_requested = true;
    if (request.state == RequestState::Queued) {
        queued_.erase(std::remove(queued_.begin(), queued_.end(), handle), queued_.end());
        finish_locked(handle, RequestState::Cancelled, 0, "");
    }
    wake_.notify_one();
    return LLW_OK;
}

llw_scheduler_snapshot_t Scheduler::snapshot() const {
    std::lock_guard lock(mutex_);
    llw_scheduler_snapshot_t result{};
    result.struct_size = sizeof(result);
    result.slot_count = slots_count_;
    result.queue_capacity = queue_capacity_;
    result.queued_count = static_cast<uint32_t>(queued_.size());
    result.active_count = static_cast<uint32_t>(std::count_if(
        slots_.begin(), slots_.end(), [](const Slot& slot) { return slot.request != 0; }));
    result.accepted_requests = accepted_;
    result.terminal_requests = terminal_;
    return result;
}

llw_metrics_t Scheduler::metrics() const {
    std::lock_guard lock(mutex_);
    return metrics_;
}

size_t Scheduler::tracked_request_count_for_test() const {
    std::lock_guard lock(mutex_);
    return requests_.size();
}

#ifdef LLW_RUNTIME_TESTING
void Scheduler::fail_next_submit_for_test(SubmitFailurePoint point) {
    std::lock_guard lock(mutex_);
    submit_failure_ = point;
}

void Scheduler::set_worker_paused_for_test(bool paused) {
    {
        std::lock_guard lock(mutex_);
        worker_paused_ = paused;
    }
    wake_.notify_all();
}
#endif

void Scheduler::cancel_all_and_wait() {
    std::unique_lock lock(mutex_);
#ifdef LLW_RUNTIME_TESTING
    worker_paused_ = false;
#endif
    for (auto& [handle, request] : requests_) {
        (void)handle;
        if (!terminal(request.state)) request.cancel_requested = true;
    }
    while (!queued_.empty()) {
        const llw_handle_t handle = queued_.front();
        queued_.pop_front();
        finish_locked(handle, RequestState::Cancelled, 0, "");
    }
    wake_.notify_one();
    idle_.wait(lock, [this] { return terminal_ == accepted_; });
}

bool Scheduler::has_work_locked() const {
    if (!queued_.empty()) return true;
    if (std::any_of(requests_.begin(), requests_.end(), [](const auto& entry) {
            return entry.second.terminal_pending;
        })) return true;
    return std::any_of(slots_.begin(), slots_.end(), [this](const Slot& slot) {
        if (slot.request == 0) return false;
        const auto found = requests_.find(slot.request);
        return found != requests_.end() && !terminal(found->second.state);
    });
}

void Scheduler::promote_locked() {
    for (Slot& slot : slots_) {
        if (slot.request != 0 || queued_.empty()) continue;
        const llw_handle_t handle = queued_.front();
        queued_.pop_front();
        Request& request = requests_.at(handle);
        if (request.cancel_requested) {
            finish_locked(handle, RequestState::Cancelled, 0, "");
            continue;
        }
        request.slot_id = slot.id;
        request.state = RequestState::Preprocessing;
        slot.request = handle;
        try {
            request.started_at = now_();
            metrics_.queue_wait_ns += static_cast<uint64_t>(
                std::chrono::duration_cast<std::chrono::nanoseconds>(
                    request.started_at - request.enqueued_at).count());
            request.prompt_tokens = engine_.start(EngineRequest{request.handle, slot.id,
                request.prompt, request.max_new_tokens, request.sampling, request.stops});
            request.engine_started = true;
            metrics_.prompt_tokens += request.prompt_tokens;
            request.state = RequestState::Running;
        } catch (const std::exception& exception) {
            finish_locked(handle, RequestState::Error, LLW_ERR_INTERNAL, exception.what());
        } catch (...) {
            finish_locked(handle, RequestState::Error, LLW_ERR_INTERNAL,
                          "unknown preprocessing failure");
        }
    }
}

bool Scheduler::publish_locked(Request& request, int32_t type, uint32_t slot,
                               int32_t error_code, std::vector<uint8_t> payload,
                               uint32_t data_format) {
    OwnedEvent event;
    event.type = type;
    event.data_format = data_format;
    event.error_code = error_code;
    event.model = request.model;
    event.request = request.handle;
    event.slot = slot;
    event.sequence = request.next_sequence;
    event.request_user_data = request.user_data;
    event.data = std::move(payload);
    if (!events_.publish(std::move(event))) return false;
    ++request.next_sequence;
    return true;
}

void Scheduler::finish_locked(llw_handle_t handle, RequestState state, int32_t error_code,
                               std::string message) {
    Request& request = requests_.at(handle);
    if (request.terminal_emitted || request.terminal_pending) return;
    request.terminal_pending = true;
    request.pending_terminal_state = state;
    request.pending_terminal_error = error_code;
    try {
        request.pending_terminal_message = std::move(message);
    } catch (...) {
        request.pending_terminal_state = RequestState::Error;
        request.pending_terminal_error = LLW_ERR_INTERNAL;
        request.pending_terminal_message.clear();
    }
    if (request.engine_started && !request.cleanup_attempted) {
        request.cleanup_attempted = true;
        try {
            engine_.cleanup(request.handle, request.slot_id);
        } catch (const std::exception& exception) {
            request.pending_terminal_state = RequestState::Error;
            request.pending_terminal_error = LLW_ERR_INTERNAL;
            try {
                request.pending_terminal_message =
                    std::string("sequence cleanup failed: ") + exception.what();
            } catch (...) {
                request.pending_terminal_message.clear();
            }
        } catch (...) {
            request.pending_terminal_state = RequestState::Error;
            request.pending_terminal_error = LLW_ERR_INTERNAL;
            try { request.pending_terminal_message = "sequence cleanup failed"; }
            catch (...) { request.pending_terminal_message.clear(); }
        }
    }
    (void)try_publish_terminal_locked(handle);
}

bool Scheduler::try_publish_terminal_locked(llw_handle_t handle) noexcept {
    try {
        const auto found = requests_.find(handle);
        if (found == requests_.end()) return true;
        Request& request = found->second;
        if (!request.terminal_pending || request.terminal_emitted) return true;
        const RequestState state = request.pending_terminal_state;
        const int32_t error_code = request.pending_terminal_error;
        const std::string& message = request.pending_terminal_message;
        int32_t event_type = LLW_EVENT_DONE;
        const std::string done_reason = message.empty() ? "stop" : message;
        std::string payload = "{\"state\":\"done\",\"reason\":\"" +
                              json_escape(done_reason) + "\",\"generatedTokens\":" +
                              std::to_string(request.generated_tokens) + "}";
        if (state == RequestState::Cancelled) {
            event_type = LLW_EVENT_CANCELLED;
            payload = "{\"state\":\"cancelled\"}";
        } else if (state == RequestState::Error) {
            event_type = LLW_EVENT_ERROR;
            payload = "{\"state\":\"error\",\"message\":\"" + json_escape(message) + "\"}";
        }
        if (!publish_locked(request, event_type, request.slot_id, error_code, bytes(payload),
                            LLW_EVENT_DATA_JSON_UTF8)) return false;
        request.terminal_emitted = true;
        request.terminal_pending = false;
        request.state = state;
        if (state == RequestState::Cancelled) ++metrics_.cancelled_requests;
        else if (state == RequestState::Error) ++metrics_.failed_requests;
        for (Slot& slot : slots_) {
            if (slot.request == handle) slot.request = 0;
        }
        ++terminal_;
        requests_.erase(handle);
        if (terminal_ == accepted_) idle_.notify_all();
        return true;
    } catch (...) {
        return false;
    }
}

void Scheduler::run() {
    std::unique_lock lock(mutex_);
    for (;;) {
        try {
            wake_.wait(lock, [this] {
#ifdef LLW_RUNTIME_TESTING
                return stopping_ || (!worker_paused_ && has_work_locked());
#else
                return stopping_ || has_work_locked();
#endif
            });
#ifdef LLW_RUNTIME_TESTING
            if (worker_paused_ && !stopping_) continue;
#endif
            if (stopping_ && !has_work_locked()) break;

            std::vector<llw_handle_t> pending;
            for (const auto& [handle, request] : requests_)
                if (request.terminal_pending) pending.push_back(handle);
            bool retry_failed = false;
            for (const llw_handle_t handle : pending)
                if (!try_publish_terminal_locked(handle)) retry_failed = true;
            if (retry_failed) {
                lock.unlock();
                std::this_thread::yield();
                lock.lock();
                continue;
            }

            for (Slot& slot : slots_) {
                if (slot.request == 0) continue;
                const llw_handle_t handle = slot.request;
                Request& request = requests_.at(handle);
                if (!request.cancel_requested || terminal(request.state)) continue;
                finish_locked(handle, RequestState::Cancelled, 0, "");
            }
            promote_locked();

            std::vector<llw_handle_t> active;
            for (const Slot& slot : slots_) {
                if (slot.request == 0) continue;
                const auto found = requests_.find(slot.request);
                if (found != requests_.end() && !found->second.terminal_pending)
                    active.push_back(slot.request);
            }
            if (active.empty()) continue;

            lock.unlock();
            std::vector<EngineStep> steps;
            std::string decode_error;
            const auto started = std::chrono::steady_clock::now();
            try { steps = engine_.decode(active); }
            catch (const std::exception& exception) { decode_error = exception.what(); }
            catch (...) { decode_error = "unknown decode failure"; }
            const auto elapsed = std::chrono::duration_cast<std::chrono::nanoseconds>(
                std::chrono::steady_clock::now() - started).count();
            lock.lock();

            ++metrics_.decode_calls;
            metrics_.decode_ns += static_cast<uint64_t>(elapsed);
            if (!decode_error.empty()) {
                for (const llw_handle_t handle : active) {
                    const auto found = requests_.find(handle);
                    if (found != requests_.end())
                        finish_locked(handle, RequestState::Error, LLW_ERR_INTERNAL, decode_error);
                }
                continue;
            }
            for (EngineStep& step : steps) {
                auto found = requests_.find(step.handle);
                if (found == requests_.end() || found->second.terminal_emitted ||
                    found->second.terminal_pending) continue;
                Request& request = found->second;
                request.generated_tokens += step.sampled_tokens;
                metrics_.generated_tokens += step.sampled_tokens;
                if (request.cancel_requested) {
                    finish_locked(request.handle, RequestState::Cancelled, 0, "");
                    continue;
                }
                if (!step.token_bytes.empty()) {
                    if (!publish_locked(request, LLW_EVENT_TOKEN, request.slot_id, 0,
                                        std::move(step.token_bytes), LLW_EVENT_DATA_BYTES)) {
                        finish_locked(request.handle, RequestState::Error, LLW_ERR_INTERNAL,
                                      "event queue capacity exceeded");
                        continue;
                    }
                }
                if (step.failed) finish_locked(request.handle, RequestState::Error,
                                               LLW_ERR_INTERNAL, step.error);
                else if (step.finished) finish_locked(request.handle, RequestState::Done, 0,
                                                      step.finish_reason);
            }

            OwnedEvent metrics_event;
            metrics_event.type = LLW_EVENT_METRICS;
            metrics_event.data_format = LLW_EVENT_DATA_JSON_UTF8;
            metrics_event.sequence = 0;
            metrics_event.data = bytes(
                "{\"promptTokens\":" + std::to_string(metrics_.prompt_tokens) +
                ",\"generatedTokens\":" + std::to_string(metrics_.generated_tokens) +
                ",\"decodeCalls\":" + std::to_string(metrics_.decode_calls) +
                ",\"queueWaitNanoseconds\":" + std::to_string(metrics_.queue_wait_ns) +
                ",\"decodeNanoseconds\":" + std::to_string(metrics_.decode_ns) + "}");
            // Metrics saturation drops telemetry without blocking request execution.
            try { (void)events_.publish(std::move(metrics_event)); }
            catch (...) {}
        } catch (...) {
            if (!lock.owns_lock()) {
                try { lock.lock(); } catch (...) { return; }
            }
            std::this_thread::yield();
        }
    }
}
