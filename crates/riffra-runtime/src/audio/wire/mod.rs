//! Typed JSON Lines protocol between the Host and the native sidecars.
//!
//! Every line written to `riffra-audio` is an [`encode_command`] envelope and
//! every line read from it decodes into one [`SidecarMessage`]. Decoding is
//! strict: unknown keys, missing keys, and unknown message types are errors.

mod commands;
#[cfg(test)]
mod fixtures;
mod messages;

pub(crate) use commands::{
    ExpectedResponse, OfflineRenderEnvelope, SidecarCommand, TakeComparisonVariant,
};
pub(crate) use messages::{
    PluginScanMessage, PluginScanMetadata, ProbeMessage, RenderMessage, SidecarError, SidecarEvent,
    SidecarMessage, SidecarResponse, WireAudioChannel, WireAudioMeters, WireAudioState,
    WireAudioStatus, WireInstrumentFault, WireMidiDevice, WirePluginState, WireRecordingComplete,
    WireRecordingPhase, WireRecoveryStatus, WireTrackPluginParameterChanged,
    WireTrackPluginStateChanged, WireTransportState, WireTransportStatus,
};

use serde::{Deserialize, Deserializer, Serialize};

/// Protocol version announced by `ready` and required by every sidecar request.
pub(crate) const SIDECAR_PROTOCOL_VERSION: u32 = 4;

/// Maximum number of line bytes copied into protocol diagnostics.
const DIAGNOSTIC_LINE_LIMIT: usize = 256;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandEnvelope<'a> {
    request_id: u64,
    command: &'a SidecarCommand,
}

/// Encodes one command line for the sidecar's standard input.
///
/// # Errors
/// Returns the serializer error when a command value cannot form JSON.
pub(crate) fn encode_command(
    request_id: u64,
    command: &SidecarCommand,
) -> Result<String, serde_json::Error> {
    serde_json::to_string(&CommandEnvelope {
        request_id,
        command,
    })
}

/// A sidecar line that did not match the message contract.
#[derive(Debug)]
pub(crate) struct UndecodableLine {
    /// Request named by the line, when its envelope still exposes one.
    pub(crate) request_id: Option<u64>,
    pub(crate) error: serde_json::Error,
}

/// Decodes one sidecar output line.
///
/// # Errors
/// Returns the decoding failure together with the request id readable from the
/// raw line so the caller can fail that request immediately.
pub(crate) fn decode_message(line: &[u8]) -> Result<SidecarMessage, UndecodableLine> {
    serde_json::from_slice(line).map_err(|error| UndecodableLine {
        request_id: serde_json::from_slice::<serde_json::Value>(line)
            .ok()
            .and_then(|value| value.get("requestId")?.as_u64()),
        error,
    })
}

/// Returns the bounded prefix of a line used in protocol diagnostics.
pub(crate) fn diagnostic_prefix(line: &[u8]) -> String {
    let end = line.len().min(DIAGNOSTIC_LINE_LIMIT);
    String::from_utf8_lossy(&line[..end]).into_owned()
}

/// Requires a key to be present while allowing its value to be `null`.
///
/// serde accepts a missing `Option` field as `None` by default; routing the
/// field through this function makes the key mandatory.
pub(crate) fn nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(deserializer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_undecodable_line_keeps_its_request_id() {
        let failure = decode_message(br#"{"kind":"response","requestId":7,"response":{}}"#)
            .expect_err("a response without a type must be rejected");

        assert_eq!(failure.request_id, Some(7));
    }

    #[test]
    fn unknown_message_types_are_errors() {
        let failure = decode_message(br#"{"kind":"event","event":{"type":"keepAlive"}}"#)
            .expect_err("an unknown event must be rejected");

        assert_eq!(failure.request_id, None);
    }

    #[test]
    fn diagnostic_prefix_is_bounded() {
        let line = vec![b'x'; DIAGNOSTIC_LINE_LIMIT * 2];

        assert_eq!(diagnostic_prefix(&line).len(), DIAGNOSTIC_LINE_LIMIT);
    }
}
