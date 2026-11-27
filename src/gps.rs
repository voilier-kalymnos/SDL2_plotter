//! Great-circle geometry on a spherical Earth.
//!
//! Every point is a `(lon, lat)` tuple in degrees, the same order as the rest
//! of the code base. Distances come in nautical miles or metres; bearings are
//! initial (forward) bearings in degrees clockwise from true north, 0..360.

/// Mean Earth radius in nautical miles.
pub const EARTH_RADIUS_NM: f64 = 3440.065;
/// Mean Earth radius in metres.
pub const EARTH_RADIUS_METERS: f64 = 6_371_000.0;

/// Central angle in radians between two `(lon, lat)` points (haversine).
fn central_angle(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lon1, lat1) = a;
    let (lon2, lat2) = b;
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let delta_lat = (lat2 - lat1).to_radians();
    let delta_lon = (lon2 - lon1).to_radians();

    let h = (delta_lat / 2.0).sin().powi(2)
        + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
    // Rounding can push h a hair above 1 for antipodal points; asin would then be NaN.
    2.0 * h.sqrt().min(1.0).asin()
}

/// Great-circle distance in nautical miles between two `(lon, lat)` points.
pub fn distance_nm(a: (f64, f64), b: (f64, f64)) -> f64 {
    EARTH_RADIUS_NM * central_angle(a, b)
}

/// Great-circle distance in metres between two `(lon, lat)` points.
pub fn distance_meters(a: (f64, f64), b: (f64, f64)) -> f64 {
    EARTH_RADIUS_METERS * central_angle(a, b)
}

/// Initial bearing in degrees (0..360, clockwise from true north) from `a` to `b`.
pub fn bearing_deg(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lon1, lat1) = a;
    let (lon2, lat2) = b;
    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();
    let delta_lon = (lon2 - lon1).to_radians();

    let y = delta_lon.sin() * lat2_rad.cos();
    let x = lat1_rad.cos() * lat2_rad.sin() - lat1_rad.sin() * lat2_rad.cos() * delta_lon.cos();
    (y.atan2(x).to_degrees() + 360.0) % 360.0
}

/// Distance in nautical miles and initial bearing in degrees from `a` to `b`.
pub fn distance_bearing_nm(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    (distance_nm(a, b), bearing_deg(a, b))
}

/// `degrees` brought into -180..180, so a longitude difference goes the
/// short way across the 180 meridian.
pub fn wrap180(degrees: f64) -> f64 {
    let wrapped = (degrees + 180.0).rem_euclid(360.0) - 180.0;
    // rem_euclid gives -180 for +180; either is the same meridian.
    if wrapped == -180.0 && degrees > 0.0 {
        180.0
    } else {
        wrapped
    }
}

/// Below this difference of Mercator latitude a leg runs along a parallel.
const ALONG_PARALLEL: f64 = 1e-12;

/// Mercator ("stretched") latitude in radians of a latitude in degrees.
fn mercator_latitude(lat: f64) -> f64 {
    (std::f64::consts::FRAC_PI_4 + lat.to_radians() / 2.0)
        .tan()
        .ln()
}

/// The parts of a rhumb line from `a` to `b`: latitude difference,
/// Mercator latitude difference and short-way longitude difference, all in
/// radians. `None` when a point is at a pole, where a rhumb line has no
/// direction.
fn rhumb_parts(a: (f64, f64), b: (f64, f64)) -> Option<(f64, f64, f64)> {
    if a.1.abs() >= 90.0 || b.1.abs() >= 90.0 {
        return None;
    }
    let dlat = (b.1 - a.1).to_radians();
    let dpsi = mercator_latitude(b.1) - mercator_latitude(a.1);
    let dlon = wrap180(b.0 - a.0).to_radians();
    Some((dlat, dpsi, dlon))
}

/// Rhumb line (constant bearing) distance in nautical miles from `a` to
/// `b`, on the same sphere as `distance_nm`. `None` at a pole.
pub fn rhumb_distance_nm(a: (f64, f64), b: (f64, f64)) -> Option<f64> {
    let (dlat, dpsi, dlon) = rhumb_parts(a, b)?;
    // Along a parallel dlat / dpsi is 0 / 0: use its limit, cos(lat).
    let q = if dpsi.abs() > ALONG_PARALLEL {
        dlat / dpsi
    } else {
        a.1.to_radians().cos()
    };
    Some((dlat * dlat + q * q * dlon * dlon).sqrt() * EARTH_RADIUS_NM)
}

