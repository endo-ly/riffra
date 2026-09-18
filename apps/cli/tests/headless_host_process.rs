use serde_json::Value;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

struct TestDataRoot {
    path: PathBuf,
}

impl TestDataRoot {
    fn new() -> Self {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before the Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "riffra-headless-host-{}-{suffix}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("temporary data root should be created");
        Self { path }
    }
}

impl Drop for TestDataRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

struct RunningHost {
    child: Child,
    data_root: TestDataRoot,
}

impl RunningHost {
    fn start(safe_mode: bool) -> Self {
        let data_root = TestDataRoot::new();
        let stdout = File::create(data_root.path.join("serve.stdout.log"))
            .expect("Host stdout log should be created");
        let stderr = File::create(data_root.path.join("serve.stderr.log"))
            .expect("Host stderr log should be created");
        let mut command = Command::new(env!("CARGO_BIN_EXE_riffra"));
        command
            .arg("--data-root")
            .arg(&data_root.path)
            .arg("serve")
            .stdout(stdout)
            .stderr(stderr);
        if safe_mode {
            command.arg("--safe-mode");
        }
        let mut child = command.spawn().expect("riffra serve should start");
        let endpoint = data_root.path.join("control").join("host.json");
        for _ in 0..200 {
            if endpoint.is_file() {
                return Self { child, data_root };
            }
            if let Some(status) = child
                .try_wait()
                .expect("Host process status should be readable")
            {
                let stderr = fs::read_to_string(data_root.path.join("serve.stderr.log"))
                    .unwrap_or_else(|error| format!("could not read Host stderr: {error}"));
                panic!("riffra serve exited before publishing its endpoint ({status}): {stderr}");
            }
            thread::sleep(Duration::from_millis(50));
        }
        panic!("riffra serve did not publish its endpoint within ten seconds");
    }

    fn data_root(&self) -> &Path {
        &self.data_root.path
    }

    fn wait_for_shutdown(&mut self) {
        for _ in 0..200 {
            if self
                .child
                .try_wait()
                .expect("Host process status should be readable")
                .is_some()
            {
                return;
            }
            thread::sleep(Duration::from_millis(50));
        }
        let _ = self.child.kill();
        panic!("riffra serve did not stop after host shutdown");
    }
}

impl Drop for RunningHost {
    fn drop(&mut self) {
        if self
            .child
            .try_wait()
            .expect("Host process status should be readable")
            .is_none()
        {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn host_list() -> Output {
    Command::new(env!("CARGO_BIN_EXE_riffra"))
        .args(["host", "list"])
        .output()
        .expect("host list should start")
}

fn attached(arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_riffra"))
        .arg("--attach")
        .args(arguments)
        .output()
        .expect("attached command should start")
}

fn standalone(data_root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_riffra"))
        .arg("--data-root")
        .arg(data_root)
        .args(arguments)
        .output()
        .expect("standalone command should start")
}

fn interactive_bootstrap() -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_riffra"))
        .args(["--attach", "--interactive"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("interactive Host command should start");
    child
        .stdin
        .take()
        .expect("interactive stdin should be available")
        .write_all(b"{\"requestId\":\"bootstrap\",\"command\":\"host.bootstrap\",\"params\":{}}\n")
        .expect("bootstrap request should be written");
    child
        .wait_with_output()
        .expect("interactive bootstrap should finish")
}

fn success_json(output: Output, operation: &str) -> Value {
    assert!(
        output.status.success(),
        "{operation} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{operation} returned invalid JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn assert_runtime_unavailable(output: Output, operation: &str) {
    assert!(
        !output.status.success(),
        "{operation} unexpectedly succeeded"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stderr.contains("runtimeUnavailable") || stdout.contains("runtimeUnavailable"),
        "{operation} did not report runtimeUnavailable: stderr={stderr}, stdout={stdout}"
    );
}

#[test]
fn headless_host_process_covers_lifecycle_and_mode_contracts() {
    let safe_mode = match std::env::var("RIFFRA_HEADLESS_SAFE_MODE").as_deref() {
        Ok("0") => false,
        Ok("1") | Err(std::env::VarError::NotPresent) => true,
        Ok(value) => panic!("RIFFRA_HEADLESS_SAFE_MODE must be 0 or 1, got {value}"),
        Err(error) => panic!("RIFFRA_HEADLESS_SAFE_MODE could not be read: {error}"),
    };
    let mut host = RunningHost::start(safe_mode);
    let data_root = host.data_root().to_path_buf();

    let hosts = success_json(host_list(), "host list");
    assert_eq!(hosts.as_array().map(Vec::len), Some(1));
    assert_eq!(hosts[0]["dataRoot"], data_root.to_string_lossy().as_ref());

    let session = success_json(attached(&["session", "get"]), "session.get");
    assert_eq!(session["result"]["type"], "session");
    assert_eq!(session["sequence"], 0);

    let bootstrap = success_json(interactive_bootstrap(), "host.bootstrap");
    assert_eq!(bootstrap["result"]["type"], "hostBootstrap");
    assert_eq!(bootstrap["result"]["value"]["canonical"]["sequence"], 0);

    let track = success_json(
        attached(&[
            "--expected-sequence",
            "0",
            "track",
            "add",
            "--name",
            "Process Test",
            "--kind",
            "instrument",
        ]),
        "track.add",
    );
    assert_eq!(track["result"]["type"], "mutation");
    assert_eq!(track["sequence"], 1);
    assert_eq!(
        track["result"]["value"]["createdEntityIds"]["tracks"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    assert!(track["result"]["value"].get("canonical").is_none());

    let undo = success_json(attached(&["--expected-sequence", "1", "undo"]), "undo");
    assert_eq!(undo["result"]["type"], "mutation");
    assert_eq!(undo["sequence"], 2);
    assert!(undo["result"]["value"].get("canonical").is_none());

    if safe_mode {
        let audio = success_json(attached(&["audio", "status"]), "audio.status");
        assert_eq!(audio["result"]["type"], "audioStatus");
        assert_runtime_unavailable(attached(&["transport", "play"]), "transport.play");
        assert_runtime_unavailable(attached(&["audio", "probe"]), "audio.probe");
        assert_runtime_unavailable(
            attached(&["plugin", "scan", "--path", data_root.to_str().unwrap()]),
            "plugin.scan",
        );
    } else {
        let status = success_json(attached(&["host", "status"]), "host.status");
        assert_eq!(status["result"]["type"], "hostStatus");
        let audio = success_json(attached(&["audio", "status"]), "audio.status");
        assert_eq!(audio["result"]["type"], "audioStatus");
    }

    let shutdown = success_json(attached(&["host", "shutdown"]), "host.shutdown");
    assert_eq!(shutdown["result"]["type"], "ok");
    host.wait_for_shutdown();

    assert!(!data_root.join("control").join("host.json").exists());
    #[cfg(unix)]
    assert!(!data_root.join("control").join("host.sock").exists());

    let reopened = success_json(
        standalone(&data_root, &["session", "get"]),
        "session reopen",
    );
    assert_eq!(reopened["result"]["type"], "session");
    assert_eq!(reopened["sequence"], 0);
}
