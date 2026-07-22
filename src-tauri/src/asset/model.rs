//! Asset domain model.
//!
//! An `Asset` is the canonical production material used across Riffra. It owns
//! a stable identity ([`AssetId`]), a production kind, the location of its
//! content file, and the [`Provenance`] that describes how it was produced.
//!
//! Invariants enforced by this module:
//! * each produced `Asset` receives a fresh, globally-unique [`AssetId`];
//! * production content is immutable — changing content mints a new `Asset`
//!   instead of mutating an existing one;
//! * only management metadata (name, tag, note) may change for a kept id.

use crate::errors::DomainError;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

/// Stable, globally-unique identifier for an `Asset`.
///
/// The only valid string form is `asset:<UUIDv7>`.
#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, TS)]
#[serde(transparent)]
#[ts(type = "string & { readonly __brand: 'AssetId' }")]
pub struct AssetId(String);

impl AssetId {
    /// Creates an id from a known value. Prefer [`AssetId::mint`] for new ids.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the value is not exactly an `asset:` prefix
    /// followed by a version-7 UUID. Any other form (including the legacy
    /// `asset:<millis>-<counter>` scheme and arbitrary strings) is rejected so
    /// that only the current UUIDv7 spec is ever valid in the domain.
    pub fn from_normalized(value: impl Into<String>) -> Result<Self, DomainError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(DomainError::InvalidAssetId(
                "Asset id must not be empty.".into(),
            ));
        }
        let Some(raw) = value.strip_prefix("asset:") else {
            return Err(DomainError::InvalidAssetId(format!(
                "Asset id '{value}' is missing the canonical asset: prefix."
            )));
        };
        let uuid = Uuid::parse_str(raw).map_err(|error| {
            DomainError::InvalidAssetId(format!("Asset id '{value}' is not a valid UUID: {error}"))
        })?;
        if uuid.get_version_num() != 7 {
            return Err(DomainError::InvalidAssetId(format!(
                "Asset id '{value}' must be a UUIDv7, found version {}.",
                uuid.get_version_num()
            )));
        }
        Ok(Self(value))
    }

    /// The underlying string value.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AssetId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// Kinds of production material an [`Asset`] can represent.
///
/// The set is intentionally open-ended; new variants are added here as the
/// production vocabulary grows.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssetKind {
    Audio,
    Midi,
    Sample,
    RackDefinition,
    GenerationDefinition,
}

/// The operation that produced an [`Asset`], recorded by [`Provenance`].
///
/// `Imported` covers assets brought in from outside Riffra (for example a
/// reference WAV or a clip from a migrated v1 session) whose origin cannot be
/// expressed by the in-process operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProvenanceOperation {
    Recorded,
    Processed,
    Sampled,
    Separated,
    Rendered,
    Generated,
    Imported,
}

/// How an [`Asset`] was produced.
///
/// `source_asset_ids` lists the assets consumed to produce this one. A single
/// source produces `Sampled`/`Processed`/`Separated` results; a render can
/// consume many. `Imported` and root `Recorded` assets carry an empty list.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Provenance {
    #[serde(default)]
    pub source_asset_ids: Vec<AssetId>,
    pub operation: ProvenanceOperation,
    #[serde(default)]
    pub parameters: serde_json::Map<String, serde_json::Value>,
}

impl Provenance {
    /// Provenance for a freshly imported asset with no in-process source.
    pub fn imported() -> Self {
        Self {
            source_asset_ids: Vec::new(),
            operation: ProvenanceOperation::Imported,
            parameters: serde_json::Map::new(),
        }
    }

    /// Provenance for a root recording with no source asset.
    pub fn recorded_root() -> Self {
        Self {
            source_asset_ids: Vec::new(),
            operation: ProvenanceOperation::Recorded,
            parameters: serde_json::Map::new(),
        }
    }

    /// The asset ids this asset was produced from.
    pub fn source_asset_ids(&self) -> &[AssetId] {
        &self.source_asset_ids
    }
}

/// Canonical production material.
///
/// Construct through [`Asset::register`] (new id) or [`Asset::derive`] (new id
/// from sources). Mutate only management metadata via [`Asset::update_metadata`].
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Asset {
    pub id: AssetId,
    pub kind: AssetKind,
    pub name: String,
    /// Canonical location of the content file on disk.
    pub content_location: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<Provenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub favorite: bool,
}

/// Mint a new globally-unique [`AssetId`] using UUIDv7.
///
/// UUIDv7 carries creation-time ordering while its random component provides
/// uniqueness across processes and machines. The `asset:` namespace is kept
/// stable so serialized sessions and project manifests remain self-describing.
pub fn mint_asset_id() -> AssetId {
    AssetId(format!("asset:{}", Uuid::now_v7()))
}

impl Asset {
    /// Registers a brand-new asset, minting a fresh id.
    #[allow(clippy::too_many_arguments)]
    pub fn register(
        kind: AssetKind,
        name: impl Into<String>,
        content_location: impl Into<String>,
        provenance: Option<Provenance>,
        now_ms: u64,
    ) -> Self {
        Self {
            id: mint_asset_id(),
            kind,
            name: name.into(),
            content_location: content_location.into(),
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            provenance,
            tag: None,
            note: None,
            favorite: false,
        }
    }

