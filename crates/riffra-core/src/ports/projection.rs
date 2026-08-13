/// Absolute ordering assigned by Core when canonical state is
/// committed. `session_revision` is retained for diagnostics and display, but
/// it is not an ordering key because restore/import may legitimately move it
/// backwards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionKey {
    /// Canonical commit sequence assigned by Core.
    pub sequence: u64,
    /// Arrangement revision included for runtime diagnostics.
    pub session_revision: u64,
}
