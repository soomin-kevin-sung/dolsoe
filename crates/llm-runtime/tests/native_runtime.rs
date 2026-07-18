use std::path::PathBuf;
use std::time::{Duration, Instant};

use llm_runtime::{
    Backend, EventKind, GenerationOptions, InferenceRuntime, ModelOptions, RuntimeOptions,
};

fn required_path(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("{name} must be set by the explicit model-backed test command"))
}

#[test]
fn dropping_a_queued_request_cancels_it_once() {
    let dll = required_path("LLW_TEST_RUNTIME");
    let gguf = required_path("LLW_TEST_GGUF");
    let options = RuntimeOptions {
        slot_count: 1,
        request_queue_capacity: 4,
        event_queue_capacity: 1024,
    };
    // SAFETY: both paths are supplied by this repository's explicit, checksum-verified test flow.
    let runtime = unsafe { InferenceRuntime::load(&dll, options) }.expect("load runtime");
    let model = runtime
        .load_model(
            &gguf,
            ModelOptions {
                backend: Backend::Cpu,
                context_tokens_per_slot: 512,
                logical_batch_tokens: 1,
                physical_batch_tokens: 1,
                n_threads: 1,
                n_threads_batch: 1,
                n_gpu_layers: 0,
                ..ModelOptions::default()
            },
        )
        .expect("load tiny model");
    let blocker_prompt = vec![0xff; 480];
    let long = GenerationOptions {
        max_new_tokens: 128,
        seed: 11,
        ..GenerationOptions::default()
    };
    let blocker = model
        .submit(&blocker_prompt, long)
        .expect("submit active request");
    let queued = model
        .submit(
            b"The",
            GenerationOptions {
                max_new_tokens: 32,
                seed: 12,
                ..GenerationOptions::default()
            },
        )
        .expect("submit queued request");
    let queued_handle = queued.handle();
    let queued_terminal = queued.terminal_receiver();

    let state_deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let snapshot = runtime.scheduler_snapshot().expect("scheduler snapshot");
        if snapshot.active_count == 1 && snapshot.queued_count == 1 {
            break;
        }
        assert!(
            Instant::now() < state_deadline,
            "blocker never became active with the second request queued: active={}, queued={}",
            snapshot.active_count,
            snapshot.queued_count
        );
        std::thread::yield_now();
    }
    drop(queued);
    let event = queued_terminal
        .recv_timeout(Duration::from_secs(30))
        .expect("queued terminal before timeout");
    assert_eq!(event.request_handle, queued_handle);
    assert_eq!(event.kind, EventKind::Cancelled);
    assert!(queued_terminal
        .recv_timeout(Duration::from_millis(250))
        .is_err());
    blocker.cancel().expect("cancel active request");
    let blocker_event = blocker
        .recv_terminal_timeout(Duration::from_secs(30))
        .expect("active cancellation terminal before timeout");
    assert!(matches!(
        blocker_event.kind,
        EventKind::Cancelled | EventKind::Done
    ));
}
