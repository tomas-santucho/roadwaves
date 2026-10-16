use crate::selection::GeoPoint;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct WorldAnchor {
    pub world_x: f64,
    pub world_z: f64,
    pub real: GeoPoint,
}

#[derive(Debug, Error, PartialEq)]
pub enum CalibrationError {
    #[error("three anchors are required")]
    AnchorCount,
    #[error("anchors are degenerate or collinear")]
    Degenerate,
    #[error("anchor geography is invalid")]
    InvalidGeography,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Calibration {
    latitude: [f64; 3],
    longitude: [f64; 3],
    min_world_x: f64,
    max_world_x: f64,
    min_world_z: f64,
    max_world_z: f64,
}

impl Calibration {
    /// The stock ETS2 Europe map transform from the game's climate data.
    ///
    /// The game expresses the globe position as:
    /// latitude  = 50 + (world_z - 16660) * -1.71570875e-4
    /// longitude = 15 + (world_x - 4150) *  1.729241463e-4
    ///
    /// It is intentionally bounded to the stock map envelope. Custom maps
    /// should continue to provide an explicit three-anchor calibration.
    pub fn ets2_default() -> Self {
        Self {
            latitude: [0.0, -1.71570875e-4, 50.0 + 16660.0 * 1.71570875e-4],
            longitude: [1.729241463e-4, 0.0, 15.0 - 4150.0 * 1.729241463e-4],
            min_world_x: -60000.0,
            max_world_x: 50000.0,
            min_world_z: -30000.0,
            max_world_z: 60000.0,
        }
    }
}

impl Calibration {
    pub fn from_anchors(anchors: &[WorldAnchor]) -> Result<Self, CalibrationError> {
        if anchors.len() != 3 {
            return Err(CalibrationError::AnchorCount);
        }
        if anchors.iter().any(|a| !a.real.valid()) {
            return Err(CalibrationError::InvalidGeography);
        }
        let matrix = [
            [anchors[0].world_x, anchors[0].world_z, 1.0],
            [anchors[1].world_x, anchors[1].world_z, 1.0],
            [anchors[2].world_x, anchors[2].world_z, 1.0],
        ];
        let determinant = det(matrix);
        if !determinant.is_finite() || determinant.abs() < 1e-9 {
            return Err(CalibrationError::Degenerate);
        }
        Ok(Self {
            latitude: solve(
                matrix,
                [
                    anchors[0].real.latitude,
                    anchors[1].real.latitude,
                    anchors[2].real.latitude,
                ],
            ),
            longitude: solve(
                matrix,
                [
                    anchors[0].real.longitude,
                    anchors[1].real.longitude,
                    anchors[2].real.longitude,
                ],
            ),
            min_world_x: anchors
                .iter()
                .map(|anchor| anchor.world_x)
                .fold(f64::INFINITY, f64::min),
            max_world_x: anchors
                .iter()
                .map(|anchor| anchor.world_x)
                .fold(f64::NEG_INFINITY, f64::max),
            min_world_z: anchors
                .iter()
                .map(|anchor| anchor.world_z)
                .fold(f64::INFINITY, f64::min),
            max_world_z: anchors
                .iter()
                .map(|anchor| anchor.world_z)
                .fold(f64::NEG_INFINITY, f64::max),
        })
    }

    pub fn resolve(&self, world_x: f64, world_z: f64) -> Option<GeoPoint> {
        if !world_x.is_finite() || !world_z.is_finite() {
            return None;
        }
        let point = GeoPoint {
            latitude: self.latitude[0] * world_x + self.latitude[1] * world_z + self.latitude[2],
            longitude: self.longitude[0] * world_x
                + self.longitude[1] * world_z
                + self.longitude[2],
        };
        point.valid().then_some(point)
    }

