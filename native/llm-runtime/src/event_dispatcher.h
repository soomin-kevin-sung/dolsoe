#pragma once
#include "llw_runtime.h"
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <functional>
#include <future>
#include <map>
#include <memory>
#include <mutex>
#include <thread>
#include <vector>

struct OwnedEvent {
    int32_t type{};
    uint32_t data_format{};
    int32_t error_code{};
    llw_handle_t model{};
    llw_handle_t request{};
    uint32_t slot{UINT32_MAX};
    uint64_t sequence{}; // Assigned by Scheduler::publish_locked for request events.
    void* request_user_data{};
    std::vector<uint8_t> data;
};

class EventDispatcher {
public:
    EventDispatcher(llw_callback_table_t callbacks, uint32_t capacity);
    ~EventDispatcher();
    EventDispatcher(const EventDispatcher&) = delete;
    EventDispatcher& operator=(const EventDispatcher&) = delete;
    // Reserve before accepting a native request; release only to roll back acceptance.
    bool reserve_terminal(llw_handle_t request);
    bool release_terminal(llw_handle_t request);
    bool publish(OwnedEvent event);
    void flush();
#ifdef LLW_RUNTIME_TESTING
    void flush_for_test(std::function<void()> barrier_enqueued,
                        std::function<void()> waiting_for_control = {});
    void fail_next_publish_of_type_for_test(int32_t event_type);
    size_t terminal_permit_count_for_test();
#endif
    void stop();
    void drain_for_test();
private:
    enum class Admission { Regular, Terminal, Control };
    enum class TerminalPermitState { Reserved, Published };
    struct DispatchItem {
        OwnedEvent event;
        std::shared_ptr<std::promise<void>> barrier;
        Admission admission{Admission::Regular};
    };
    void flush_impl(std::function<void()> barrier_enqueued,
                    std::function<void()> waiting_for_control);
    void run();
    llw_callback_table_t callbacks_{};
    // Terminal permits bound reserved, queued, and callback-active request lifetimes together.
    // The deque holds at most regular_capacity_ + terminal_capacity_ + one barrier.
    const size_t regular_capacity_;
    static constexpr size_t terminal_capacity_ = LLW_MAX_QUEUE_CAPACITY + LLW_MAX_SLOTS;
    std::mutex mutex_;
    std::condition_variable readable_;
    std::condition_variable control_writable_;
    std::condition_variable drained_;
    std::condition_variable joined_;
    std::deque<DispatchItem> queue_;
    size_t regular_queued_{};
    std::map<llw_handle_t, TerminalPermitState> terminal_permits_;
    bool barrier_queued_{};
    bool stopping_{};
    bool join_started_{};
    bool join_finished_{};
    size_t in_callback_{};
    std::thread::id callback_thread_{};
#ifdef LLW_RUNTIME_TESTING
    int32_t fail_next_type_{};
#endif
    std::thread thread_;
};
