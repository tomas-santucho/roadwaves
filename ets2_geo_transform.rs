//! RoadWaves ETS2 (vanilla map) world coordinates -> approximate WGS84 latitude/longitude.
//!
//! Uses the spherical Lambert Conformal Conic projection parameters documented
//! for ETS2 map coordinate conversion. This is an independent implementation
//! of the mathematical formula (no third-party Rust dependencies).
//!
//! IMPORTANT:
//! - Approximate real-world position only: ETS2's map is intentionally distorted.
//! - Calibrate locally with city anchors for improved radio station selection.
//! - These parameters must be treated as map-profile-specific (not universal for map mods/ATS).
//! - For ETS2 position use horizontal `x` and `z`; `y` is height/elevation.

use std::f64::consts::PI;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoCoordinate {
    pub latitude: f64,
    pub longitude: f64,
}

/// Convert game X/Z into approximate real-world latitude / longitude.
/// Returns None if the input or computed output is invalid.
pub fn ets2_to_wgs84(x: f64, z: f64) -> Option<GeoCoordinate> {
    if !x.is_finite() || !z.is_finite() {
        return None;
    }

    const EARTH_RADIUS_M: f64 = 6_370_997.0;
    const MAP_OFFSET_X: f64 = 16_660.0;
    const MAP_OFFSET_Z: f64 = 4_150.0;
    const MAP_FACTOR_Z: f64 = -0.000_171_570_875;
    const MAP_FACTOR_X: f64 = 0.000_172_924_146_3;

    let lat1 = 37.0_f64.to_radians();
    let lat2 = 65.0_f64.to_radians();
    let lat0 = 50.0_f64.to_radians();
    let lon0 = 15.0_f64.to_radians();

    let t = |phi: f64| (PI / 4.0 + phi / 2.0).tan();
    let n = (lat1.cos() / lat2.cos()).ln() / (t(lat2) / t(lat1)).ln();
    let f = lat1.cos() * t(lat1).powf(n) / n;
    let rho0 = EARTH_RADIUS_M * f / t(lat0).powf(n);

    let meters_per_degree = EARTH_RADIUS_M * PI / 180.0;
    let east = (x - MAP_OFFSET_X) * MAP_FACTOR_X * meters_per_degree;
    let north = (z - MAP_OFFSET_Z) * MAP_FACTOR_Z * meters_per_degree;

    let rho = east.hypot(rho0 - north);
    if rho <= f64::EPSILON {
        return None;
    }
    let theta = east.atan2(rho0 - north);

    let latitude = (2.0 * (EARTH_RADIUS_M * f / rho).powf(1.0 / n).atan() - PI / 2.0)
        .to_degrees();
    let longitude = (lon0 + theta / n).to_degrees();

    if !latitude.is_finite()
        || !longitude.is_finite()
        || !(-90.0..=90.0).contains(&latitude)
        || !(-180.0..=180.0).contains(&longitude)
    {
        return None;
    }

    Some(GeoCoordinate { latitude, longitude })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(value: f64, expected: f64, tolerance: f64) {
        assert!((value - expected).abs() <= tolerance, "{value} != {expected}");
    }

    #[test]
    fn maps_berlin_reference_point() {
        let p = ets2_to_wgs84(10_183.1, -10_001.2).unwrap();
        approx(p.latitude, 52.4878, 0.001);
        approx(p.longitude, 13.1037, 0.001);
    }

    #[test]
    fn maps_amsterdam_reference_point() {
        let p = ets2_to_wgs84(-18_562.0, -11_736.2).unwrap();
        approx(p.latitude, 52.3650, 0.001);
        approx(p.longitude, 4.6829, 0.001);
    }

    #[test]
    fn maps_warsaw_reference_point() {
        let p = ets2_to_wgs84(37_444.9, -9_435.84).unwrap();
        approx(p.latitude, 52.2489, 0.001);
        approx(p.longitude, 21.0591, 0.001);
    }

    #[test]
    fn rejects_invalid_input() {
        assert_eq!(ets2_to_wgs84(f64::NAN, 1.0), None);
        assert_eq!(ets2_to_wgs84(1.0, f64::INFINITY), None);
    }
}
