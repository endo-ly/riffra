use crate::runtime::model::ProjectionKey;

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct TransportSequence(u64);

impl TransportSequence {
    pub(crate) const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TransportIntent {
    Stopped,
    PlayRequested {
        sequence: TransportSequence,
        required_projection: Option<ProjectionKey>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlayDecision {
    Accepted { sequence: TransportSequence },
    Rejected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StopDecision {
    Accepted,
    Rejected,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct TransportController {
    sequence: TransportSequence,
    intent: TransportIntent,
}

impl Default for TransportController {
    fn default() -> Self {
        Self {
            sequence: TransportSequence::new(0),
            intent: TransportIntent::Stopped,
        }
    }
}

impl TransportController {
    pub(crate) fn request_play(
        &mut self,
        sequence: u64,
        required_projection: Option<ProjectionKey>,
    ) -> PlayDecision {
        let sequence = TransportSequence::new(sequence);
        if sequence < self.sequence {
            return PlayDecision::Rejected;
        }
        self.sequence = sequence;
        self.intent = TransportIntent::PlayRequested {
            sequence,
            required_projection,
        };
        PlayDecision::Accepted { sequence }
    }

    pub(crate) fn request_stop(&mut self, sequence: u64) -> StopDecision {
        let sequence = TransportSequence::new(sequence);
        if sequence < self.sequence {
            return StopDecision::Rejected;
        }
        self.sequence = sequence;
        self.intent = TransportIntent::Stopped;
        StopDecision::Accepted
    }

    pub(crate) fn projection_activated(
        &self,
        projection: ProjectionKey,
    ) -> Option<TransportSequence> {
        match self.intent {
            TransportIntent::PlayRequested {
                sequence,
                required_projection,
            } if required_projection.is_none_or(|required| required == projection) => {
                Some(sequence)
            }
            TransportIntent::Stopped | TransportIntent::PlayRequested { .. } => None,
        }
    }

    pub(crate) fn can_execute_play(
        &self,
        sequence: TransportSequence,
        active_projection: Option<ProjectionKey>,
    ) -> bool {
        let TransportIntent::PlayRequested {
            sequence: requested_sequence,
            required_projection,
        } = self.intent
        else {
            return false;
        };

        requested_sequence == sequence
            && required_projection.is_none_or(|required| active_projection == Some(required))
    }

    pub(crate) fn record_play_failure(&mut self, sequence: TransportSequence) -> bool {
        if self.sequence != sequence
            || !matches!(
                self.intent,
                TransportIntent::PlayRequested {
                    sequence: requested_sequence,
                    ..
                } if requested_sequence == sequence
            )
        {
            return false;
        }
        self.intent = TransportIntent::Stopped;
        true
    }

    pub(crate) fn is_play_requested(&self, sequence: TransportSequence) -> bool {
        matches!(
            self.intent,
            TransportIntent::PlayRequested {
                sequence: requested_sequence,
                ..
            } if requested_sequence == sequence
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
    fn rejects_an_older_request_without_changing_the_current_intent() {
        // Arrange
        let mut controller = TransportController::default();
        controller.request_play(2, None);

        // Act
        let decision = controller.request_stop(1);

        // Assert
        assert_eq!(decision, StopDecision::Rejected);
        assert!(controller.is_play_requested(TransportSequence::new(2)));
    }

    #[test]
    fn only_the_current_play_sequence_can_record_failure() {
        // Arrange
        let mut controller = TransportController::default();
        let PlayDecision::Accepted { sequence } = controller.request_play(1, None) else {
            panic!("the current play request must be accepted")
        };
        controller.request_play(2, None);

        // Act
        let stale_failure = controller.record_play_failure(sequence);

        // Assert
        assert!(!stale_failure);
        assert!(controller.is_play_requested(TransportSequence::new(2)));
    }

    #[test]
    fn activates_play_only_for_the_required_projection() {
        // Arrange
        let mut controller = TransportController::default();
        let required = key(3, 8);
        let other = key(4, 9);
        let PlayDecision::Accepted { sequence } = controller.request_play(7, Some(required)) else {
            panic!("the play request must be accepted")
        };

        // Act
        let wrong_projection = controller.projection_activated(other);
        let required_projection = controller.projection_activated(required);

        // Assert
        assert_eq!(wrong_projection, None);
        assert_eq!(required_projection, Some(sequence));
    }
}
