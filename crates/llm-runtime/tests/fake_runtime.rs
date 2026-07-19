use llm_runtime::{Backend, RuntimeLibrary};

#[test]
fn probes_native_runtime_contract() {
    let path = std::env::var_os("LLW_TEST_RUNTIME")
        .map(std::path::PathBuf::from)
        .expect("LLW_TEST_RUNTIME must point to the staged native runtime DLL");
    // SAFETY: CI stages this repository's conforming native runtime pack before this test.
    let runtime = unsafe { RuntimeLibrary::load(&path) }.expect("load native runtime");
    let info = runtime.info();
    assert_eq!(info.abi_major, 1);
    assert_eq!(info.runtime_version, "0.2.0");
    assert_eq!(
        info.llama_cpp_commit,
        "571d0d540df04f25298d0e159e520d9fc62ed121"
    );
    assert!(info.capabilities.supports_cpu);
    assert_eq!(info.capabilities.max_parallel_slots, 4);
    let devices = runtime.devices(Backend::Cpu).expect("list CPU devices");
    assert_eq!(devices.len(), 1);
    assert!(!devices[0].id.is_empty());
}
