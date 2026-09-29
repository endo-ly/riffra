//! The persisted form of a [`CreativeSession`].
//!
//! A session document is `{"schemaVersion": 1, "session": {...}}`. The version
//! is read before the session, so a document written in another format is
//! reported as such instead of as a corrupt session.

use super::CreativeSession;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The session document schema this build reads and writes.
pub const SESSION_SCHEMA_VERSION: u32 = 1;

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

/// Decodes a versioned session document.
///
/// # Errors
///
/// Returns [`SessionDocumentError::UnsupportedSchemaVersion`] when the version
/// is missing or differs from [`SESSION_SCHEMA_VERSION`], and
/// [`SessionDocumentError::Invalid`] when the document is not valid JSON or
/// the session does not match the schema.
pub fn deserialize_session_document(
    payload: &[u8],
) -> Result<CreativeSession, SessionDocumentError> {
    let version = serde_json::from_slice::<SessionDocumentVersion>(payload)?.schema_version;
    if version != Some(SESSION_SCHEMA_VERSION) {
        return Err(SessionDocumentError::UnsupportedSchemaVersion {
            found: version,
            expected: SESSION_SCHEMA_VERSION,
        });
    }
    Ok(serde_json::from_slice::<OwnedSessionDocument>(payload)?.session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_session_round_trips_through_its_document() {
        // Arrange
        let session = CreativeSession::new(1_000);

        // Act
        let payload = serialize_session_document(&session).unwrap();

        // Assert
        let value: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(value["schemaVersion"], SESSION_SCHEMA_VERSION);
        assert_eq!(deserialize_session_document(&payload).unwrap(), session);
    }

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
            let document = serde_json::json!({"schemaVersion": 1, "session": session});

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
                serde_json::json!({"schemaVersion": 2, "session": session}),
                Some(2),
                "2",
            ),
        ];

        for (document, found, shown) in cases {
            // Act
            let error =
                deserialize_session_document(&serde_json::to_vec(&document).unwrap()).unwrap_err();

            // Assert
            assert!(matches!(
                error,
                SessionDocumentError::UnsupportedSchemaVersion { found: actual, expected: 1 }
                    if actual == found
            ));
            assert_eq!(
                error.to_string(),
                format!("unsupported session schema version: found {shown}, expected 1")
            );
        }
    }
}
