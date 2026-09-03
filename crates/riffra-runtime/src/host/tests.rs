use super::*;
use riffra_control::{
    ControlCommand, HelloRequest, HelloResponse, LocalHostClient, LocalHostRegistry, endpoint_path,
    new_instance_id, read_endpoint, transport,
};

#[test]
fn plugin_parameter_changes_coalesce_per_parameter_index() {
    let mut pending = HashMap::new();
    let mut next_order = 0;
    collect_plugin_change(
        &mut pending,
        riffra_control::HostEventFrame::new(
            "track-plugin-parameter-changed",
            serde_json::json!({
                "trackId": "track:1",
                "deviceId": "device:1",
                "parameterIndex": 1,
                "value": 0.25,
            }),
        ),
        &mut next_order,
    );
    collect_plugin_change(
        &mut pending,
        riffra_control::HostEventFrame::new(
            "track-plugin-parameter-changed",
            serde_json::json!({
                "trackId": "track:1",
                "deviceId": "device:1",
                "parameterIndex": 2,
                "value": 0.75,
            }),
        ),
        &mut next_order,
    );
    collect_plugin_change(
        &mut pending,
        riffra_control::HostEventFrame::new(
            "track-plugin-parameter-changed",
            serde_json::json!({
                "trackId": "track:1",
                "deviceId": "device:1",
                "parameterIndex": 1,
                "value": 0.5,
            }),
        ),
        &mut next_order,
    );

    assert_eq!(pending.len(), 2);
    assert!(matches!(
        pending.get(&PluginChangeKey::Parameter(
            "track:1".into(),
            "device:1".into(),
            1,
        )),
        Some(QueuedPluginChange {
            change: PendingPluginChange::Parameter(change),
            ..
        }) if change.value == 0.5
    ));
    assert!(matches!(
        pending.get(&PluginChangeKey::Parameter(
            "track:1".into(),
            "device:1".into(),
            2,
        )),
        Some(QueuedPluginChange {
            change: PendingPluginChange::Parameter(change),
            ..
        }) if change.value == 0.75
    ));
}

#[test]
fn runtime_restart_discards_pending_plugin_changes() {
    let mut pending = HashMap::new();
    let mut next_order = 0;
    collect_plugin_change(
        &mut pending,
        riffra_control::HostEventFrame::new(
            "track-plugin-parameter-changed",
            serde_json::json!({
                "trackId": "track:1",
                "deviceId": "device:1",
                "parameterIndex": 1,
                "value": 0.5,
            }),
        ),
        &mut next_order,
    );
    collect_plugin_change(
        &mut pending,
        riffra_control::HostEventFrame::new(
            "runtime-restarted",
            serde_json::json!({"generation": 2}),
        ),
        &mut next_order,
    );

    assert!(pending.is_empty());
}

#[test]
fn safe_mode_host_publishes_endpoint_and_handles_attached_mutation() {
    let data_root = std::env::temp_dir().join(format!(
        "riffra-runtime-host-{}-{}",
        std::process::id(),
        new_instance_id()
    ));
    let config = HostConfig {
        data_root: data_root.clone(),
        safe_mode: true,
        binaries: RuntimeBinaries::new(
            data_root.join("riffra-audio"),
            data_root.join("riffra-plugin-scan"),
            data_root.join("riffra-render"),
        ),
    };
    let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
    let descriptor = read_endpoint(&data_root).unwrap();

    {
        let mut stream = transport::connect(descriptor.endpoint()).unwrap();
        transport::write_frame(&mut stream, &HelloRequest::new()).unwrap();
        let hello: HelloResponse = transport::read_frame(&mut stream).unwrap();
        assert_eq!(hello.instance_id, descriptor.instance_id);

        transport::write_frame(
            &mut stream,
            &ControlRequest::new(
                "session-get",
                ControlCommand::new("session.get", serde_json::json!({})),
                Some(0),
            ),
        )
        .unwrap();
        let session_response: ControlResponse = transport::read_frame(&mut stream).unwrap();
        assert!(session_response.ok);
        assert_eq!(session_response.sequence, Some(0));
        assert_eq!(
            session_response
                .result
                .as_ref()
                .map(|result| result.result_type.as_str()),
            Some("session")
        );

        let request = ControlRequest::new(
            "host-test",
            ControlCommand::new(
                "track.add",
                serde_json::json!({"name": "Synth", "kind": "instrument"}),
            ),
            Some(0),
        );
        transport::write_frame(&mut stream, &request).unwrap();
        let response: ControlResponse = transport::read_frame(&mut stream).unwrap();
        assert!(response.ok);
        assert_eq!(response.sequence, Some(1));
    }

    assert_eq!(
        host.runtime_status().unwrap().state,
        crate::RuntimeProjectionState::Idle
    );
    host.shutdown();
    assert!(!endpoint_path(&data_root).exists());
    drop(host);
    let _ = std::fs::remove_dir_all(data_root);
}

