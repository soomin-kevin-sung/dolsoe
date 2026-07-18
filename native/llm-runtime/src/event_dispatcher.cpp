#include "event_dispatcher.h"
#include <chrono>
#include <stdexcept>
#include <utility>

EventDispatcher::EventDispatcher(llw_callback_table_t callbacks, uint32_t capacity)
    : callbacks_(callbacks), capacity_(capacity), thread_([this] { run(); }) {}

EventDispatcher::~EventDispatcher() { stop(); }

bool EventDispatcher::publish(OwnedEvent event) {
    std::unique_lock lock(mutex_);
#ifdef LLW_RUNTIME_TESTING
    if (fail_next_type_ == event.type) {
        fail_next_type_ = 0;
        return false;
    }
#endif
    writable_.wait(lock, [this] { return stopping_ || queue_.size() < capacity_; });
    if (stopping_) return false;
    queue_.push_back(DispatchItem{std::move(event), {}});
    readable_.notify_one();
    return true;
}

void EventDispatcher::flush() { flush_impl({}); }

#ifdef LLW_RUNTIME_TESTING
void EventDispatcher::flush_for_test(std::function<void()> barrier_enqueued) {
    flush_impl(std::move(barrier_enqueued));
}

void EventDispatcher::fail_next_publish_of_type_for_test(int32_t event_type) {
    std::lock_guard lock(mutex_);
    fail_next_type_ = event_type;
}
#endif

void EventDispatcher::flush_impl(std::function<void()> barrier_enqueued) {
    auto barrier = std::make_shared<std::promise<void>>();
    std::future<void> completed = barrier->get_future();
    {
        std::unique_lock lock(mutex_);
        if (std::this_thread::get_id() == callback_thread_)
            throw std::logic_error("event dispatcher flush is not callback-reentrant");
        writable_.wait(lock, [this] { return stopping_ || queue_.size() < capacity_; });
        if (stopping_) throw std::runtime_error("event dispatcher is stopping");
        queue_.push_back(DispatchItem{{}, std::move(barrier)});
        readable_.notify_one();
    }
    if (barrier_enqueued) barrier_enqueued();
    completed.get();
}

void EventDispatcher::stop() {
    {
        std::lock_guard lock(mutex_);
        if (stopping_) return;
        stopping_ = true;
    }
    readable_.notify_all();
    writable_.notify_all();
    if (thread_.joinable()) thread_.join();
}

void EventDispatcher::drain_for_test() {
    std::unique_lock lock(mutex_);
    if (!drained_.wait_for(lock, std::chrono::seconds(5),
                           [this] { return queue_.empty() && in_callback_ == 0; }))
        throw std::runtime_error("event dispatcher drain timeout");
}

void EventDispatcher::run() {
    {
        std::lock_guard lock(mutex_);
        callback_thread_ = std::this_thread::get_id();
    }
    for (;;) {
        DispatchItem item;
        {
            std::unique_lock lock(mutex_);
            readable_.wait(lock, [this] { return stopping_ || !queue_.empty(); });
            if (queue_.empty() && stopping_) break;
            item = std::move(queue_.front());
            queue_.pop_front();
            if (!item.barrier) ++in_callback_;
            writable_.notify_one();
        }
        if (item.barrier) {
            item.barrier->set_value();
            continue;
        }
        OwnedEvent& owned = item.event;
        if (callbacks_.on_event) {
            llw_event_t event{};
            event.struct_size = sizeof(event);
            event.flags = owned.data_format;
            event.event_type = owned.type;
            event.error_code = owned.error_code;
            event.model_handle = owned.model;
            event.request_handle = owned.request;
            event.slot_id = owned.slot;
            event.sequence_number = owned.sequence;
            event.data = owned.data.empty() ? nullptr : owned.data.data();
            event.data_len = owned.data.size();
            event.request_user_data = owned.request_user_data;
            callbacks_.on_event(&event, callbacks_.user_data);
        }
        {
            std::lock_guard lock(mutex_);
            --in_callback_;
            if (queue_.empty() && in_callback_ == 0) drained_.notify_all();
        }
    }
    std::lock_guard lock(mutex_);
    if (queue_.empty() && in_callback_ == 0) drained_.notify_all();
}
