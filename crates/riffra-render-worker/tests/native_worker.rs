use riffra_core::{AudioRuntime, OfflineRenderRequest};
use riffra_render_worker::RenderWorker;

#[test]
#[ignore = "requires a built riffra-render executable"]
fn renders_wave_without_an_audio_device() {
    // Arrange
    let executable =
        std::env::var_os("RIFFRA_RENDER_WORKER").expect("RIFFRA_RENDER_WORKER must be set");
    let destination = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("render-{}.wav", std::process::id()));
    let _ = std::fs::remove_file(&destination);
    let worker = RenderWorker::new(executable.into());
    let request = OfflineRenderRequest {
        snapshot: serde_json::json!({
            "revision": 1,
            "timebase": {
                "ppq": 960,
                "bpm": 120.0,
                "timeSignatureNumerator": 4,
                "timeSignatureDenominator": 4
            },
            "loopRange": {
                "enabled": false,
                "startTick": 0,
                "endTick": 0
            },
            "tracks": []
        }),
        destination: destination.clone(),
        start_tick: 0,
        end_tick: 960,
        sample_rate: 48_000,
        block_size: 512,
        master_gain_db: -18.0,
        normalize: false,
    };

    // Act
    worker
        .render_timeline_offline(request)
        .expect("offline render should succeed");

    // Assert
    let wave = std::fs::read(&destination).expect("rendered WAV");
    assert!(wave.len() > 44);
    assert_eq!(&wave[..4], b"RIFF");
    assert_eq!(&wave[8..12], b"WAVE");
    std::fs::remove_file(destination).expect("rendered WAV cleanup");
}