#[test]
fn stale_render_and_undo_requests_are_rejected_by_the_canonical_sequence() {
    let data_root = std::env::temp_dir().join(format!(
        "riffra-runtime-sequence-guard-{}-{}",
        std::process::id(),
        new_instance_id()
    ));
    let config = HostConfig {
        data_root: data_root.clone(),
        safe_mode: true,
        binaries: RuntimeBinaries::new(
            data_root.join("riffra-audio"),
            data_root.join("riffra-plugin-scan"),
            data_root.join("riffra-render"),
        ),
    };
    let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();

    let mutation = host.dispatch_control(ControlRequest::new(
        "track-add",
        ControlCommand::new(
            "track.add",
            serde_json::json!({"name": "Synth", "kind": "instrument"}),
        ),
        Some(0),
    ));
    assert!(mutation.ok);
    assert_eq!(mutation.sequence, Some(1));

    let undo = host.dispatch_control(ControlRequest::new(
        "stale-undo",
        ControlCommand::new("undo", serde_json::json!({})),
        Some(0),
    ));
    assert!(!undo.ok);
    assert_eq!(
        undo.error.as_ref().map(|error| error.code),
        Some(ErrorCode::Conflict)
    );

    let render = host.dispatch_control(ControlRequest::new(
        "stale-render",
        ControlCommand::new("render.start", serde_json::json!({})),
        Some(0),
    ));
    assert!(!render.ok);
    assert_eq!(
        render.error.as_ref().map(|error| error.code),
        Some(ErrorCode::Conflict)
    );

    host.shutdown();
    drop(host);
    let _ = std::fs::remove_dir_all(data_root);
}

#[test]
fn host_info_returns_the_lightweight_selector_payload() {
    let data_root = std::env::temp_dir().join(format!(
        "riffra-runtime-info-{}-{}",
        std::process::id(),
        new_instance_id()
    ));
    let config = HostConfig {
        data_root: data_root.clone(),
        safe_mode: true,
        binaries: RuntimeBinaries::new(
            data_root.join("riffra-audio"),
            data_root.join("riffra-plugin-scan"),
            data_root.join("riffra-render"),
        ),
    };
    let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
    let client = LocalHostClient::connect_data_root(&data_root).unwrap();

    let response = client
        .request(&ControlRequest::new(
            "info",
            ControlCommand::new("host.info", serde_json::json!({})),
            None,
        ))
        .unwrap();

    assert!(response.ok);
    let info = response.result.unwrap().value;
    assert_eq!(info["instanceId"], host.identity().instance_id);
    assert_eq!(info["pid"], host.identity().pid);
    assert_eq!(info["dataRoot"], data_root.to_string_lossy().into_owned());
    assert!(info["projectName"].is_null());
    assert_eq!(info["safeMode"], true);
    assert_eq!(info["runtimeState"], "offline");

    host.shutdown();
    drop(host);
    let _ = std::fs::remove_dir_all(data_root);
}

