use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GeoPoint {
    pub latitude: f64,
    pub longitude: f64,
}

impl GeoPoint {
    pub fn valid(self) -> bool {
        self.latitude.is_finite()
            && self.longitude.is_finite()
            && self.latitude.abs() <= 90.0
            && self.longitude.abs() <= 180.0
    }
    pub fn distance_km(self, other: Self) -> f64 {
        let dlat = (other.latitude - self.latitude).to_radians();
        let dlon = (other.longitude - self.longitude).to_radians();
        let a = (dlat / 2.0).sin().powi(2)
            + self.latitude.to_radians().cos()
                * other.latitude.to_radians().cos()
                * (dlon / 2.0).sin().powi(2);
        6371.0088 * 2.0 * a.sqrt().atan2((1.0 - a).sqrt())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub city: Option<String>,
    pub location: GeoPoint,
    pub country: Option<String>,
    pub reliable: f64,
    pub playable: bool,
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default)]
    pub successes: u32,
    #[serde(default)]
    pub failures: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct SelectionConfig {
    pub radius_km: f64,
    pub proximity_weight: f64,
    pub reliability_weight: f64,
    pub preference_weight: f64,
    pub variety_weight: f64,
    pub improvement_threshold: f64,
}
impl Default for SelectionConfig {
    fn default() -> Self {
        Self {
            radius_km: 200.0,
            proximity_weight: 55.0,
            reliability_weight: 25.0,
            preference_weight: 10.0,
            variety_weight: 10.0,
            improvement_threshold: 0.30,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SelectionState {
    pub current_id: Option<String>,
    pub favorite_lock: bool,
    pub banned: Vec<String>,
    pub recent_history: Vec<String>,
}

pub fn score(candidate: &Candidate, origin: GeoPoint, config: SelectionConfig) -> Option<f64> {
    score_with_history(candidate, origin, config, &[])
}

fn score_with_history(
    candidate: &Candidate,
    origin: GeoPoint,
    config: SelectionConfig,
    recent_history: &[String],
) -> Option<f64> {
    if !origin.valid()
        || !candidate.location.valid()
        || !candidate.playable
        || candidate.reliable.is_nan()
        || candidate.hidden
    {
        return None;
    }
    let distance = origin.distance_km(candidate.location);
    if distance > config.radius_km {
        return None;
    }
    let proximity = (-distance / 120.0).exp().clamp(0.0, 1.0);
    let reliability = if candidate.successes + candidate.failures > 0 {
        (f64::from(candidate.successes) + 3.0)
            / (f64::from(candidate.successes + candidate.failures) + 5.0)
    } else {
        candidate.reliable.clamp(0.0, 1.0)
    };
    let preference = if candidate.favorite { 1.0 } else { 0.5 };
    let variety = if candidate.favorite || !recent_history.iter().any(|id| id == &candidate.id) {
        1.0
    } else {
        0.0
    };
    Some(
        (config.proximity_weight * proximity
            + config.reliability_weight * reliability
            + config.preference_weight * preference
            + config.variety_weight * variety)
            .clamp(0.0, 100.0),
    )
}

pub fn choose_station(
    candidates: &[Candidate],
    origin: GeoPoint,
    config: SelectionConfig,
    state: &SelectionState,
) -> Option<(Candidate, f64)> {
    choose_station_for_country(candidates, origin, None, config, state)
}

pub fn choose_station_for_country(
    candidates: &[Candidate],
    origin: GeoPoint,
    country: Option<&str>,
    config: SelectionConfig,
    state: &SelectionState,
) -> Option<(Candidate, f64)> {
    if state.favorite_lock {
        return state.current_id.as_ref().and_then(|id| {
            candidates
                .iter()
                .find(|c| &c.id == id)
                .cloned()
                .map(|c| (c, 100.0))
        });
    }
    let mut ranked: Vec<_> = candidates
        .iter()
        .filter(|c| !state.banned.contains(&c.id))
        .filter(|c| country.is_none_or(|wanted| c.country.as_deref() == Some(wanted)))
        .filter_map(|c| {
            score_with_history(c, origin, config, &state.recent_history).map(|s| (c, s))
        })
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.id.cmp(&b.0.id)));
    let (best, score) = *ranked.first()?;
    if let Some(current) = &state.current_id {
        if let Some((_, old)) = ranked.iter().find(|(c, _)| &c.id == current) {
            if *old > 0.0 && score < *old * (1.0 + config.improvement_threshold) {
                return Some((candidates.iter().find(|c| &c.id == current)?.clone(), *old));
            }
        }
    }
    Some((best.clone(), score))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn c(id: &str, lat: f64, lon: f64, reliable: f64) -> Candidate {
        Candidate {
            id: id.into(),
            name: id.into(),
            city: None,
            location: GeoPoint {
                latitude: lat,
                longitude: lon,
            },
            country: None,
            reliable,
            playable: true,
            favorite: false,
            hidden: false,
            successes: 0,
            failures: 0,
        }
    }
    #[test]
    fn nearby_beats_far() {
        let origin = GeoPoint {
            latitude: 48.1,
            longitude: 11.5,
        };
        let got = choose_station(
            &[c("near", 48.11, 11.51, 0.8), c("far", 52.5, 13.4, 1.0)],
            origin,
            SelectionConfig::default(),
            &SelectionState::default(),
        )
        .unwrap();
        assert_eq!(got.0.id, "near");
    }
    #[test]
    fn lock_and_ban_work() {
        let origin = GeoPoint {
            latitude: 0.,
            longitude: 0.,
        };
        let mut s = SelectionState {
            current_id: Some("a".into()),
            favorite_lock: false,
            banned: vec!["a".into()],
            recent_history: Vec::new(),
        };
        assert_eq!(
            choose_station(
                &[c("a", 0., 0., 1.), c("b", 1., 1., 1.)],
                origin,
                SelectionConfig::default(),
                &s
            )
            .unwrap()
            .0
            .id,
            "b"
        );
        s.favorite_lock = true;
        assert_eq!(
            choose_station(
                &[c("a", 0., 0., 1.), c("b", 1., 1., 1.)],
                origin,
                SelectionConfig::default(),
                &s
            )
            .unwrap()
            .0
            .id,
            "a"
        );
    }

    #[test]
    fn score_uses_explicit_weighted_formula_and_neutral_prior() {
        let candidate = Candidate {
            successes: 0,
            failures: 0,
            ..c("station", 0., 0., 0.6)
        };
        let score_value = score(
            &candidate,
            GeoPoint {
                latitude: 0.,
                longitude: 0.,
            },
            SelectionConfig::default(),
        )
        .unwrap();
        assert!((score_value - 85.0).abs() < 1e-9);
    }

    #[test]
    fn country_gate_and_relative_loyalty_are_deterministic() {
        let origin = GeoPoint {
            latitude: 0.,
            longitude: 0.,
        };
        let mut current = c("current", 0., 0., 1.0);
        current.country = Some("DE".into());
        let mut other_country = c("other", 0., 0.001, 1.0);
        other_country.country = Some("PL".into());
        let state = SelectionState {
            current_id: Some("current".into()),
            recent_history: Vec::new(),
            ..SelectionState::default()
        };
        assert_eq!(
            choose_station_for_country(
                &[current.clone(), other_country],
                origin,
                Some("DE"),
                SelectionConfig::default(),
                &state,
            )
            .unwrap()
            .0
            .id,
            "current"
        );

        let mut better = c("better", 0., 0.001, 1.0);
        better.country = Some("DE".into());
        let retained = choose_station(
            &[current, better],
            origin,
            SelectionConfig::default(),
            &state,
        )
        .unwrap();
        assert_eq!(retained.0.id, "current");
    }
}
