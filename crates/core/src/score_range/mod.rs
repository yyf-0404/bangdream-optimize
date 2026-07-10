//! Score-range search use case.
//!
//! The implementation will consume the same prepared cards, event context, and charts as
//! [`crate::maximize`], while keeping its search policy independent from maximization.

mod mitm;
mod model;
mod pt;
mod search;
mod team;

pub use model::*;
pub use pt::*;
pub use search::*;
pub use team::*;

use crate::EventType;

pub const SUPPORTED_EVENT_TYPES: [EventType; 6] = [
    EventType::Medley,
    EventType::Versus,
    EventType::Challenge,
    EventType::LiveTry,
    EventType::Festival,
    EventType::MissionLive,
];

pub fn supports_event_type(event_type: EventType) -> bool {
    SUPPORTED_EVENT_TYPES.contains(&event_type)
}

pub(crate) const fn fire_cost_for_multiplier(fire_multiplier: u32) -> u64 {
    match fire_multiplier {
        1 => 0,
        5 => 1,
        10 => 2,
        15 => 3,
        _ => panic!("unsupported score-range fire multiplier"),
    }
}

pub(crate) fn total_fire_cost(plays: &[ScoreRangePlay]) -> u64 {
    plays.iter().fold(0_u64, |total, play| {
        total.saturating_add(
            u64::from(play.count).saturating_mul(fire_cost_for_multiplier(play.fire_multiplier)),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supports_requested_event_types() {
        for event_type in SUPPORTED_EVENT_TYPES {
            assert!(supports_event_type(event_type));
        }
        assert!(supports_event_type(EventType::LiveTry));
        assert!(supports_event_type(EventType::Festival));
        assert!(supports_event_type(EventType::MissionLive));
    }

    #[test]
    fn converts_fire_multiplier_to_consumed_fire() {
        assert_eq!(fire_cost_for_multiplier(1), 0);
        assert_eq!(fire_cost_for_multiplier(5), 1);
        assert_eq!(fire_cost_for_multiplier(10), 2);
        assert_eq!(fire_cost_for_multiplier(15), 3);
    }
}
