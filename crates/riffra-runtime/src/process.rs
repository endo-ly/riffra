//! Construction of sidecar [`Command`]s shared by every runtime spawn site.

use std::ffi::OsStr;
use std::process::Command;

/// Starts building a [`Command`] for a bundled sidecar process.
///
/// Windows gives every console-subsystem child of a GUI-subsystem parent its
/// own console window, and piping the standard streams does not suppress it.
/// All sidecar stdio is piped or null, so detaching them from any console
/// changes no observable behavior.
pub(crate) fn sidecar_command(program: impl AsRef<OsStr>) -> Command {
    #[cfg_attr(not(windows), allow(unused_mut))]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}
