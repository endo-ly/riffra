use riffra_core::ProjectionKey;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct TransportOperationId(pub(crate) u64);

impl TransportOperationId {
    fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransportIntent {
    Stopped,
    PlayRequested {
        operation: TransportOperationId,
        required_projection: Option<ProjectionKey>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct PlayDecision {
    pub(crate) operation: TransportOperationId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct StopDecision {
    pub(crate) operation: TransportOperationId,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TransportController {
    operation: TransportOperationId,
    intent: TransportIntent,
}

impl Default for TransportController {
    fn default() -> Self {
        Self {
            operation: TransportOperationId(0),
            intent: TransportIntent::Stopped,
        }
    }
}

impl TransportController {
    pub(crate) fn request_play(
        &mut self,
        required_projection: Option<ProjectionKey>,
    ) -> PlayDecision {
        self.operation = self.operation.next();
        self.intent = TransportIntent::PlayRequested {
            operation: self.operation,
            required_projection,
        };
        PlayDecision {
            operation: self.operation,
        }
    }

    pub(crate) fn request_stop(&mut self) -> StopDecision {
        self.operation = self.operation.next();
        self.intent = TransportIntent::Stopped;
        StopDecision {
            operation: self.operation,
        }
    }

    pub(crate) fn projection_activated(
        &self,
        projection: ProjectionKey,
    ) -> Option<TransportOperationId> {
        match self.intent {
            TransportIntent::PlayRequested {
                operation,
                required_projection,
            } if required_projection.is_none_or(|required| required == projection) => {
                Some(operation)
            }
            TransportIntent::Stopped | TransportIntent::PlayRequested { .. } => None,
        }
    }

    pub(crate) fn can_execute_play(
        &self,
        operation: TransportOperationId,
        active_projection: Option<ProjectionKey>,
    ) -> bool {
        let TransportIntent::PlayRequested {
            operation: requested_operation,
            required_projection,
        } = self.intent
        else {
            return false;
        };

        requested_operation == operation
            && required_projection.is_none_or(|required| active_projection == Some(required))
    }

    pub(crate) fn record_play_failure(&mut self, operation: TransportOperationId) -> bool {
        if !matches!(
            self.intent,
            TransportIntent::PlayRequested {
                operation: requested_operation,
                ..
            } if requested_operation == operation
        ) {
            return false;
        }
        self.intent = TransportIntent::Stopped;
        true
    }

    pub(crate) fn record_projection_failure(&mut self, projection: ProjectionKey) -> bool {
        let TransportIntent::PlayRequested {
            operation,
            required_projection: Some(required_projection),
        } = self.intent
        else {
            return false;
        };
        required_projection == projection && self.record_play_failure(operation)
    }

    pub(crate) fn invalidate_for_audio_environment(&mut self) {
        self.operation = self.operation.next();
        self.intent = TransportIntent::Stopped;
    }

    #[cfg(test)]
    pub(crate) fn is_play_requested(&self, operation: TransportOperationId) -> bool {
        matches!(
            self.intent,
            TransportIntent::PlayRequested {
                operation: requested_operation,
                ..
            } if requested_operation == operation
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(sequence: u64, session_revision: u64) -> ProjectionKey {
        ProjectionKey {
            sequence,
            session_revision,
        }
    }

    #[test]
    fn host_generates_monotonic_operation_ids_for_play_and_stop() {
        let mut controller = TransportController::default();
        let first = controller.request_play(None);
        let second = controller.request_stop();
        let third = controller.request_play(None);

        assert!(first.operation < second.operation);
        assert!(second.operation < third.operation);
    }

    #[test]
    fn newer_stop_replaces_the_pending_play_intent() {
        let mut controller = TransportController::default();
        let play = controller.request_play(None);
        let stop = controller.request_stop();

        assert!(!controller.is_play_requested(play.operation));
        assert!(stop.operation > play.operation);
    }

    #[test]
    fn activation_only_releases_the_matching_projection() {
        let mut controller = TransportController::default();
        let required = key(3, 8);
        let other = key(4, 9);
        let play = controller.request_play(Some(required));

        assert_eq!(controller.projection_activated(other), None);
        assert_eq!(
            controller.projection_activated(required),
            Some(play.operation)
        );
    }

    #[test]
    fn audio_environment_change_invalidates_the_current_play_intent() {
        let mut controller = TransportController::default();
        let play = controller.request_play(None);

        controller.invalidate_for_audio_environment();

        assert!(!controller.is_play_requested(play.operation));
    }
}
