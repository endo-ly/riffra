use riffra_control::{ControlRequest, LocalHostRegistry};
use riffra_runtime::{DawHost, HostConfig, NoopHostEventSink};
use serde_json::{Value, json};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;

static HOST_TEST_LOCK: Mutex<()> = Mutex::new(());

struct TestHost {
    host: DawHost,
    root: TempDir,
}

impl TestHost {
    fn start() -> Self {
        let root = tempfile::tempdir().expect("temporary data root should be created");
        let resources = root.path().join("resources");
        fs::create_dir(&resources).expect("resource root should be created");
        fs::write(
            resources.join("manifest.json"),
            br#"{"sourceRelease":"vtest","presets":[]}"#,
        )
        .expect("resource manifest should be written");
        let mut config = HostConfig::new(root.path().to_path_buf(), resources)
            .expect("Host configuration should be created");
        config.safe_mode = true;
        let host =
            DawHost::open(config, Arc::new(NoopHostEventSink)).expect("safe-mode Host should open");
        let instance_id = host.identity().instance_id.clone();
        let registry = LocalHostRegistry::current_user();
        for _ in 0..200 {
            let discoverable = registry
                .discover(|registration| {
                    ControlRequest::new(
                        format!("riffra-mcp-test-{}", registration.instance_id),
                        "host.status",
                        json!({}),
                        None,
                    )
                })
                .is_ok_and(|hosts| {
                    hosts
                        .iter()
                        .any(|host| host.registration.instance_id == instance_id)
                });
            if discoverable {
                return Self { host, root };
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("Host did not become discoverable within ten seconds");
    }

    fn stderr_path(&self) -> PathBuf {
        self.root.path().join("mcp.stderr.log")
    }
}

struct McpProcess {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
    stderr_path: PathBuf,
}

impl McpProcess {
    fn start(instance_id: &str, stderr_path: PathBuf) -> Self {
        let stderr = fs::File::create(&stderr_path).expect("MCP stderr log should be created");
        let mut child = Command::new(env!("CARGO_BIN_EXE_riffra-mcp"))
            .args(["--host", instance_id])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()
            .expect("riffra-mcp should start");
        let stdin = child.stdin.take().expect("MCP stdin should be available");
        let stdout = child.stdout.take().expect("MCP stdout should be available");
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else {
                    break;
                };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            next_id: 1,
            stderr_path,
        }
    }

    fn notify(&mut self, method: &str, params: Value) {
        let message = json!({"jsonrpc": "2.0", "method": method, "params": params});
        writeln!(self.stdin, "{message}").expect("notification should be written");
        self.stdin.flush().expect("notification should be flushed");
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        let message = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(self.stdin, "{message}").expect("request should be written");
        self.stdin.flush().expect("request should be flushed");
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(15))
                .unwrap_or_else(|error| {
                    let stderr = fs::read_to_string(&self.stderr_path)
                        .unwrap_or_else(|error| format!("could not read stderr: {error}"));
                    panic!("MCP response was not received ({error}):\n{stderr}");
                });
            let response: Value = serde_json::from_str(&line)
                .unwrap_or_else(|error| panic!("stdout was not JSON-RPC ({error}): {line}"));
            if response.get("id").and_then(Value::as_u64) == Some(id) {
                return response;
            }
        }
    }

    fn initialize(&mut self) -> Value {
        let response = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": {"name": "riffra-mcp-test", "version": "1"}
            }),
        );
        self.notify("notifications/initialized", json!({}));
        response
    }

    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
    }
}

