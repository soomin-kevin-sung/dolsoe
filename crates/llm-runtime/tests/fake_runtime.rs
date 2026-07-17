use llm_runtime::{Backend, RuntimeLibrary};

#[test]
fn probes_fake_runtime() {
    let path = std::env::var_os("LLW_TEST_RUNTIME")
        .map(std::path::PathBuf::from)
        .expect("LLW_TEST_RUNTIME must point to the fake DLL");
    let runtime = RuntimeLibrary::load(&path).expect("load fake runtime");
    let info = runtime.info();
    assert_eq!(info.abi_major, 1);
    assert_eq!(info.runtime_version, "0.1.0-fake");
    assert_eq!(info.llama_cpp_commit, "not-linked");
    assert!(info.capabilities.supports_cpu);
    assert_eq!(info.capabilities.max_parallel_slots, 4);
    let devices = runtime.devices(Backend::Cpu).expect("list CPU devices");
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].id, "cpu:0");
}
