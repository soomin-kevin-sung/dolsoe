#![cfg(windows)]

use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

const CHILD_ENV: &str = "LLW_PACK_LOCAL_TEST_CHILD";
const DLL_ENV: &str = "LLW_PACK_LOCAL_TEST_DLL";

#[test]
fn loads_pack_local_dependency_when_working_directory_is_elsewhere() {
    if std::env::var_os(CHILD_ENV).is_some() {
        load_fixture_in_child();
        return;
    }

    let build_dir = test_scratch_dir("build");
    let elsewhere = test_scratch_dir("cwd");
    reset_dir(&build_dir);
    reset_dir(&elsewhere);

    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../native/llm-runtime/tests/fixtures/windows-pack-local");
    assert_success(
        Command::new("cmake")
            .arg("-S")
            .arg(&fixture)
            .arg("-B")
            .arg(&build_dir)
            .status()
            .expect("run CMake configure"),
        "configure loader fixture",
    );
    assert_success(
        Command::new("cmake")
            .arg("--build")
            .arg(&build_dir)
            .args(["--config", "Debug"])
            .status()
            .expect("run CMake build"),
        "build loader fixture",
    );

    let runtime = find_file(&build_dir, "local_llm_runtime.dll");
    let helper = find_file(&build_dir, "llw_pack_local_helper.dll");
    assert_eq!(
        runtime.parent(),
        helper.parent(),
        "fixture DLLs must be siblings"
    );

    let status = Command::new(std::env::current_exe().expect("locate test executable"))
        .args([
            "--exact",
            "loads_pack_local_dependency_when_working_directory_is_elsewhere",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .env(DLL_ENV, &runtime)
        .current_dir(&elsewhere)
        .status()
        .expect("run loader fixture child");
    std::fs::remove_dir_all(&build_dir).expect("remove fixture build directory");
    std::fs::remove_dir_all(&elsewhere).expect("remove fixture working directory");
    assert_success(status, "load fixture from unrelated working directory");
}

fn load_fixture_in_child() {
    let runtime = PathBuf::from(std::env::var_os(DLL_ENV).expect("fixture DLL path"));
    let api = unsafe { llm_runtime_sys::Api::load(&runtime) }.expect("load fixture runtime");
    let query = llm_runtime_sys::AbiQuery::default();
    let mut info = llm_runtime_sys::AbiInfo::default();
    let mut error = llm_runtime_sys::Error::default();
    let result = unsafe { (api.get_abi_info)(&query, &mut info, &mut error) };
    assert_eq!(result, llm_runtime_sys::OK);
    assert_eq!(info.abi_major, llm_runtime_sys::ABI_MAJOR);
    let version = unsafe { CStr::from_ptr((api.runtime_version)()) };
    assert_eq!(version.to_bytes(), b"pack-local-helper");
}

fn test_scratch_dir(name: &str) -> PathBuf {
    let local_data = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    local_data
        .join("llw-tests")
        .join(format!("pack-local-{}-{name}", std::process::id()))
}

fn reset_dir(path: &Path) {
    if path.exists() {
        std::fs::remove_dir_all(path).expect("remove stale fixture directory");
    }
    std::fs::create_dir_all(path).expect("create fixture directory");
}

fn find_file(root: &Path, name: &str) -> PathBuf {
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("read fixture build directory") {
            let entry = entry.expect("read fixture build entry");
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.file_name().is_some_and(|file_name| file_name == name) {
                return path;
            }
        }
    }
    panic!("did not find {name} beneath {}", root.display());
}

fn assert_success(status: ExitStatus, action: &str) {
    assert!(status.success(), "failed to {action}: {status}");
}