#[test]
fn a_data_root_owned_by_another_host_is_reported_as_data_root_in_use() {
    let data_root = std::env::temp_dir().join(format!(
        "riffra-runtime-in-use-{}-{}",
        std::process::id(),
        new_instance_id()
    ));
    let config = HostConfig {
        data_root: data_root.clone(),
        safe_mode: true,
        binaries: RuntimeBinaries::new(
            data_root.join("riffra-audio"),
            data_root.join("riffra-plugin-scan"),
            data_root.join("riffra-render"),
        ),
    };
    let owner = DawHost::open(config.clone(), Arc::new(crate::NoopHostEventSink)).unwrap();

    let second = DawHost::open(config, Arc::new(crate::NoopHostEventSink));

    assert!(matches!(second, Err(HostError::DataRootInUse)));
    owner.shutdown();
    drop(owner);
    let _ = std::fs::remove_dir_all(data_root);
}

#[test]
fn shared_client_receives_bootstrap_and_canonical_events() {
    let data_root = std::env::temp_dir().join(format!(
        "riffra-runtime-client-{}-{}",
        std::process::id(),
        new_instance_id()
    ));
    let config = HostConfig {
        data_root: data_root.clone(),
        safe_mode: true,
        binaries: RuntimeBinaries::new(
            data_root.join("riffra-audio"),
            data_root.join("riffra-plugin-scan"),
            data_root.join("riffra-render"),
        ),
    };
    let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
    let client = LocalHostClient::connect_data_root(&data_root).unwrap();
    let mut events = client.open_event_stream().unwrap();

    let bootstrap = client
        .request(&ControlRequest::new(
            "bootstrap",
            ControlCommand::new("host.bootstrap", serde_json::json!({})),
            Some(0),
        ))
        .unwrap();
    assert!(bootstrap.ok);
    let bootstrap: HostBootstrap = serde_json::from_value(bootstrap.result.unwrap().value).unwrap();
    assert_eq!(bootstrap.canonical.sequence, 0);

    let mutation = client
        .request(&ControlRequest::new(
            "track-add",
            ControlCommand::new(
                "track.add",
                serde_json::json!({"name": "Synth", "kind": "instrument"}),
            ),
            Some(0),
        ))
        .unwrap();
    assert!(mutation.ok);
    assert_eq!(
        mutation
            .result
            .as_ref()
            .map(|result| result.result_type.as_str()),
        Some("arrangementMutation")
    );
    let mutation_result: crate::model::ArrangementMutationResult =
        serde_json::from_value(mutation.result.unwrap().value).unwrap();
    assert_eq!(mutation_result.canonical.sequence, 1);
    assert!(matches!(
        mutation_result.projection,
        crate::model::ArrangementProjectionOutcome::NotRequired
    ));
    let event = events.recv().unwrap();
    assert_eq!(event.event, "canonical-state-changed");
    assert_eq!(event.payload["sequence"], 1);

    let discovered = LocalHostRegistry::current_user()
        .discover()
        .unwrap()
        .into_iter()
        .find(|entry| entry.registration.instance_id == host.identity().instance_id);
    assert!(discovered.is_some());
    drop(discovered);

    host.shutdown();
    assert!(!endpoint_path(&data_root).exists());
    drop(host);
    let _ = std::fs::remove_dir_all(data_root);
}