    /// Resolves only inside the calibrated world-space envelope. Extrapolation is
    /// deliberately refused because three anchors do not prove a whole-map transform.
    pub fn resolve_bounded(&self, world_x: f64, world_z: f64) -> Option<GeoPoint> {
        if world_x < self.min_world_x
            || world_x > self.max_world_x
            || world_z < self.min_world_z
            || world_z > self.max_world_z
        {
            return None;
        }
        self.resolve(world_x, world_z)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocationConfidence {
    BoundedInterpolation,
    BuiltInEts2Map,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocationSource {
    ManualCalibration,
    BuiltInEts2Map,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LocationResolution {
    pub estimated_geo: Option<GeoPoint>,
    pub country_code: Option<String>,
    pub country_name: Option<String>,
    pub nearest_city: Option<String>,
    pub game_map_id: Option<String>,
    pub confidence: LocationConfidence,
    pub source: LocationSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameGeoResolver {
    pub calibration: Option<Calibration>,
    pub source: LocationSource,
}

impl GameGeoResolver {
    pub fn unknown() -> Self {
        Self {
            calibration: None,
            source: LocationSource::Unknown,
        }
    }

    pub fn from_calibration(calibration: Calibration) -> Self {
        Self {
            calibration: Some(calibration),
            source: LocationSource::ManualCalibration,
        }
    }

    pub fn ets2_default() -> Self {
        Self {
            calibration: Some(Calibration::ets2_default()),
            source: LocationSource::BuiltInEts2Map,
        }
    }

    pub fn resolve(&self, world_x: f64, world_z: f64) -> LocationResolution {
        let estimated_geo = self
            .calibration
            .as_ref()
            .and_then(|calibration| calibration.resolve_bounded(world_x, world_z));
        let known = estimated_geo.is_some();
        LocationResolution {
            estimated_geo,
            country_code: None,
            country_name: None,
            nearest_city: None,
            game_map_id: None,
            confidence: if known {
                match self.source {
                    LocationSource::BuiltInEts2Map => LocationConfidence::BuiltInEts2Map,
                    _ => LocationConfidence::BoundedInterpolation,
                }
            } else {
                LocationConfidence::Unknown
            },
            source: if known {
                self.source
            } else {
                LocationSource::Unknown
            },
        }
    }
}

fn det(m: [[f64; 3]; 3]) -> f64 {
    m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
}
fn solve(m: [[f64; 3]; 3], b: [f64; 3]) -> [f64; 3] {
    let mut result = [0.0; 3];
    for column in 0..3 {
        let mut replaced = m;
        for row in 0..3 {
            replaced[row][column] = b[row];
        }
        result[column] = det(replaced) / det(m);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn three_anchors_interpolate_world_coordinates() {
        let calibration = Calibration::from_anchors(&[
            WorldAnchor {
                world_x: 0.,
                world_z: 0.,
                real: GeoPoint {
                    latitude: 10.,
                    longitude: 20.,
                },
            },
            WorldAnchor {
                world_x: 100.,
                world_z: 0.,
                real: GeoPoint {
                    latitude: 11.,
                    longitude: 20.,
                },
            },
            WorldAnchor {
                world_x: 0.,
                world_z: 100.,
                real: GeoPoint {
                    latitude: 10.,
                    longitude: 21.,
                },
            },
        ])
        .unwrap();
        let point = calibration.resolve(50., 50.).unwrap();
        assert!((point.latitude - 10.5).abs() < 1e-9);
        assert!((point.longitude - 20.5).abs() < 1e-9);
    }
    #[test]
    fn collinear_anchors_are_rejected() {
        let anchors = [
            WorldAnchor {
                world_x: 0.,
                world_z: 0.,
                real: GeoPoint {
                    latitude: 0.,
                    longitude: 0.,
                },
            },
            WorldAnchor {
                world_x: 1.,
                world_z: 1.,
                real: GeoPoint {
                    latitude: 1.,
                    longitude: 1.,
                },
            },
            WorldAnchor {
                world_x: 2.,
                world_z: 2.,
                real: GeoPoint {
                    latitude: 2.,
                    longitude: 2.,
                },
            },
        ];
        assert!(matches!(
            Calibration::from_anchors(&anchors),
            Err(CalibrationError::Degenerate)
        ));
    }

    #[test]
    fn resolver_refuses_extrapolation_outside_anchor_envelope() {
        let calibration = Calibration::from_anchors(&[
            WorldAnchor {
                world_x: 0.,
                world_z: 0.,
                real: GeoPoint {
                    latitude: 10.,
                    longitude: 20.,
                },
            },
            WorldAnchor {
                world_x: 100.,
                world_z: 0.,
                real: GeoPoint {
                    latitude: 11.,
                    longitude: 20.,
                },
            },
            WorldAnchor {
                world_x: 0.,
                world_z: 100.,
                real: GeoPoint {
                    latitude: 10.,
                    longitude: 21.,
                },
            },
        ])
        .unwrap();
        let resolver = GameGeoResolver::from_calibration(calibration);
        assert_eq!(
            resolver.resolve(200., 200.).confidence,
            LocationConfidence::Unknown
        );
        assert_eq!(
            resolver.resolve(50., 50.).confidence,
            LocationConfidence::BoundedInterpolation
        );
    }

    #[test]
    fn stock_ets2_transform_resolves_live_world_coordinates() {
        let resolver = GameGeoResolver::ets2_default();
        let location = resolver.resolve(-28842.38, 4982.94);
        let point = location.estimated_geo.unwrap();
        assert_eq!(location.confidence, LocationConfidence::BuiltInEts2Map);
        assert_eq!(location.source, LocationSource::BuiltInEts2Map);
        assert!((51.9..=52.1).contains(&point.latitude));
        assert!((9.1..=9.5).contains(&point.longitude));
    }
}
