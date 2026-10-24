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
struct OwnedSessionDocument<T> {
    #[serde(rename = "schemaVersion")]
    _schema_version: u32,
    session: T,
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

/// Decodes a versioned session document, migrating older persisted state.
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
            Ok(serde_json::from_slice::<OwnedSessionDocument<CreativeSession>>(payload)?.session)
        }
        Some(1) => {
            let document: OwnedSessionDocument<serde_json::Value> =
                serde_json::from_slice(payload)?;
            Ok(serde_json::from_value(migrate_v1(document.session)?)?)
        }
        _ => Err(SessionDocumentError::UnsupportedSchemaVersion {
            found: version,
            expected: SESSION_SCHEMA_VERSION,
        }),
    }
}

fn migrate_v1(mut session: serde_json::Value) -> Result<serde_json::Value, serde_json::Error> {
    if let Some(tracks) = session
        .get_mut("arrangement")
        .and_then(|value| value.get_mut("tracks"))
        .and_then(serde_json::Value::as_array_mut)
    {
        for track in tracks {
            let Some(fields) = track.as_object_mut() else {
                continue;
            };
            let Some(source) = fields.remove("externalAudioSourceTrackId") else {
                continue;
            };
            let sources = match source {
                serde_json::Value::Null => Vec::new(),
                serde_json::Value::String(source) => vec![serde_json::Value::String(source)],
                _ => {
                    return Err(serde::de::Error::custom(
                        "invalid external audio source track id",
                    ));
                }
            };
            if fields
                .insert("externalAudioSourceTrackIds".into(), sources.into())
                .is_some()
            {
                return Err(serde::de::Error::custom(
                    "conflicting external audio source fields",
                ));
            }
        }
    }
    Ok(session)
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
    fn migrates_single_source_routes_and_writes_the_current_schema() {
        // Arrange
        let mut session = CreativeSession::new(1_000);
        session
            .arrangement
            .tracks
            .push(crate::Track::audio("source".into(), "Source".into()));
        let mut consumer = crate::Track::instrument("consumer".into(), "Consumer".into());
        consumer.external_audio_source_track_ids = vec!["source".into()];
        session.arrangement.tracks.push(consumer);
        let mut legacy = serde_json::to_value(&session).unwrap();
        for (index, track) in legacy["arrangement"]["tracks"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .enumerate()
        {
            let fields = track.as_object_mut().unwrap();
            fields.remove("externalAudioSourceTrackIds");
            fields.insert(
                "externalAudioSourceTrackId".into(),
                if index == 0 {
                    serde_json::Value::Null
                } else {
                    "source".into()
                },
            );
        }
        let document = serde_json::json!({"schemaVersion":1,"session":legacy});

        // Act
        let loaded = deserialize_session_document(&serde_json::to_vec(&document).unwrap()).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&serialize_session_document(&loaded).unwrap()).unwrap();

        // Assert
        assert_eq!(loaded, session);
        assert_eq!(saved["schemaVersion"], SESSION_SCHEMA_VERSION);
        assert_eq!(
            deserialize_session_document(&serde_json::to_vec(&saved).unwrap()).unwrap(),
            session
        );
        let mut conflicting = document.clone();
        conflicting["session"]["arrangement"]["tracks"][1]["externalAudioSourceTrackIds"] =
            serde_json::json!([]);
        assert!(deserialize_session_document(&serde_json::to_vec(&conflicting).unwrap()).is_err());
        for invalid in [
            serde_json::json!(0),
            serde_json::json!({"arrangement":false}),
        ] {
            assert!(
                deserialize_session_document(
                    &serde_json::to_vec(&serde_json::json!({"schemaVersion":1,"session":invalid}))
                        .unwrap()
                )
                .is_err()
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