#[test]
fn an_open_client_cannot_mutate_after_shutdown_and_the_root_reopens() {
    let data_root = std::env::temp_dir().join(format!(
        "riffra-runtime-shutdown-{}-{}",
        std::process::id(),
        new_instance_id()
    ));
    let config = HostConfig {
        data_root: data_root.clone(),
        safe_mode: true,
        binaries: RuntimeBinaries::new(
            data_root.join("riffra-audio"),
            data_root.join("riffra-plugin-scan"),
            data_root.join("riffra-render"),
        ),
    };
    let host = DawHost::open(config.clone(), Arc::new(crate::NoopHostEventSink)).unwrap();
    let descriptor = read_endpoint(&data_root).unwrap();
    let mut stream = transport::connect(descriptor.endpoint()).unwrap();
    transport::write_frame(&mut stream, &HelloRequest::new()).unwrap();
    let _: HelloResponse = transport::read_frame(&mut stream).unwrap();

    transport::write_frame(
        &mut stream,
        &ControlRequest::new(
            "shutdown-request",
            ControlCommand::new("host.shutdown", serde_json::json!({})),
            Some(0),
        ),
    )
    .unwrap();
    let shutdown_response: ControlResponse = transport::read_frame(&mut stream).unwrap();
    assert!(shutdown_response.ok);
    transport::write_frame(
        &mut stream,
        &ControlRequest::new(
            "after-shutdown",
            ControlCommand::new(
                "track.add",
                serde_json::json!({"name": "Rejected", "kind": "audio"}),
            ),
            Some(0),
        ),
    )
    .unwrap();
    let response: ControlResponse = transport::read_frame(&mut stream).unwrap();
    assert!(!response.ok);
    assert_eq!(
        response.error.as_ref().map(|error| error.code),
        Some(ErrorCode::HostUnavailable)
    );
    drop(stream);
    drop(host);

    let reopened = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
    assert_eq!(reopened.canonical_state().unwrap().sequence, 0);
    reopened.shutdown();
    drop(reopened);
    let _ = std::fs::remove_dir_all(data_root);
}

#[test]
fn shutdown_waits_for_inflight_host_operations() {
    let data_root = std::env::temp_dir().join(format!(
        "riffra-runtime-shutdown-gate-{}-{}",
        std::process::id(),
        new_instance_id()
    ));
    let config = HostConfig {
        data_root: data_root.clone(),
        safe_mode: true,
        binaries: RuntimeBinaries::new(
            data_root.join("riffra-audio"),
            data_root.join("riffra-plugin-scan"),
            data_root.join("riffra-render"),
        ),
    };
    let host = Arc::new(DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap());
    let inflight = host
        .state
        .lifecycle_gate
        .read()
        .expect("Host lifecycle gate was not poisoned");
    let (finished_tx, finished_rx) = std::sync::mpsc::channel();
    let shutdown_host = Arc::clone(&host);
    let shutdown_thread = std::thread::spawn(move || {
        shutdown_host.shutdown();
        finished_tx.send(()).unwrap();
    });

    assert!(
        finished_rx
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err()
    );
    drop(inflight);
    assert!(
        finished_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .is_ok()
    );
    shutdown_thread.join().unwrap();
    drop(host);
    let _ = std::fs::remove_dir_all(data_root);
}

#[test]
fn normal_host_returns_arrangement_mutation_before_shutdown() {
    let data_root = std::env::temp_dir().join(format!(
        "riffra-runtime-startup-shutdown-{}-{}",
        std::process::id(),
        new_instance_id()
    ));
    let config = HostConfig {
        data_root: data_root.clone(),
        safe_mode: false,
        binaries: RuntimeBinaries::new(
            data_root.join("missing-riffra-audio"),
            data_root.join("missing-riffra-plugin-scan"),
            data_root.join("missing-riffra-render"),
        ),
    };
    let host = DawHost::open(config.clone(), Arc::new(crate::NoopHostEventSink)).unwrap();

    let response = host.dispatch_control(ControlRequest::new(
        "track-add",
        ControlCommand::new(
            "track.add",
            serde_json::json!({"name": "Synth", "kind": "instrument"}),
        ),
        Some(0),
    ));

    assert!(response.ok);
    assert_eq!(
        response
            .result
            .as_ref()
            .map(|result| result.result_type.as_str()),
        Some("arrangementMutation")
    );
    let mutation: crate::model::ArrangementMutationResult =
        serde_json::from_value(response.result.unwrap().value).unwrap();
    assert_eq!(mutation.canonical.sequence, 1);
    assert!(matches!(
        mutation.projection,
        crate::model::ArrangementProjectionOutcome::Queued
            | crate::model::ArrangementProjectionOutcome::Failed { .. }
    ));

    let marker = host.dispatch_control(ControlRequest::new(
        "marker-add",
        ControlCommand::new(
            "marker.add",
            serde_json::json!({"name": "Verse", "tick": 0}),
        ),
        Some(1),
    ));
    assert!(marker.ok);
    assert_eq!(
        marker
            .result
            .as_ref()
            .map(|result| result.result_type.as_str()),
        Some("arrangementMutation")
    );
    let marker: crate::model::ArrangementMutationResult =
        serde_json::from_value(marker.result.unwrap().value).unwrap();
    assert_eq!(marker.canonical.sequence, 2);
    assert!(matches!(
        marker.projection,
        crate::model::ArrangementProjectionOutcome::NotRequired
    ));

    let settings = host.dispatch_control(ControlRequest::new(
        "session-settings-update",
        ControlCommand::new(
            "session.settings.update",
            serde_json::json!({"note": "authoring note"}),
        ),
        Some(2),
    ));
    assert!(settings.ok);
    assert_eq!(
        settings
            .result
            .as_ref()
            .map(|result| result.result_type.as_str()),
        Some("arrangementMutation")
    );
    let settings: crate::model::ArrangementMutationResult =
        serde_json::from_value(settings.result.unwrap().value).unwrap();
    assert_eq!(settings.canonical.sequence, 3);
    assert!(matches!(
        settings.projection,
        crate::model::ArrangementProjectionOutcome::NotRequired
    ));

    host.shutdown();
    drop(host);

    let reopened = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
    reopened.shutdown();
    drop(reopened);
    let _ = std::fs::remove_dir_all(data_root);
}

