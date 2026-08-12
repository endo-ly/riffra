/// Absolute ordering assigned by Core when canonical state is
/// committed. `session_revision` is retained for diagnostics and display, but
/// it is not an ordering key because restore/import may legitimately move it
/// backwards.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProjectionKey {
    pub sequence: u64,
    pub session_revision: u64,
}