impl Drop for McpProcess {
    fn drop(&mut self) {
        let _ = self.stdin.flush();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn structured(response: &Value) -> &Value {
    &response["result"]["structuredContent"]
}

fn tool_error(response: &Value) -> &Value {
    assert_eq!(response["result"]["isError"], true, "{response}");
    &response["result"]["structuredContent"]["error"]
}

fn silent_wav(path: &std::path::Path, seconds: u32) {
    let frames = 44_100u32 * seconds;
    let data_len = usize::try_from(frames * 2).expect("test WAV fits");
    let mut wav = vec![0u8; 44 + data_len];
    wav[0..4].copy_from_slice(b"RIFF");
    wav[4..8].copy_from_slice(&(36 + frames * 2).to_le_bytes());
    wav[8..12].copy_from_slice(b"WAVE");
    wav[12..16].copy_from_slice(b"fmt ");
    wav[16..20].copy_from_slice(&16u32.to_le_bytes());
    wav[20..22].copy_from_slice(&1u16.to_le_bytes());
    wav[22..24].copy_from_slice(&1u16.to_le_bytes());
    wav[24..28].copy_from_slice(&44_100u32.to_le_bytes());
    wav[28..32].copy_from_slice(&88_200u32.to_le_bytes());
    wav[32..34].copy_from_slice(&2u16.to_le_bytes());
    wav[34..36].copy_from_slice(&16u16.to_le_bytes());
    wav[36..40].copy_from_slice(b"data");
    wav[40..44].copy_from_slice(&(frames * 2).to_le_bytes());
    fs::write(path, wav).expect("test WAV should be written");
}

#[test]
fn job_wait_times_out_without_cancelling_the_job() {
    // Arrange
    let _host_test = HOST_TEST_LOCK.lock().expect("host test lock");
    let test_host = TestHost::start();
    let instance_id = test_host.host.identity().instance_id.clone();
    let wav = test_host.root.path().join("measure.wav");
    silent_wav(&wav, 10);
    let mut mcp = McpProcess::start(&instance_id, test_host.stderr_path());
    mcp.initialize();

    // Act
    let measured = mcp.tool(
        "analysis",
        json!({
            "action": "measure",
            "params": {
                "paths": [wav.to_string_lossy()],
                "ranges": [{"name": "all", "startSeconds": 0, "endSeconds": 10}]
            }
        }),
    );
    let job_id = structured(&measured)["result"]["value"]["id"]
        .as_str()
        .expect("analysis should start a job")
        .to_owned();
    let timed_out = mcp.tool(
        "job",
        json!({"action": "wait", "params": {"id": job_id, "timeoutMs": 0}}),
    );
    let still_running = mcp.tool("job", json!({"action": "get", "params": {"id": job_id}}));
    let completed = mcp.tool(
        "job",
        json!({"action": "wait", "params": {"id": job_id, "timeoutMs": 15000}}),
    );
    let missing = mcp.tool(
        "job",
        json!({"action": "wait", "params": {"id": "job:missing", "timeoutMs": 100}}),
    );

    // Assert
    let timed_out = tool_error(&timed_out);
    assert_eq!(timed_out["code"], "commandFailed");
    assert!(
        timed_out["details"]["state"] == "queued" || timed_out["details"]["state"] == "running"
    );
    assert!(
        !matches!(
            structured(&still_running)["result"]["value"]["state"].as_str(),
            Some("cancelled")
        ),
        "wait timeout must not cancel the job"
    );
    assert_eq!(
        structured(&completed)["result"]["value"]["state"],
        "completed"
    );
    let missing = tool_error(&missing);
    assert_eq!(missing["code"], "commandFailed");
}

#[test]
fn stdio_tools_share_the_live_host_and_preserve_control_results() {
    // Arrange
    let test_host = TestHost::start();
    let instance_id = test_host.host.identity().instance_id.clone();
    let mut mcp = McpProcess::start(&instance_id, test_host.stderr_path());

    // Act
    let initialized = mcp.initialize();
    let listed = mcp.request("tools/list", json!({}));

    // Assert
    assert_eq!(initialized["result"]["serverInfo"]["name"], "riffra-mcp");
    let tools = listed["result"]["tools"]
        .as_array()
        .expect("tools should be an array");
    assert_eq!(tools.len(), 48);
    assert_eq!(tools[0]["name"], "host_info");
    assert_eq!(tools[47]["name"], "audio_safety");
    assert_eq!(tools[0]["inputSchema"]["type"], "object");

    // Act
    let status = mcp.tool("host_info", json!({"action": "status"}));
    let inspected = mcp.tool("session_read", json!({"action": "inspect", "params": {}}));
    let projects = mcp.tool("project", json!({"action": "list"}));
    let original_project = structured(&projects)["result"]["value"]["activeProjectId"]
        .as_str()
        .expect("the Host should expose its active Project")
        .to_owned();
    let applied = mcp.tool(
        "session_edit",
        json!({
            "action": "apply",
            "params": {
                "operations": [
                    {"command": "track.add", "params": {"name": "Lead", "kind": "instrument"}},
                    {"command": "music.midi-clip.create", "params": {
                        "trackName": "Lead", "start": "1:1", "end": "5:1", "name": "Verse"
                    }},
                    {"command": "music.note.insert", "params": {
                        "trackName": "Lead",
                        "clipName": "Verse",
                        "notes": [
                            {"pitch": "C4", "position": "1:1", "duration": "1/4"},
                            {"pitch": "E4", "position": "1:2", "duration": "1/4"},
                            {"pitch": "G4", "position": "1:3", "duration": "1/4"}
                        ]
                    }}
                ],
                "includeCreatedIds": true
            }
        }),
    );

    // Assert
    assert_eq!(structured(&status)["command"], "host.status");
    assert_eq!(
        structured(&status)["result"]["value"]["instanceId"],
        instance_id
    );
    assert_eq!(structured(&inspected)["command"], "session.inspect");
    assert_eq!(
        structured(&inspected)["result"]["value"]["counts"]["tracks"],
        0
    );
    assert_eq!(structured(&applied)["command"], "session.apply");
    let created = &structured(&applied)["result"]["value"]["createdEntityIds"];
    let track_id = created["tracks"][0]
        .as_str()
        .expect("the batch should create one Track");
    assert_eq!(created["midiNotes"].as_array().unwrap().len(), 3);
    assert!(
        !structured(&applied)
            .as_object()
            .unwrap()
            .contains_key("canonical")
    );

    // Act
    let listed_tracks = mcp.tool("track", json!({"action": "list"}));
    let notes = mcp.tool(
        "music_note",
        json!({"action": "list", "params": {"trackId": track_id, "start": "1:1", "end": "5:1"}}),
    );
    let conflict = mcp.tool(
        "track",
        json!({
            "action": "add",
            "params": {"name": "Stale", "kind": "audio"},
            "expectedSequence": 0.0
        }),
    );
    let created_project = mcp.tool(
        "project",
        json!({"action": "create", "params": {"name": "Other"}}),
    );
    assert_eq!(structured(&created_project)["result"]["type"], "mutation");
    let switched = mcp.tool("project", json!({"action": "list"}));
    assert_ne!(
        structured(&switched)["result"]["value"]["activeProjectId"],
        original_project
    );
    let project_conflict = mcp.tool(
        "track",
        json!({
            "action": "add",
            "params": {"name": "Wrong Project", "kind": "audio"},
            "expectedProjectId": original_project
        }),
    );
    let invalid = mcp.tool(
        "clip_audio",
        json!({
            "action": "update",
            "params": {"clipId": "clip:1", "patch": {"muted": "yes"}}
        }),
    );
    let invalid_sequence = mcp.tool(
        "track",
        json!({
            "action": "add",
            "params": {"name": "Overflow", "kind": "audio"},
            "expectedSequence": 1e20
        }),
    );
    let unknown = mcp.request("tools/call", json!({"name": "not_a_tool", "arguments": {}}));

    // Assert
    assert!(
        structured(&listed_tracks)["result"]["value"]
            .as_array()
            .unwrap()
            .iter()
            .any(|track| track["name"] == "Lead")
    );
    assert_eq!(structured(&notes)["result"]["value"]["count"], 3, "{notes}");
    let project_conflict = tool_error(&project_conflict);
    assert_eq!(project_conflict["code"], "conflict");
    assert_eq!(
        project_conflict["details"]["expectedProjectId"],
        original_project
    );
    let conflict = tool_error(&conflict);
    assert_eq!(conflict["code"], "conflict");
    assert!(conflict["details"]["currentSequence"].is_u64());
    let invalid = tool_error(&invalid);
    assert_eq!(invalid["code"], "invalidRequest");
    assert_eq!(invalid["details"]["path"], "/params/patch/muted");
    let invalid_sequence = tool_error(&invalid_sequence);
    assert_eq!(invalid_sequence["code"], "invalidRequest");
    assert_eq!(invalid_sequence["details"]["path"], "/expectedSequence");
    assert!(unknown.get("error").is_some(), "{unknown}");

    // Act
    test_host.host.shutdown();
    let unavailable = mcp.tool("host_info", json!({"action": "status"}));

    // Assert
    let unavailable = tool_error(&unavailable);
    assert_eq!(unavailable["code"], "hostUnavailable");
}
