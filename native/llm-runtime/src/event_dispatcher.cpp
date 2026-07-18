#include "event_dispatcher.h"
#include <chrono>
#include <new>
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

bool EventDispatcher::reserve_terminal(llw_handle_t request) {
    std::lock_guard lock(mutex_);
    if (stopping_ || request == 0 || terminal_permits_.size() >= terminal_capacity_ ||
        terminal_permits_.count(request) != 0)
        return false;
    terminal_permits_.emplace(request, TerminalPermitState::Reserved);
    return true;
}

bool EventDispatcher::release_terminal(llw_handle_t request) {
    std::lock_guard lock(mutex_);
    const auto found = terminal_permits_.find(request);
    if (found == terminal_permits_.end() || found->second != TerminalPermitState::Reserved)
        return false;
    terminal_permits_.erase(found);
    return true;
}

bool EventDispatcher::publish(OwnedEvent event) {
    const bool terminal = event.request != 0 &&
        (event.type == LLW_EVENT_DONE || event.type == LLW_EVENT_ERROR ||
         event.type == LLW_EVENT_CANCELLED);
    std::lock_guard lock(mutex_);
#ifdef LLW_RUNTIME_TESTING
    if (throw_next_type_ == event.type) {
        throw_next_type_ = 0;
        throw std::bad_alloc();
    }
    if (fail_next_type_ == event.type) {
        fail_next_type_ = 0;
        return false;
    }
#endif
    if (stopping_) return false;
    auto terminal_permit = terminal_permits_.end();
    if (terminal) {
        terminal_permit = terminal_permits_.find(event.request);
        if (terminal_permit == terminal_permits_.end() ||
            terminal_permit->second != TerminalPermitState::Reserved)
            return false;
    } else if (regular_queued_ >= regular_capacity_) {
        return false;
    }
    const Admission admission = terminal ? Admission::Terminal : Admission::Regular;
    queue_.push_back(DispatchItem{std::move(event), admission});
    ++enqueued_;
    if (terminal) terminal_permit->second = TerminalPermitState::Published;
    else ++regular_queued_;
    readable_.notify_one();
    return true;
}

void EventDispatcher::flush() { flush_impl(nullptr, nullptr); }

bool EventDispatcher::is_callback_thread() {
    std::lock_guard lock(mutex_);
    return std::this_thread::get_id() == callback_thread_;
}

#ifdef LLW_RUNTIME_TESTING
void EventDispatcher::flush_for_test(
    void (LLW_CALL *barrier_enqueued)(void*), void* user_data) {
    flush_impl(barrier_enqueued, user_data);
}

void EventDispatcher::fail_next_publish_of_type_for_test(int32_t event_type) {
    std::lock_guard lock(mutex_);
    fail_next_type_ = event_type;
}

void EventDispatcher::throw_next_publish_of_type_for_test(int32_t event_type) {
    std::lock_guard lock(mutex_);
    throw_next_type_ = event_type;
}

size_t EventDispatcher::terminal_permit_count_for_test() {
    std::lock_guard lock(mutex_);
    return terminal_permits_.size();
}
#endif

void EventDispatcher::flush_impl(
    void (LLW_CALL *barrier_enqueued)(void*), void* user_data) {
    uint64_t target{};
    {
        std::unique_lock lock(mutex_);
        if (std::this_thread::get_id() == callback_thread_)
            throw std::logic_error("event dispatcher flush is not callback-reentrant");
        if (stopping_) throw std::runtime_error("event dispatcher is stopping");
        target = enqueued_;
    }
    if (barrier_enqueued) {
        try { barrier_enqueued(user_data); } catch (...) {}
    }
    std::unique_lock lock(mutex_);
    flushed_.wait(lock, [this, target] { return stopping_ || completed_ >= target; });
    if (completed_ < target) throw std::runtime_error("event dispatcher stopped before flush");
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
    flushed_.notify_all();
    if (!join_worker) return;
    if (thread_.joinable()) thread_.join();
    {
        std::lock_guard lock(mutex_);
        terminal_permits_.clear();
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
            ++in_callback_;
        }
        const llw_handle_t terminal_request =
            item.admission == Admission::Terminal ? item.event.request : 0;
        auto callback_done = make_scope_exit([this, terminal_request] {
            std::lock_guard lock(mutex_);
            if (terminal_request != 0) terminal_permits_.erase(terminal_request);
            --in_callback_;
            ++completed_;
            flushed_.notify_all();
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
