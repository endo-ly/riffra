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
    built_in_instruments_root: Option<PathBuf>,
}

fn prepare_safe_mode_resource_root(data_root: &Path) -> PathBuf {
    let resource_root = data_root.join("safe-mode-resources");
    fs::create_dir_all(&resource_root).expect("Safe Mode resource root should be created");
    fs::write(
        resource_root.join("manifest.json"),
        br#"{"sourceRelease":"vtest","presets":[]}"#,
    )
    .expect("Safe Mode resource manifest should be written");
    resource_root
}

impl RunningHost {
    fn start(safe_mode: bool) -> Self {
        let data_root = TestDataRoot::new();
        let safe_mode_resources =
            safe_mode.then(|| prepare_safe_mode_resource_root(&data_root.path));
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
        if let Some(resource_root) = &safe_mode_resources {
            command.env("RIFFRA_BUILTIN_INSTRUMENTS_ROOT", resource_root);
        }
        let mut child = command.spawn().expect("riffra serve should start");
        let endpoint = data_root.path.join("control").join("host.json");
        let mut endpoint_published = false;
        for _ in 0..200 {
            if endpoint.is_file() {
                endpoint_published = true;
                if host_is_discoverable(&data_root.path) {
                    return Self {
                        child,
                        data_root,
                        built_in_instruments_root: safe_mode_resources,
                    };
                }
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
        if endpoint_published {
            panic!("riffra serve did not become discoverable within ten seconds");
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

fn host_is_discoverable(data_root: &Path) -> bool {
    let output = host_list();
    if !output.status.success() {
        return false;
    }
    let Ok(hosts) = serde_json::from_slice::<Value>(&output.stdout) else {
        return false;
    };
    let data_root = data_root.to_string_lossy();
    hosts.as_array().is_some_and(|hosts| {
        hosts.iter().any(|host| {
            host.get("dataRoot").and_then(Value::as_str) == Some(data_root.as_ref())
                && host
                    .get("instanceId")
                    .and_then(Value::as_str)
                    .is_some_and(|instance_id| !instance_id.is_empty())
        })
    })
}

fn attached(instance_id: &str, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_riffra"))
        .args(["--attach", "--host", instance_id])
        .args(arguments)
        .output()
        .expect("attached command should start")
}

fn standalone(
    data_root: &Path,
    built_in_instruments_root: Option<&Path>,
    arguments: &[&str],
) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_riffra"));
    command.arg("--data-root").arg(data_root);
    if let Some(resource_root) = built_in_instruments_root {
        command.env("RIFFRA_BUILTIN_INSTRUMENTS_ROOT", resource_root);
    }
    command
        .args(arguments)
        .output()
        .expect("standalone command should start")
}

fn interactive_bootstrap(instance_id: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_riffra"))
        .args(["--attach", "--host", instance_id, "--interactive"])
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

fn structured_failure_json(output: Output, operation: &str) -> Value {
    assert!(
        !output.status.success(),
        "{operation} unexpectedly succeeded: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "{operation} did not return structured failure JSON ({error}): stdout={}, stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

#[test]
fn standalone_one_shot_failure_returns_structured_json() {
    let data_root = TestDataRoot::new();
    let resource_root = prepare_safe_mode_resource_root(&data_root.path);
    let output = standalone(
        &data_root.path,
        Some(&resource_root),
        &["track", "remove", "--track-id", "track:missing"],
    );
    let body = structured_failure_json(output, "standalone track.remove");
    assert_eq!(body["requestId"], "one-shot");
    assert_eq!(body["ok"], false);
    assert!(body["error"]["code"].is_string());
    assert!(body["error"]["message"].is_string());
}

#[test]
fn attached_one_shot_conflict_returns_structured_json() {
    let mut host = RunningHost::start(true);
    let data_root_string = host.data_root().to_string_lossy().into_owned();
    let hosts = success_json(host_list(), "host list");
    let instance_id = hosts
        .as_array()
        .expect("host list should return an array")
        .iter()
        .find(|entry| {
            entry.get("dataRoot").and_then(Value::as_str) == Some(data_root_string.as_str())
        })
        .and_then(|entry| entry.get("instanceId").and_then(Value::as_str))
        .expect("the started Host should be discoverable")
        .to_owned();

    let output = attached(
        &instance_id,
        &[
            "--expected-sequence",
            "999",
            "track",
            "add",
            "--name",
            "Conflict",
            "--kind",
            "instrument",
        ],
    );
    let body = structured_failure_json(output, "attached track.add conflict");
    assert_eq!(body["requestId"], "one-shot");
    assert_eq!(body["ok"], false);
    assert_eq!(body["error"]["code"], "conflict");
    assert!(body["error"]["details"]["expectedSequence"].is_number());
    assert!(body["error"]["details"]["currentSequence"].is_number());

    let shutdown = success_json(
        attached(&instance_id, &["host", "shutdown"]),
        "host.shutdown",
    );
    assert_eq!(shutdown["result"]["type"], "ok");
    host.wait_for_shutdown();
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
    let data_root_string = data_root.to_string_lossy().into_owned();

    let hosts = success_json(host_list(), "host list");
    let matching_hosts = hosts
        .as_array()
        .expect("host list should return an array")
        .iter()
        .filter(|host| {
            host.get("dataRoot").and_then(Value::as_str) == Some(data_root_string.as_str())
        })
        .collect::<Vec<_>>();
    assert_eq!(
        matching_hosts.len(),
        1,
        "the started Host should be discoverable exactly once"
    );
    let instance_id = matching_hosts[0]["instanceId"]
        .as_str()
        .expect("the discovered Host should have an instance id");

    let session = success_json(attached(instance_id, &["session", "get"]), "session.get");
    assert_eq!(session["result"]["type"], "session");
    assert_eq!(session["sequence"], 0);

    let bootstrap = success_json(interactive_bootstrap(instance_id), "host.bootstrap");
    assert_eq!(bootstrap["result"]["type"], "hostBootstrap");
    assert_eq!(bootstrap["result"]["value"]["canonical"]["sequence"], 0);

    let track = success_json(
        attached(
            instance_id,
            &[
                "--expected-sequence",
                "0",
                "track",
                "add",
                "--name",
                "Process Test",
                "--kind",
                "instrument",
            ],
        ),
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

    let undo = success_json(
        attached(instance_id, &["--expected-sequence", "1", "undo"]),
        "undo",
    );
    assert_eq!(undo["result"]["type"], "mutation");
    assert_eq!(undo["sequence"], 2);
    assert!(undo["result"]["value"].get("canonical").is_none());

    if safe_mode {
        let audio = success_json(attached(instance_id, &["audio", "status"]), "audio.status");
        assert_eq!(audio["result"]["type"], "audioStatus");
        assert_runtime_unavailable(
            attached(instance_id, &["transport", "play"]),
            "transport.play",
        );
        assert_runtime_unavailable(attached(instance_id, &["audio", "probe"]), "audio.probe");
        assert_runtime_unavailable(
            attached(
                instance_id,
                &["plugin", "scan", "--path", data_root.to_str().unwrap()],
            ),
            "plugin.scan",
        );
    } else {
        let status = success_json(attached(instance_id, &["host", "status"]), "host.status");
        assert_eq!(status["result"]["type"], "hostStatus");
        let audio = success_json(attached(instance_id, &["audio", "status"]), "audio.status");
        assert_eq!(audio["result"]["type"], "audioStatus");
    }

    assert_measurement_jobs(&mut host, instance_id);

    let client_directory = data_root.join("client");
    fs::create_dir(&client_directory).unwrap();
    success_json(
        Command::new(env!("CARGO_BIN_EXE_riffra"))
            .current_dir(&client_directory)
            .args([
                "--attach",
                "--host",
                instance_id,
                "project",
                "export",
                "--output",
                "song.riffra",
            ])
            .output()
            .expect("project export should start from the client directory"),
        "project.export with a relative client path",
    );
    assert!(client_directory.join("song.riffra").is_file());

    let shutdown = success_json(
        attached(instance_id, &["host", "shutdown"]),
        "host.shutdown",
    );
    assert_eq!(shutdown["result"]["type"], "ok");
    host.wait_for_shutdown();

    assert!(!data_root.join("control").join("host.json").exists());
    #[cfg(unix)]
    assert!(!data_root.join("control").join("host.sock").exists());

    let reopened = success_json(
        standalone(
            &data_root,
            host.built_in_instruments_root.as_deref(),
            &["session", "get"],
        ),
        "session reopen",
    );
    assert_eq!(reopened["result"]["type"], "session");
    assert_eq!(reopened["sequence"], 0);
}

fn assert_measurement_jobs(host: &mut RunningHost, instance_id: &str) {
    let root = host.data_root().to_path_buf();
    let before = success_json(
        attached(instance_id, &["session", "get"]),
        "session before measurement",
    );
    let write_wav = |path: &Path, seconds: u32| {
        let size = 48_000 * seconds * 2;
        let mut file = File::create(path).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + size).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16_u32.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&1_u16.to_le_bytes()).unwrap();
        file.write_all(&48_000_u32.to_le_bytes()).unwrap();
        file.write_all(&96_000_u32.to_le_bytes()).unwrap();
        file.write_all(&2_u16.to_le_bytes()).unwrap();
        file.write_all(&16_u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&size.to_le_bytes()).unwrap();
        file.write_all(&vec![0; size as usize]).unwrap();
    };
    let first = root.join("first.wav");
    let second = root.join("second.wav");
    write_wav(&first, 1);
    write_wav(&second, 2);
    let input = root.join("measure.json");
    let ranges = serde_json::json!([
        {"name":"later","startSeconds":0.5,"endSeconds":1.0},
        {"name":"overlap","startSeconds":0.25,"endSeconds":0.75}
    ]);
    fs::write(
        &input,
        serde_json::to_vec(&serde_json::json!({"paths":[second, first], "ranges":ranges})).unwrap(),
    )
    .unwrap();
    let started = success_json(
        attached(
            instance_id,
            &["analysis", "measure", "--file", input.to_str().unwrap()],
        ),
        "analysis.measure",
    );
    let id = started["result"]["value"]["id"].as_str().unwrap();

    let completed = success_json(
        attached(
            instance_id,
            &["job", "wait", "--id", id, "--timeout-ms", "30000"],
        ),
        "completed job wait",
    );
    assert_eq!(completed["result"]["value"]["state"], "completed");
    assert_eq!(completed["result"]["value"]["kind"], "analysisMeasure");
    let sources = &completed["result"]["value"]["result"]["sources"];
    assert_eq!(sources[0]["path"], second.to_string_lossy().as_ref());
    assert_eq!(sources[1]["path"], first.to_string_lossy().as_ref());
    assert_eq!(sources[0]["sections"][0]["name"], "later");
    assert_eq!(sources[0]["sections"][1]["name"], "overlap");
    let after = success_json(
        attached(instance_id, &["session", "get"]),
        "session after measurement",
    );
    assert_eq!(before["sequence"], after["sequence"]);
    assert_eq!(before["result"], after["result"]);

    let failed = success_json(
        attached(instance_id, &["render", "start"]),
        "empty render job start",
    );
    let id = failed["result"]["value"]["id"].as_str().unwrap();
    let failure = structured_failure_json(
        attached(
            instance_id,
            &["job", "wait", "--id", id, "--timeout-ms", "30000"],
        ),
        "failed job wait",
    );
    assert_eq!(failure["ok"], true);
    assert_eq!(failure["result"]["value"]["state"], "failed");
    assert!(
        !failure["result"]["value"]["message"]
            .as_str()
            .unwrap()
            .is_empty()
    );

    write_wav(&second, 120);
    fs::write(
        &input,
        serde_json::to_vec(&serde_json::json!({"paths":vec![second; 20], "ranges":ranges}))
            .unwrap(),
    )
    .unwrap();
    let started = success_json(
        attached(
            instance_id,
            &["analysis", "measure", "--file", input.to_str().unwrap()],
        ),
        "long analysis start",
    );
    let id = started["result"]["value"]["id"].as_str().unwrap();
    let timeout = attached(
        instance_id,
        &["job", "wait", "--id", id, "--timeout-ms", "0"],
    );
    assert!(!timeout.status.success());
    assert!(String::from_utf8_lossy(&timeout.stderr).contains("timed out"));
    success_json(
        attached(instance_id, &["job", "cancel", "--id", id]),
        "analysis cancellation",
    );
    let cancelled = structured_failure_json(
        attached(
            instance_id,
            &["job", "wait", "--id", id, "--timeout-ms", "30000"],
        ),
        "cancelled job wait",
    );
    assert_eq!(cancelled["ok"], true);
    assert_eq!(cancelled["result"]["value"]["state"], "cancelled");
}
