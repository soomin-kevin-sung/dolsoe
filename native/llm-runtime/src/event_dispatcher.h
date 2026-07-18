#pragma once
#include "llw_runtime.h"
#include <condition_variable>
#include <cstddef>
#include <cstdint>
#include <deque>
#include <functional>
#include <future>
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
    bool publish(OwnedEvent event);
    void flush();
#ifdef LLW_RUNTIME_TESTING
    void flush_for_test(std::function<void()> barrier_enqueued);
    void fail_next_publish_of_type_for_test(int32_t event_type);
#endif
    void stop();
    void drain_for_test();
private:
    struct DispatchItem {
        OwnedEvent event;
        std::shared_ptr<std::promise<void>> barrier;
    };
    void flush_impl(std::function<void()> barrier_enqueued);
    void run();
    llw_callback_table_t callbacks_{};
    const size_t capacity_;
    std::mutex mutex_;
    std::condition_variable readable_;
    std::condition_variable writable_;
    std::condition_variable drained_;
    std::deque<DispatchItem> queue_;
    bool stopping_{};
    size_t in_callback_{};
    std::thread::id callback_thread_{};
#ifdef LLW_RUNTIME_TESTING
    int32_t fail_next_type_{};
#endif
    std::thread thread_;
};