    /// Produces a new asset derived from `sources`, minting a fresh id and
    /// recording the supplied provenance. The source assets are unchanged.
    ///
    /// # Errors
    /// Returns [`DomainError`] when the operation implies sources but none are
    /// supplied, since an operation such as `Sampled` without a source is a
    /// modelling error rather than missing data.
    pub fn derive(
        sources: &[&Asset],
        kind: AssetKind,
        name: impl Into<String>,
        content_location: impl Into<String>,
        operation: ProvenanceOperation,
        parameters: serde_json::Map<String, serde_json::Value>,
        now_ms: u64,
    ) -> Result<Self, DomainError> {
        let requires_source = matches!(
            operation,
            ProvenanceOperation::Processed
                | ProvenanceOperation::Sampled
                | ProvenanceOperation::Separated
                | ProvenanceOperation::Rendered
                | ProvenanceOperation::Generated
        );
        if requires_source && sources.is_empty() {
            return Err(DomainError::InvalidProvenance(format!(
                "Operation {operation:?} requires at least one source asset."
            )));
        }
        let provenance = Provenance {
            source_asset_ids: sources.iter().map(|asset| asset.id.clone()).collect(),
            operation,
            parameters,
        };
        Ok(Self::register(
            kind,
            name,
            content_location,
            Some(provenance),
            now_ms,
        ))
    }

    /// Updates management metadata only. The id, kind, content, and provenance
    /// are preserved, honouring the immutability of production content.
    pub fn update_metadata(
        mut self,
        name: Option<String>,
        tag: Option<Option<String>>,
        note: Option<Option<String>>,
        favorite: Option<bool>,
        now_ms: u64,
    ) -> Self {
        if let Some(name) = name {
            self.name = name;
        }
        if let Some(tag) = tag {
            self.tag = tag;
        }
        if let Some(note) = note {
            self.note = note;
        }
        if let Some(favorite) = favorite {
            self.favorite = favorite;
        }
        self.updated_at_ms = now_ms;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newly_registered_asset_receives_a_fresh_unique_id() {
        let a = Asset::register(
            AssetKind::Audio,
            "take-1",
            "C:\\data\\take-1.wav",
            Some(Provenance::recorded_root()),
            1_000,
        );
        let b = Asset::register(
            AssetKind::Audio,
            "take-2",
            "C:\\data\\take-2.wav",
            Some(Provenance::recorded_root()),
            1_000,
        );
        assert_ne!(a.id, b.id);
        assert!(a.id.as_str().starts_with("asset:"));
        let uuid = Uuid::parse_str(a.id.as_str().strip_prefix("asset:").unwrap()).unwrap();
        assert_eq!(uuid.get_version_num(), 7);
    }

    #[test]
    fn deriving_new_content_mints_a_new_id_instead_of_reusing() {
        let source = Asset::register(
            AssetKind::Audio,
            "raw",
            "C:\\data\\raw.wav",
            Some(Provenance::recorded_root()),
            1_000,
        );
        let processed = Asset::derive(
            &[&source],
            AssetKind::Audio,
            "processed",
            "C:\\data\\processed.wav",
            ProvenanceOperation::Processed,
            serde_json::Map::new(),
            2_000,
        )
        .unwrap();
        assert_ne!(source.id, processed.id);
        let sources = processed.provenance.as_ref().unwrap().source_asset_ids();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0], source.id);
    }

    #[test]
    fn metadata_update_preserves_asset_id() {
        let asset = Asset::register(AssetKind::Audio, "take", "C:\\data\\take.wav", None, 1_000);
        let updated = asset.update_metadata(
            Some("renamed".into()),
            Some(Some("idea".into())),
            Some(Some("note".into())),
            Some(true),
            2_000,
        );
        assert!(updated.id.as_str().starts_with("asset:"));
        assert_eq!(updated.name, "renamed");
        assert_eq!(updated.tag.as_deref(), Some("idea"));
        assert_eq!(updated.note.as_deref(), Some("note"));
        assert!(updated.favorite);
        assert_eq!(updated.updated_at_ms, 2_000);
    }

    #[test]
    fn provenance_exposes_source_asset_ids() {
        let a = Asset::register(
            AssetKind::Audio,
            "a",
            "C:\\a.wav",
            Some(Provenance::recorded_root()),
            1_000,
        );
        let b = Asset::register(
            AssetKind::Audio,
            "b",
            "C:\\b.wav",
            Some(Provenance::recorded_root()),
            1_000,
        );
        let render = Asset::derive(
            &[&a, &b],
            AssetKind::Audio,
            "render",
            "C:\\render.wav",
            ProvenanceOperation::Rendered,
            serde_json::Map::new(),
            3_000,
        )
        .unwrap();
        let sources = render.provenance.as_ref().unwrap().source_asset_ids();
        assert!(sources.contains(&a.id));
        assert!(sources.contains(&b.id));
    }

    #[test]
    fn source_required_operations_reject_empty_sources() {
        let error = Asset::derive(
            &[],
            AssetKind::Audio,
            "orphan",
            "C:\\orphan.wav",
            ProvenanceOperation::Sampled,
            serde_json::Map::new(),
            1_000,
        )
        .unwrap_err();
        assert!(matches!(error, DomainError::InvalidProvenance(_)));
    }

    #[test]
    fn asset_id_accepts_only_uuidv7() {
        assert!(AssetId::from_normalized("").is_err());
        assert!(AssetId::from_normalized("C:\\path.wav").is_err());
        assert!(AssetId::from_normalized("asset:1-0").is_err());
        assert!(AssetId::from_normalized("asset:not-a-uuid").is_err());
        // A valid UUID but not version 7 must be rejected.
        assert!(AssetId::from_normalized("asset:01890aef-3d2c-4b4e-8f1a-2b3c4d5e6f70").is_err());
        assert!(AssetId::from_normalized(format!("asset:{}", Uuid::now_v7())).is_ok());
    }

    #[test]
    fn imported_provenance_has_no_source_and_is_imported() {
        let provenance = Provenance::imported();
        assert!(provenance.source_asset_ids.is_empty());
        assert_eq!(provenance.operation, ProvenanceOperation::Imported);
    }
}
