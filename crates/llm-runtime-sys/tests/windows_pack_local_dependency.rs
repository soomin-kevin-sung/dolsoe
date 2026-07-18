#![cfg(windows)]

use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use llm_runtime_sys::{AbiInfo, AbiQuery, Api, Error, OK};

const CHILD_ENV: &str = "LLW_PACK_LOCAL_CHILD";
const RUNTIME_ENV: &str = "LLW_PACK_LOCAL_RUNTIME";

#[test]
fn loads_dependency_from_runtime_pack_directory() {
    if std::env::var_os(CHILD_ENV).is_some() {
        run_child();
        return;
    }

    let fixture = fixture_dir();
    let root = short_build_root();
    let build = root.join("build");
    let unrelated = root.join("cwd");
    std::fs::create_dir_all(&unrelated).expect("create unrelated cwd");

    run(Command::new("cmake")
        .arg("-S")
        .arg(&fixture)
        .arg("-B")
        .arg(&build));
    run(Command::new("cmake")
        .arg("--build")
        .arg(&build)
        .arg("--config")
        .arg("Debug"));

    let runtime = find_runtime(&build);
    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .arg("--exact")
        .arg("loads_dependency_from_runtime_pack_directory")
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .env(RUNTIME_ENV, &runtime)
        .current_dir(&unrelated)
        .output()
        .expect("re-execute integration test");
    assert!(
        output.status.success(),
        "child failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_child() {
    let runtime = PathBuf::from(std::env::var_os(RUNTIME_ENV).expect("runtime DLL path"));
    assert!(runtime.is_absolute());
    let api = unsafe { Api::load(&runtime) }.expect("load runtime and pack-local dependency");
    let query = AbiQuery::default();
    let mut info = AbiInfo::default();
    let mut error = Error::default();
    assert_eq!(
        unsafe { (api.get_abi_info)(&query, &mut info, &mut error) },
        OK
    );
    let version = unsafe { CStr::from_ptr((api.runtime_version)()) };
    assert_eq!(version.to_bytes(), b"pack-local-helper-sentinel");
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../native/llm-runtime/tests/fixtures/windows-pack-local")
}

fn short_build_root() -> PathBuf {
    let local = PathBuf::from(std::env::var_os("LOCALAPPDATA").expect("LOCALAPPDATA"));
    local.join(format!("llw-pack-test-{}", std::process::id()))
}

fn find_runtime(build: &Path) -> PathBuf {
    for candidate in [
        build.join("Debug/local_llm_runtime.dll"),
        build.join("local_llm_runtime.dll"),
    ] {
        if candidate.is_file() {
            return candidate.canonicalize().expect("canonical runtime DLL");
        }
    }
    panic!("runtime DLL not found beneath {}", build.display());
}

fn run(command: &mut Command) {
    let output = command.output().expect("run command");
    assert!(
        output.status.success(),
        "command failed: {command:?}\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
