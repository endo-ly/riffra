//! The compiled Score document a Project is linked to.

use crate::score;
use crate::score_yaml::export::{ticks, to_quarters};
use riffra_core::CreativeSession;
use serde_json::Value;

/// Original Score declarations the Session cannot reconstruct.
pub(crate) struct LinkedScore {
    document: Value,
    plan: score::ScorePlan,
}

impl LinkedScore {
    /// Compiles the Score document the Session is linked to.
    pub(crate) fn load(session: &CreativeSession) -> Option<Self> {
        let source = session.score_source.as_ref()?;
        let plan = score::compile(&source.yaml).ok()?;
        let document = score::parse_document(&source.yaml).ok()?;
        Some(Self { document, plan })
    }

    /// Returns the Score form occurrence matching one section start.
    pub(crate) fn occurrence(
        &self,
        name: &str,
        start_tick: u64,
        ppq: u32,
    ) -> Option<&score::Occurrence> {
        self.plan.sections.iter().find(|occurrence| {
            occurrence.section == name
                && ticks(
                    &to_quarters(occurrence.start_tick, u32::from(self.plan.ticks_per_beat)),
                    ppq,
                ) == Some(start_tick)
        })
    }

    /// Returns a raw Score field, following no inheritance.
    pub(crate) fn field(&self, path: &[&str]) -> Option<&Value> {
        let mut value = &self.document;
        for key in path {
            value = value.get(key)?;
        }
        Some(value)
    }

    /// Returns the compiled Score plan.
    pub(crate) fn plan(&self) -> &score::ScorePlan {
        &self.plan
    }
}
