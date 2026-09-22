//! Runtime resolution for built-in and User Instruments.

mod builtin;
mod metadata;
mod user;

pub use builtin::{
    BuiltInInstrumentCatalog, BuiltInInstrumentDefinition, BuiltInInstrumentSummary,
};
pub use metadata::{
    InstrumentPreviewDefinition, InstrumentPreviewNote, InstrumentPreviewTimeSignature,
    InstrumentRecommendedRange,
};
pub use user::{
    ProjectInstrumentSnapshot, ResolvedUserInstrument, UserInstrumentManifest, UserInstrumentStore,
};