/// The constant true bearing in degrees (0..360) of the rhumb line from
/// `a` to `b`. `None` at a pole.
pub fn rhumb_bearing_deg(a: (f64, f64), b: (f64, f64)) -> Option<f64> {
    let (_, dpsi, dlon) = rhumb_parts(a, b)?;
    Some(dlon.atan2(dpsi).to_degrees().rem_euclid(360.0))
}

/// Rhumb line distance in nautical miles and bearing in degrees from `a`
/// to `b`. `None` at a pole.
pub fn rhumb_distance_bearing_nm(a: (f64, f64), b: (f64, f64)) -> Option<(f64, f64)> {
    rhumb_distance_nm(a, b).zip(rhumb_bearing_deg(a, b))
}

/// Distance in nautical miles and true bearing in degrees of a leg that a
/// helmsman steers: the rhumb line, or the great circle where a rhumb line
/// has no value (a pole).
pub fn leg_nm(a: (f64, f64), b: (f64, f64)) -> (f64, f64) {
    rhumb_distance_bearing_nm(a, b).unwrap_or_else(|| distance_bearing_nm(a, b))
}

/// The point at `fraction` (0 to 1) of the distance along the rhumb line
/// from `a` to `b`. `None` at a pole.
pub fn rhumb_point(a: (f64, f64), b: (f64, f64), fraction: f64) -> Option<(f64, f64)> {
    let (_, dpsi, dlon) = rhumb_parts(a, b)?;
    let lat = a.1 + fraction * (b.1 - a.1);
    // The longitude moves in step with the Mercator latitude.
    let part = if dpsi.abs() > ALONG_PARALLEL {
        (mercator_latitude(lat) - mercator_latitude(a.1)) / dpsi
    } else {
        fraction
    };
    Some((wrap180(a.0 + part * dlon.to_degrees()), lat))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent source of truth: one degree of arc on a sphere of radius R is
    // R * pi / 180. With the radii above that is 60.04 nautical miles and
    // 111 195 metres. Points are (lon, lat) throughout.
    const ONE_DEGREE_NM: f64 = 60.04;
    const ONE_DEGREE_METERS: f64 = 111_195.0;

    #[test]
    fn one_degree_of_latitude_is_sixty_nautical_miles() {
        let d = distance_nm((0.0, 0.0), (0.0, 1.0));
        assert!((d - ONE_DEGREE_NM).abs() < 0.01, "got {d}");
    }

    #[test]
    fn one_degree_of_latitude_in_metres() {
        let d = distance_meters((0.0, 0.0), (0.0, 1.0));
        assert!((d - ONE_DEGREE_METERS).abs() < 1.0, "got {d}");
    }

    #[test]
    fn points_are_lon_lat_so_a_latitude_step_at_any_longitude_is_one_degree() {
        // If the arguments were read as (lat, lon) this would be a one degree
        // step of longitude at 47.5 N, about 40 nautical miles, not 60.
        let d = distance_nm((-122.3, 47.5), (-122.3, 48.5));
        assert!((d - ONE_DEGREE_NM).abs() < 0.01, "got {d}");
    }

    #[test]
    fn distance_is_zero_for_the_same_point_and_symmetric() {
        let a = (-4.5, 48.4);
        let b = (-1.6, 50.1);
        assert_eq!(distance_nm(a, a), 0.0);
        assert!((distance_nm(a, b) - distance_nm(b, a)).abs() < 1e-9);
    }

    #[test]
    fn bearing_follows_the_compass() {
        let origin = (0.0, 0.0);
        assert!(
            (bearing_deg(origin, (0.0, 1.0)) - 0.0).abs() < 1e-9,
            "north"
        );
        assert!(
            (bearing_deg(origin, (1.0, 0.0)) - 90.0).abs() < 1e-9,
            "east"
        );
        assert!(
            (bearing_deg(origin, (0.0, -1.0)) - 180.0).abs() < 1e-9,
            "south"
        );
        assert!(
            (bearing_deg(origin, (-1.0, 0.0)) - 270.0).abs() < 1e-9,
            "west"
        );
    }

    #[test]
    fn distance_and_bearing_agree_with_the_separate_functions() {
        let a = (-4.5, 48.4);
        let b = (-1.6, 50.1);
        let (d, brg) = distance_bearing_nm(a, b);
        assert_eq!(d, distance_nm(a, b));
        assert_eq!(brg, bearing_deg(a, b));
    }

    // Rhumb line values with R = 3440.065 NM, from the formulas worked by
    // hand in docs/rhumb-magnetic-research-2026-11-26.md, section 3.
    fn close(got: f64, want: f64, tolerance: f64) -> bool {
        (got - want).abs() < tolerance
    }

    #[test]
    fn a_rhumb_line_along_a_meridian_is_sixty_nautical_miles_per_degree() {
        let (a, b) = ((0.0, 0.0), (0.0, 1.0));
        assert!(close(rhumb_distance_nm(a, b).unwrap(), 60.0405, 1e-3));
        assert!(close(rhumb_bearing_deg(a, b).unwrap(), 0.0, 1e-9));
    }

    #[test]
    fn a_rhumb_line_along_a_parallel_shrinks_with_the_cosine_of_latitude() {
        // 60.0405 * cos 60 = 30.0202: an east-west leg needs the guard.
        let (a, b) = ((0.0, 60.0), (1.0, 60.0));
        assert!(close(rhumb_distance_nm(a, b).unwrap(), 30.0202, 1e-3));
        assert!(close(rhumb_bearing_deg(a, b).unwrap(), 90.0, 1e-9));
    }

    #[test]
    fn a_rhumb_line_across_the_antimeridian_takes_the_short_way() {
        let (a, b) = ((179.5, 0.0), (-179.5, 0.0));
        assert!(close(rhumb_distance_nm(a, b).unwrap(), 60.0405, 1e-3));
        assert!(close(rhumb_bearing_deg(a, b).unwrap(), 90.0, 1e-9));
        assert!(close(rhumb_bearing_deg(b, a).unwrap(), 270.0, 1e-9));
    }

    #[test]
    fn diagonal_rhumb_lines_match_the_worked_values() {
        let (a, b) = ((0.0, 0.0), (1.0, 1.0));
        assert!(close(rhumb_distance_nm(a, b).unwrap(), 84.9079, 1e-3));
        assert!(close(rhumb_bearing_deg(a, b).unwrap(), 44.9985, 1e-3));
        let (a, b) = ((-4.5, 48.4), (-1.6, 50.1));
        assert!(close(rhumb_distance_nm(a, b).unwrap(), 152.7495, 1e-3));
        assert!(close(rhumb_bearing_deg(a, b).unwrap(), 48.0709, 1e-3));
    }

    #[test]
    fn a_rhumb_line_from_a_pole_has_no_value() {
        assert_eq!(rhumb_distance_nm((0.0, 90.0), (0.0, 80.0)), None);
        assert_eq!(rhumb_bearing_deg((0.0, 80.0), (0.0, -90.0)), None);
        assert_eq!(rhumb_point((0.0, 90.0), (0.0, 80.0), 0.5), None);
    }

    #[test]
    fn a_point_on_a_rhumb_line_keeps_the_bearing_and_splits_the_distance() {
        let (a, b) = ((-4.5, 48.4), (-1.6, 50.1));
        let m = rhumb_point(a, b, 0.5).unwrap();
        // Half the latitude, and on the same constant bearing both sides.
        assert!(close(m.1, 49.25, 1e-9));
        let full = rhumb_bearing_deg(a, b).unwrap();
        assert!(close(rhumb_bearing_deg(a, m).unwrap(), full, 1e-6));
        assert!(close(rhumb_bearing_deg(m, b).unwrap(), full, 1e-6));
        assert!(close(
            rhumb_distance_nm(a, m).unwrap(),
            152.7495 / 2.0,
            1e-3
        ));
        assert_eq!(rhumb_point(a, b, 0.0), Some(a));
    }

    #[test]
    fn a_point_on_a_parallel_across_the_antimeridian_stays_in_range() {
        let m = rhumb_point((179.5, 10.0), (-179.5, 10.0), 0.75).unwrap();
        assert!(close(m.0, -179.75, 1e-9), "{m:?}");
        assert!(close(m.1, 10.0, 1e-9));
    }

    #[test]
    fn antipodal_points_do_not_produce_nan() {
        let d = distance_nm((0.0, 0.0), (180.0, 0.0));
        assert!(d.is_finite());
        assert!((d - 180.0 * ONE_DEGREE_NM).abs() < 1.0, "got {d}");
    }
}