#[test]
fn normal_host_publishes_canonical_state_before_projection_status() {
    let data_root = std::env::temp_dir().join(format!(
        "riffra-runtime-event-order-{}-{}",
        std::process::id(),
        new_instance_id()
    ));
    let config = HostConfig {
        data_root: data_root.clone(),
        safe_mode: false,
        binaries: RuntimeBinaries::new(
            data_root.join("missing-riffra-audio"),
            data_root.join("missing-riffra-plugin-scan"),
            data_root.join("missing-riffra-render"),
        ),
    };
    let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();
    let events = host
        .state
        .subscribe_events()
        .expect("Host event subscription should be available");

    let response = host.dispatch_control(ControlRequest::new(
        "track-add",
        ControlCommand::new(
            "track.add",
            serde_json::json!({"name": "Synth", "kind": "instrument"}),
        ),
        Some(0),
    ));
    assert!(response.ok);

    let mut canonical_index = None;
    let mut projection_index = None;
    for index in 0..16 {
        let event = events
            .recv_timeout(std::time::Duration::from_secs(1))
            .expect("Host should publish the mutation events");
        if event.event == "canonical-state-changed" && event.payload["sequence"].as_u64() == Some(1)
        {
            canonical_index = Some(index);
        }
        if event.event == "runtime-projection-status"
            && event.payload["targetProjectionSequence"].as_u64() == Some(1)
        {
            projection_index = Some(index);
            break;
        }
    }

    assert!(
        canonical_index.is_some_and(|canonical| {
            projection_index.is_some_and(|projection| canonical < projection)
        }),
        "canonical state must be published before projection status"
    );

    host.shutdown();
    drop(host);
    let _ = std::fs::remove_dir_all(data_root);
}

#[test]
fn lifecycle_operations_are_rejected_after_shutdown() {
    let data_root = std::env::temp_dir().join(format!(
        "riffra-runtime-lifecycle-{}-{}",
        std::process::id(),
        new_instance_id()
    ));
    let config = HostConfig {
        data_root: data_root.clone(),
        safe_mode: true,
        binaries: RuntimeBinaries::new(
            data_root.join("riffra-audio"),
            data_root.join("riffra-plugin-scan"),
            data_root.join("riffra-render"),
        ),
    };
    let host = DawHost::open(config, Arc::new(crate::NoopHostEventSink)).unwrap();

    assert_eq!(host.with_lifecycle(|| Ok::<_, String>(7)), Ok(7));
    host.shutdown();
    assert_eq!(
        host.with_lifecycle(|| Ok::<_, String>(7)),
        Err("Riffra Host has shut down".to_owned())
    );

    drop(host);
    let _ = std::fs::remove_dir_all(data_root);
}
