use crate::selection::{choose_station, Candidate, GeoPoint, SelectionConfig, SelectionState};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum TuneAction {
    Switched(Candidate),
    Retained(Candidate),
    Cooldown,
    NoCandidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TuneMode {
    #[default]
    AutoTune,
    Explore,
    FavoriteLock,
}

pub struct AutoTune {
    pub config: SelectionConfig,
    pub state: SelectionState,
    pub cooldown: Duration,
    pub current: Option<Candidate>,
    pub mode: TuneMode,
    history: Vec<Candidate>,
    last_switch: Option<Instant>,
}

impl Default for AutoTune {
    fn default() -> Self {
        Self {
            config: SelectionConfig::default(),
            state: SelectionState::default(),
            cooldown: Duration::from_secs(180),
            current: None,
            mode: TuneMode::AutoTune,
            history: Vec::new(),
            last_switch: None,
        }
    }
}

impl AutoTune {
    pub fn update(
        &mut self,
        now: Instant,
        origin: GeoPoint,
        candidates: &[Candidate],
    ) -> TuneAction {
        if self.mode == TuneMode::Explore {
            return self
                .current
                .clone()
                .map_or(TuneAction::NoCandidate, TuneAction::Retained);
        }
        let Some((candidate, _)) = choose_station(candidates, origin, self.config, &self.state)
        else {
            return TuneAction::NoCandidate;
        };
        if self
            .current
            .as_ref()
            .is_some_and(|current| current.id == candidate.id)
        {
            return TuneAction::Retained(candidate);
        }
        if !self.state.favorite_lock
            && self
                .last_switch
                .is_some_and(|last| now.duration_since(last) < self.cooldown)
        {
            return TuneAction::Cooldown;
        }
        self.switch_to(candidate.clone(), now);
        TuneAction::Switched(candidate)
    }

    pub fn next(&mut self, candidates: &[Candidate], now: Instant) -> TuneAction {
        let candidate = candidates
            .iter()
            .find(|candidate| {
                !self.state.banned.contains(&candidate.id)
                    && self
                        .current
                        .as_ref()
                        .is_none_or(|current| current.id != candidate.id)
            })
            .cloned();
        match candidate {
            Some(candidate) => {
                self.switch_to(candidate.clone(), now);
                TuneAction::Switched(candidate)
            }
            None => TuneAction::NoCandidate,
        }
    }

    pub fn previous(&mut self, now: Instant) -> TuneAction {
        let Some(candidate) = self.history.pop() else {
            return TuneAction::NoCandidate;
        };
        self.current = Some(candidate.clone());
        self.state.current_id = Some(candidate.id.clone());
        self.last_switch = Some(now);
        TuneAction::Switched(candidate)
    }

    pub fn set_favorite_lock(&mut self, locked: bool) {
        self.state.favorite_lock = locked;
        self.mode = if locked {
            TuneMode::FavoriteLock
        } else {
            TuneMode::AutoTune
        };
    }

    pub fn set_mode(&mut self, mode: TuneMode) {
        self.mode = mode;
        self.state.favorite_lock = mode == TuneMode::FavoriteLock;
    }

    /// Allow the next location-driven update to select a better local station.
    ///
    /// This is used after a meaningful in-game move or quick travel. Ordinary
    /// telemetry updates still retain the cooldown so the station does not
    /// churn while the player is driving through the same area.
    pub fn reset_location_cooldown(&mut self) {
        self.last_switch = None;
    }

    fn switch_to(&mut self, candidate: Candidate, now: Instant) {
        if let Some(current) = self.current.replace(candidate.clone()) {
            self.history.push(current);
        }
        self.state.current_id = Some(candidate.id);
        self.last_switch = Some(now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(id: &str) -> Candidate {
        Candidate {
            id: id.into(),
            name: id.into(),
            city: None,
            location: GeoPoint {
                latitude: 0.,
                longitude: 0.,
            },
            country: None,
            reliable: 1.,
            playable: true,
            favorite: false,
            hidden: false,
            successes: 0,
            failures: 0,
        }
    }
    #[test]
    fn cooldown_lock_and_history_behave() {
        let origin = GeoPoint {
            latitude: 0.,
            longitude: 0.,
        };
        let now = Instant::now();
        let mut tune = AutoTune::default();
        let list = [candidate("a"), candidate("b")];
        assert!(matches!(
            tune.update(now, origin, &list),
            TuneAction::Switched(_)
        ));
        assert!(matches!(
            tune.next(&list, now + Duration::from_secs(1)),
            TuneAction::Switched(_)
        ));
        assert!(matches!(
            tune.previous(now + Duration::from_secs(2)),
            TuneAction::Switched(_)
        ));
        tune.set_favorite_lock(true);
        assert!(matches!(
            tune.update(now + Duration::from_secs(3), origin, &list),
            TuneAction::Retained(_)
        ));
    }
    #[test]
    fn automatic_switch_respects_cooldown() {
        let now = Instant::now();
        let mut tune = AutoTune::default();
        let a = candidate("a");
        let b = candidate("b");
        let origin = GeoPoint {
            latitude: 0.,
            longitude: 0.,
        };
        assert!(matches!(
            tune.update(now, origin, &[a]),
            TuneAction::Switched(_)
        ));
        tune.config.improvement_threshold = 0.;
        assert!(matches!(
            tune.update(now + Duration::from_secs(1), origin, &[b]),
            TuneAction::Cooldown
        ));
    }

    #[test]
    fn explore_suppresses_location_driven_updates() {
        let now = Instant::now();
        let mut tune = AutoTune::default();
        let a = candidate("a");
        assert!(matches!(
            tune.update(
                now,
                GeoPoint {
                    latitude: 0.,
                    longitude: 0.
                },
                std::slice::from_ref(&a)
            ),
            TuneAction::Switched(_)
        ));
        tune.set_mode(TuneMode::Explore);
        assert!(
            matches!(tune.update(now + Duration::from_secs(1), GeoPoint { latitude: 10., longitude: 10. }, &[]), TuneAction::Retained(current) if current.id == "a")
        );
    }

    #[test]
    fn favorite_lock_can_resume_from_a_persisted_current_id() {
        let now = Instant::now();
        let mut tune = AutoTune::default();
        tune.set_mode(TuneMode::FavoriteLock);
        tune.state.current_id = Some("favorite".into());
        let favorite = Candidate {
            favorite: true,
            ..candidate("favorite")
        };
        assert!(matches!(
            tune.update(
                now,
                GeoPoint {
                    latitude: 0.,
                    longitude: 0.
                },
                &[favorite]
            ),
            TuneAction::Switched(candidate) if candidate.id == "favorite"
        ));
    }

    #[test]
    fn location_cooldown_can_be_reset_after_travel() {
        let now = Instant::now();
        let mut tune = AutoTune::default();
        let a = candidate("a");
        let b = candidate("b");
        let origin = GeoPoint {
            latitude: 0.,
            longitude: 0.,
        };
        assert!(matches!(
            tune.update(now, origin, &[a]),
            TuneAction::Switched(_)
        ));
        tune.config.improvement_threshold = 0.;
        assert!(matches!(
            tune.update(
                now + Duration::from_secs(1),
                origin,
                std::slice::from_ref(&b)
            ),
            TuneAction::Cooldown
        ));
        tune.reset_location_cooldown();
        assert!(matches!(
            tune.update(now + Duration::from_secs(1), origin, &[b]),
            TuneAction::Switched(_)
        ));
    }
}
