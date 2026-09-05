use crate::args::ServeArgs;
use crate::resources;
use riffra_control::read_endpoint;
use riffra_runtime::{DawHost, HostConfig, NoopHostEventSink};
use signal_hook::consts::SIGINT;
#[cfg(unix)]
use signal_hook::consts::SIGTERM;
use signal_hook::flag;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Runs the foreground live Host until the process receives a termination
/// signal.
pub fn run(data_root: PathBuf, args: ServeArgs) -> Result<(), String> {
    let mut config = HostConfig::new(data_root.clone(), resources::built_in_instruments_root()?)
        .map_err(|error| format!("serve configuration could not be created: {error}"))?;
    config.safe_mode = args.safe_mode;
    let host =
        DawHost::open(config, Arc::new(NoopHostEventSink)).map_err(|error| error.to_string())?;
    let descriptor = read_endpoint(&data_root)?;
    eprintln!(
        "riffra serve ready: {} (instance {})",
        riffra_control::endpoint_path(&data_root).display(),
        descriptor.instance_id
    );

    let stopped = Arc::new(AtomicBool::new(false));
    flag::register(SIGINT, Arc::clone(&stopped))
        .map_err(|error| format!("SIGINT handler could not be installed: {error}"))?;
    #[cfg(unix)]
    flag::register(SIGTERM, Arc::clone(&stopped))
        .map_err(|error| format!("SIGTERM handler could not be installed: {error}"))?;
    while !stopped.load(Ordering::Acquire) && !host.shutdown_requested() {
        std::thread::sleep(Duration::from_millis(100));
    }
    host.shutdown();
    Ok(())
}
