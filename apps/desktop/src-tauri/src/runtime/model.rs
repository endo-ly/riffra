/// Absolute ordering assigned by the Session Actor when canonical state is
/// committed. `session_revision` is retained for diagnostics and display, but
/// it is not an ordering key because restore/import may legitimately move it
/// backwards.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct ProjectionKey {
    pub sequence: u64,
    pub session_revision: u64,
}
