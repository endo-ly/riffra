use super::*;

impl HostState {
    pub(super) fn identity(&self) -> &HostIdentity {
        &self.identity
    }

    pub(crate) fn subscribe_events(&self) -> Option<HostEventSubscription> {
        self.event_hub.subscribe()
    }

    pub(super) fn canonical(&self) -> Result<CanonicalState, HostError> {
        self.core
            .canonical_state()
            .map_err(|error| HostError::State(error.to_string()))
    }

    pub(super) fn bootstrap(&self) -> Result<HostBootstrap, HostError> {
        let recovered_from_generation = self.core.recovered_from_generation();
        let recovery_candidates = if recovered_from_generation {
            self.storage
                .recovery_candidates()
                .map_err(|error| HostError::State(error.to_string()))?
        } else {
            Vec::new()
        };
        Ok(HostBootstrap {
            canonical: self.canonical()?,
            plugin_catalog: plugins::load(&self.data_root)
                .map_err(|error| HostError::State(error.to_string()))?,
            runtime_started: self.core.audio().startup_completed(),
            runtime_startup_finished: self.core.audio().startup_finished(),
            runtime_projection: self.runtime.status(),
            audio_status: self
                .core
                .audio()
                .status()
                .map_err(|error| HostError::State(error.to_string()))?,
            recovered_from_generation,
            safe_mode: self.core.safe_mode(),
            recovery_candidates,
            data_root: self.data_root.clone(),
        })
    }
}
