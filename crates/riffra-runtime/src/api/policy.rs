//! Execution properties declared for every Control Command.

/// How a Control Command is gated and which executor runs it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommandPolicy {
    pub scope: CommandScope,
    pub executor: CommandExecutor,
}

/// Whether a command is bound to the active Project.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandScope {
    /// Does not require `expectedProjectId`.
    Host,
    /// Belongs to the active Project and requires `expectedProjectId`.
    ///
    /// `long_running` commands only guard the Project commit instead of
    /// holding the Host command gate for their whole duration.
    Project { long_running: bool },
}

/// The executor that owns a command's implementation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CommandExecutor {
    /// Reads or edits canonical state; runs in Standalone and live Hosts.
    Canonical { access: CanonicalAccess },
    /// Selects, creates, imports, or exports Project containers.
    Project,
    /// Requires the runtime services of a live Host.
    Runtime,
}

/// Whether a canonical command can change canonical state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CanonicalAccess {
    Read,
    Mutation { batchable: bool },
}
