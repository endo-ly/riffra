//! The persisted form of a [`CreativeSession`].
//!
//! A session document is `{"schemaVersion": 2, "session": {...}}`. The version
//! is read before the session, so a document written in another format is
//! reported as such instead of as a corrupt session.

use super::CreativeSession;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The current session document schema written by this build.
pub const SESSION_SCHEMA_VERSION: u32 = 2;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionDocument<'a> {
    schema_version: u32,
    session: &'a CreativeSession,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionDocumentVersion {
    schema_version: Option<u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct OwnedSessionDocument {
    #[serde(rename = "schemaVersion")]
    _schema_version: u32,
    session: CreativeSession,
}

/// A session document that cannot be read by this build.
#[derive(Debug, Error)]
pub enum SessionDocumentError {
    #[error(
        "unsupported session schema version: found {}, expected {expected}",
        found.map_or_else(|| "none".to_owned(), |version| version.to_string())
    )]
    UnsupportedSchemaVersion { found: Option<u32>, expected: u32 },
    #[error("invalid session document: {0}")]
    Invalid(#[from] serde_json::Error),
}

/// Encodes a session as a versioned session document.
///
/// # Errors
///
/// Returns a JSON error when the session cannot be encoded.
pub fn serialize_session_document(session: &CreativeSession) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec_pretty(&SessionDocument {
        schema_version: SESSION_SCHEMA_VERSION,
        session,
    })
}

/// Decodes a session document in the current schema.
///
/// # Errors
///
/// Returns [`SessionDocumentError::UnsupportedSchemaVersion`] when the version
/// is missing or unsupported, and
/// [`SessionDocumentError::Invalid`] when the document is not valid JSON or
/// the session does not match the schema.
pub fn deserialize_session_document(
    payload: &[u8],
) -> Result<CreativeSession, SessionDocumentError> {
    let version = serde_json::from_slice::<SessionDocumentVersion>(payload)?.schema_version;
    match version {
        Some(SESSION_SCHEMA_VERSION) => {
            Ok(serde_json::from_slice::<OwnedSessionDocument>(payload)?.session)
        }
        _ => Err(SessionDocumentError::UnsupportedSchemaVersion {
            found: version,
            expected: SESSION_SCHEMA_VERSION,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_missing_session_keys_are_invalid() {
        // Arrange
        let session = serde_json::to_value(CreativeSession::new(1_000)).unwrap();
        let mut unknown = session.clone();
        unknown["settings"]["loopEnabld"] = true.into();
        let mut missing_setting = session.clone();
        missing_setting["settings"]
            .as_object_mut()
            .unwrap()
            .remove("loopEnabled");
        let mut missing_arrangement = session;
        missing_arrangement
            .as_object_mut()
            .unwrap()
            .remove("arrangement");
        let cases = [
            (unknown, "unknown field `loopEnabld`"),
            (missing_setting, "missing field `loopEnabled`"),
            (missing_arrangement, "missing field `arrangement`"),
        ];

        for (session, message) in cases {
            let document =
                serde_json::json!({"schemaVersion": SESSION_SCHEMA_VERSION, "session": session});

            // Act
            let error =
                deserialize_session_document(&serde_json::to_vec(&document).unwrap()).unwrap_err();

            // Assert
            assert!(
                matches!(&error, SessionDocumentError::Invalid(_))
                    && error.to_string().contains(message),
                "{error}"
            );
        }
    }

    #[test]
    fn a_missing_or_different_version_is_unsupported() {
        // Arrange
        let session = serde_json::to_value(CreativeSession::new(1_000)).unwrap();
        let cases = [
            (serde_json::json!(session), None, "none"),
            (
                serde_json::json!({"schemaVersion": 1, "session": session}),
                Some(1),
                "1",
            ),
            (
                serde_json::json!({"schemaVersion": 3, "session": session}),
                Some(3),
                "3",
            ),
        ];

        for (document, found, shown) in cases {
            // Act
            let error =
                deserialize_session_document(&serde_json::to_vec(&document).unwrap()).unwrap_err();

            // Assert
            assert!(matches!(
                error,
                SessionDocumentError::UnsupportedSchemaVersion { found: actual, expected: 2 }
                    if actual == found
            ));
            assert_eq!(
                error.to_string(),
                format!("unsupported session schema version: found {shown}, expected 2")
            );
        }
    }

    #[test]
    fn serializes_summary_metadata_before_large_session_fields() {
        // Arrange
        let mut session = CreativeSession::new(1_000);
        session.project_name = Some("Project".into());
        session.score_source = Some(super::super::ScoreSource {
            file_name: "score.yaml".into(),
            yaml: "score".into(),
        });

        // Act
        let payload = serialize_session_document(&session).unwrap();
        let serialized = std::str::from_utf8(&payload).unwrap();
        let loaded = deserialize_session_document(&payload).unwrap();

        // Assert
        let field_positions = [
            "\"sessionId\"",
            "\"updatedAtMs\"",
            "\"projectName\"",
            "\"scoreSource\"",
            "\"arrangement\"",
        ]
        .map(|field| serialized.find(field).unwrap());
        assert!(field_positions.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(loaded, session);
    }
}
