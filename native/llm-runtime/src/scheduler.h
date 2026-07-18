#pragma once
#include "event_dispatcher.h"
#include "inference_engine.h"
#include "llw_runtime.h"
#include <chrono>
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <functional>
#include <map>
#include <mutex>
#include <optional>
#include <string>
#include <thread>
#include <vector>

enum class RequestState { Queued, Preprocessing, Running, Done, Cancelled, Error };

class Scheduler {
public:
    using TimePoint = std::chrono::steady_clock::time_point;
    using NowFn = std::function<TimePoint()>;
    Scheduler(uint32_t slots, uint32_t queue_capacity, InferenceEngine& engine,
              EventDispatcher& events,
              NowFn now = [] { return std::chrono::steady_clock::now(); });
    ~Scheduler();
    llw_result_t submit(const llw_request_params_t& params, llw_handle_t& out, std::string& error);
    llw_result_t cancel(llw_handle_t handle, std::string& error);
    llw_scheduler_snapshot_t snapshot() const;
    llw_metrics_t metrics() const;
    size_t tracked_request_count_for_test() const;
#ifdef LLW_RUNTIME_TESTING
    enum class SubmitFailurePoint { RequestInsert, QueueInsert };
    void fail_next_submit_for_test(SubmitFailurePoint point);
    void set_worker_paused_for_test(bool paused);
#endif
    void cancel_all_and_wait();
private:
    struct Request {
        llw_handle_t handle{};
        llw_handle_t model{};
        RequestState state{RequestState::Queued};
        std::vector<uint8_t> prompt;
        std::vector<std::vector<uint8_t>> stops;
        SamplingConfig sampling{};
        uint32_t max_new_tokens{};
        uint32_t generated_tokens{};
        uint64_t prompt_tokens{};
        uint32_t slot_id{UINT32_MAX};
        void* user_data{};
        bool cancel_requested{};
        bool terminal_emitted{};
        bool terminal_pending{};
        bool engine_started{};
        bool cleanup_attempted{};
        RequestState pending_terminal_state{RequestState::Error};
        int32_t pending_terminal_error{LLW_ERR_INTERNAL};
        std::string pending_terminal_message;
        uint64_t next_sequence{1};
        TimePoint enqueued_at{};
        TimePoint started_at{};
    };
    struct Slot { uint32_t id{}; llw_handle_t request{}; };
    void run();
    void promote_locked();
    void finish_locked(llw_handle_t, RequestState, int32_t, std::string);
    bool try_publish_terminal_locked(llw_handle_t) noexcept;
    bool publish_locked(Request&, int32_t, uint32_t, int32_t, std::vector<uint8_t>, uint32_t);
    bool has_work_locked() const;
    uint32_t slots_count_{};
    uint32_t queue_capacity_{};
    InferenceEngine& engine_;
    EventDispatcher& events_;
    NowFn now_;
    mutable std::mutex mutex_;
    std::condition_variable wake_;
    std::condition_variable idle_;
    std::deque<llw_handle_t> queued_;
    std::map<llw_handle_t, Request> requests_;
    std::vector<Slot> slots_;
    bool stopping_{};
    std::thread worker_;
    llw_metrics_t metrics_{};
    uint64_t accepted_{};
    uint64_t terminal_{};
#ifdef LLW_RUNTIME_TESTING
    std::optional<SubmitFailurePoint> submit_failure_;
    bool worker_paused_{};
#endif
};
