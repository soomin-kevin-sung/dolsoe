#include "event_dispatcher.h"
#include <chrono>
#include <stdexcept>
#include <utility>

namespace {

template <typename Function>
class ScopeExit {
public:
    explicit ScopeExit(Function function) : function_(std::move(function)) {}
    ~ScopeExit() noexcept { function_(); }
    ScopeExit(const ScopeExit&) = delete;
    ScopeExit& operator=(const ScopeExit&) = delete;
private:
    Function function_;
};

template <typename Function>
ScopeExit<Function> make_scope_exit(Function function) {
    return ScopeExit<Function>(std::move(function));
}

} // namespace

EventDispatcher::EventDispatcher(llw_callback_table_t callbacks, uint32_t capacity)
    : callbacks_(callbacks), regular_capacity_(capacity), thread_([this] { run(); }) {}

EventDispatcher::~EventDispatcher() { stop(); }

bool EventDispatcher::publish(OwnedEvent event) {
    const bool terminal = event.request != 0 &&
        (event.type == LLW_EVENT_DONE || event.type == LLW_EVENT_ERROR ||
         event.type == LLW_EVENT_CANCELLED);
    std::lock_guard lock(mutex_);
#ifdef LLW_RUNTIME_TESTING
    if (fail_next_type_ == event.type) {
        fail_next_type_ = 0;
        return false;
    }
#endif
    if (stopping_) return false;
    if (terminal) {
        if (terminal_queued_ >= terminal_capacity_) return false;
    } else if (regular_queued_ >= regular_capacity_) {
        return false;
    }
    const Admission admission = terminal ? Admission::Terminal : Admission::Regular;
    queue_.push_back(DispatchItem{std::move(event), {}, admission});
    if (terminal) ++terminal_queued_;
    else ++regular_queued_;
    readable_.notify_one();
    return true;
}

void EventDispatcher::flush() { flush_impl({}, {}); }

#ifdef LLW_RUNTIME_TESTING
void EventDispatcher::flush_for_test(std::function<void()> barrier_enqueued,
                                     std::function<void()> waiting_for_control) {
    flush_impl(std::move(barrier_enqueued), std::move(waiting_for_control));
}

void EventDispatcher::fail_next_publish_of_type_for_test(int32_t event_type) {
    std::lock_guard lock(mutex_);
    fail_next_type_ = event_type;
}
#endif

void EventDispatcher::flush_impl(std::function<void()> barrier_enqueued,
                                 std::function<void()> waiting_for_control) {
    auto barrier = std::make_shared<std::promise<void>>();
    std::future<void> completed = barrier->get_future();
    {
        std::unique_lock lock(mutex_);
        if (std::this_thread::get_id() == callback_thread_)
            throw std::logic_error("event dispatcher flush is not callback-reentrant");
        if (barrier_queued_ && waiting_for_control) {
            lock.unlock();
            waiting_for_control();
            lock.lock();
        }
        control_writable_.wait(lock, [this] { return stopping_ || !barrier_queued_; });
        if (stopping_) throw std::runtime_error("event dispatcher is stopping");
        queue_.push_back(DispatchItem{{}, std::move(barrier), Admission::Control});
        barrier_queued_ = true;
        readable_.notify_one();
    }
    if (barrier_enqueued) barrier_enqueued();
    completed.get();
}

void EventDispatcher::stop() {
    bool join_worker = false;
    {
        std::unique_lock lock(mutex_);
        stopping_ = true;
        if (std::this_thread::get_id() != callback_thread_) {
            if (!join_started_) {
                join_started_ = true;
                join_worker = true;
            } else {
                joined_.wait(lock, [this] { return join_finished_; });
                return;
            }
        }
    }
    readable_.notify_all();
    control_writable_.notify_all();
    if (!join_worker) return;
    if (thread_.joinable()) thread_.join();
    {
        std::lock_guard lock(mutex_);
        join_finished_ = true;
    }
    joined_.notify_all();
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
            if (item.admission == Admission::Regular) --regular_queued_;
            else if (item.admission == Admission::Terminal) --terminal_queued_;
            else {
                barrier_queued_ = false;
                control_writable_.notify_one();
            }
            if (!item.barrier) ++in_callback_;
        }
        if (item.barrier) {
            item.barrier->set_value();
            std::lock_guard lock(mutex_);
            if (queue_.empty() && in_callback_ == 0) drained_.notify_all();
            continue;
        }
        auto callback_done = make_scope_exit([this] {
            std::lock_guard lock(mutex_);
            --in_callback_;
            if (queue_.empty() && in_callback_ == 0) drained_.notify_all();
        });
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
            try {
                callbacks_.on_event(&event, callbacks_.user_data);
            } catch (...) {
                // Callbacks are ABI boundaries; user exceptions cannot unwind the worker.
            }
        }
    }
    std::lock_guard lock(mutex_);
    if (queue_.empty() && in_callback_ == 0) drained_.notify_all();
}
