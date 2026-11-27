//! Magnetic variation from the World Magnetic Model (WMM2025).
//!
//! Notice under 17 U.S.C. 403: the WMM2025 coefficients in
//! `magnetic/WMM.COF` are a work of the US Government (NOAA National
//! Centers for Environmental Information), not subject to copyright
//! protection in the United States (17 U.S.C. 105).
//!
//! Variation (declination) is in degrees, east positive:
//! `magnetic = true - variation`. Design: `docs/magnetic-and-rhumb-design.md`.
//! The equations are those of the WMM2025 technical report (NOAA NCEI).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// The coefficient file as NOAA publishes it.
const COEFFICIENTS: &str = include_str!("magnetic/WMM.COF");
/// The model's degree and order.
const DEGREE: usize = 12;
/// The first and last valid years: 2025.0 up to the end of 2029.
const EPOCH: f64 = 2025.0;
const MODEL_END: f64 = 2030.0;
/// Below this horizontal field (nT) NOAA calls the declination unreliable:
/// the Blackout Zone around each magnetic pole.
const BLACKOUT_NT: f64 = 2000.0;
/// The geomagnetic reference radius (km).
const REFERENCE_RADIUS: f64 = 6371.2;
/// WGS 84 semi-major axis (km) and flattening.
const WGS84_A: f64 = 6378.137;
const WGS84_F: f64 = 1.0 / 298.257_223_563;

/// How a date relates to the years the model covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    Valid,
    /// After 2029: the model is used, and marked expired.
    Expired,
    /// Before 2025: the clock is wrong, and the model uses its first day.
    BeforeModel,
}

pub fn status(year: f64) -> Status {
    if year < EPOCH {
        Status::BeforeModel
    } else if year >= MODEL_END {
        Status::Expired
    } else {
        Status::Valid
    }
}

/// The WARN to log at start when the clock (`year`) is outside the years
/// of the model, in French as every log line. `None` inside them.
pub fn start_warning(year: f64) -> Option<String> {
    match status(year) {
        Status::Valid => None,
        Status::Expired => Some(format!(
            "Modèle magnétique WMM2025 périmé (valable jusqu'à fin 2029, horloge {year:.1}) : la variation reste calculée, marquée « WMM expired »"
        )),
        Status::BeforeModel => Some(format!(
            "Horloge système en {year:.1}, avant le modèle WMM2025 : la variation utilise le 1er janvier 2025 jusqu'à une date GPS"
        )),
    }
}

/// Logs `start_warning` the first time a date in use is outside the model:
/// a GPS date after a week rollover, or a clock past 2029.
pub fn warn_once_outside_model(year: f64) {
    static WARNED: AtomicBool = AtomicBool::new(false);
    if let Some(warning) = start_warning(year) {
        if !WARNED.swap(true, Ordering::Relaxed) {
            crate::diagnostics::record("WARN", warning);
        }
    }
}

/// `date` as a decimal year: 2025-01-01 is 2025.0.
pub fn decimal_year(date: chrono::NaiveDate) -> f64 {
    use chrono::Datelike;
    let days = if date.leap_year() { 366.0 } else { 365.0 };
    f64::from(date.year()) + f64::from(date.ordinal0()) / days
}

/// The variation in degrees (east positive) at sea level at `position`
/// (`(lon, lat)`) and `year` (decimal). `None` in the Blackout Zone near a
/// magnetic pole. A year before the model uses its first day.
pub fn variation(position: (f64, f64), year: f64) -> Option<f64> {
    let field = field(position, 0.0, year.max(EPOCH));
    (field.horizontal >= BLACKOUT_NT).then_some(field.declination)
}

/// Where the variation in use comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Source {
    /// The GPS or the Signal K server sends it.
    Live,
    /// The magnetic model, inside its years.
    Model,
    /// The magnetic model, after its last year.
    ModelExpired,
}

impl Source {
    /// The name in the bottom bar.
    pub fn label(self) -> &'static str {
        match self {
            Source::Live => "GPS",
            Source::Model => "WMM",
            Source::ModelExpired => "WMM expired",
        }
    }
}

/// A variation in degrees, east positive, and where it comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Variation {
    pub degrees: f64,
    pub source: Source,
}

/// The variation to use: the `live` one when a source sends it (and it is
/// not stale), else the model's at `position` and `year`. `None` with no
/// position, or near a magnetic pole.
pub fn in_use(live: Option<f64>, position: Option<(f64, f64)>, year: f64) -> Option<Variation> {
    if let Some(degrees) = live {
        return Some(Variation {
            degrees,
            source: Source::Live,
        });
    }
    let degrees = variation(position?, year)?;
    let source = match status(year) {
        Status::Expired => Source::ModelExpired,
        Status::Valid | Status::BeforeModel => Source::Model,
    };
    Some(Variation { degrees, source })
}

/// A true bearing in degrees as the navigator reads it: magnetic with an M
/// when a `variation` is given (`magnetic = true - variation`), else true
/// with a T. Three digits, `000` to `359`.
pub fn bearing_text(true_degrees: f64, variation: Option<f64>) -> String {
    let (degrees, mark) = match variation {
        Some(variation) => (true_degrees - variation, 'M'),
        None => (true_degrees, 'T'),
    };
    let whole = (degrees.rem_euclid(360.0).round() as u32) % 360;
    format!("{whole:03}°{mark}")
}

/// A variation in degrees and minutes with its side: `3°06'W`.
pub fn variation_text(degrees: f64) -> String {
    let minutes = (degrees.abs() * 60.0).round() as u32;
    let side = if degrees < 0.0 { 'W' } else { 'E' };
    format!("{}°{:02}'{side}", minutes / 60, minutes % 60)
}

/// The bottom bar's variation: `Var 3°06'W (GPS)`, or `Var --` when there
/// is none (near a magnetic pole).
pub fn bar_text(variation: Option<Variation>) -> String {
    match variation {
        Some(v) => format!("Var {} ({})", variation_text(v.degrees), v.source.label()),
        None => "Var --".to_string(),
    }
}

/// The part of the field the plotter needs.
#[derive(Debug, Clone, Copy)]
struct Field {
    /// Degrees, east positive.
    declination: f64,
    /// Horizontal intensity, nT.
    horizontal: f64,
}

/// Gauss coefficients and their yearly change, indexed by `index(n, m)`.
struct Model {
    g: Vec<f64>,
    h: Vec<f64>,
    g_dot: Vec<f64>,
    h_dot: Vec<f64>,
}

fn index(n: usize, m: usize) -> usize {
    n * (n + 1) / 2 + m
}

/// The coefficients, read once from the built-in file. The file is part
/// of the binary and its test passes, so a bad line is a build defect.
fn model() -> &'static Model {
    static MODEL: OnceLock<Model> = OnceLock::new();
    MODEL.get_or_init(|| {
        let size = index(DEGREE, DEGREE) + 1;
        let mut model = Model {
            g: vec![0.0; size],
            h: vec![0.0; size],
            g_dot: vec![0.0; size],
            h_dot: vec![0.0; size],
        };
        // The first line is the epoch and name; the file ends with a line of 9s.
        for line in COEFFICIENTS.lines().skip(1) {
            let values: Vec<f64> = line
                .split_whitespace()
                .filter_map(|v| v.parse().ok())
                .collect();
            let [n, m, g, h, g_dot, h_dot] = values[..] else {
                continue;
            };
            let (n, m) = (n as usize, m as usize);
            if n > DEGREE || m > n {
                continue;
            }
            let i = index(n, m);
            (model.g[i], model.h[i], model.g_dot[i], model.h_dot[i]) = (g, h, g_dot, h_dot);
        }
        model
    })
}

/// The field at `position` (`(lon, lat)`, geodetic degrees), `height` km
/// above the WGS 84 ellipsoid, at `year`.
fn field(position: (f64, f64), height: f64, year: f64) -> Field {
    let model = model();
    let (lon, lat) = (position.0.to_radians(), position.1.to_radians());
    let t = year - EPOCH;

    // Geodetic to geocentric spherical coordinates.
    let e2 = WGS84_F * (2.0 - WGS84_F);
    let rc = WGS84_A / (1.0 - e2 * lat.sin().powi(2)).sqrt();
    let p = (rc + height) * lat.cos();
    let z = (rc * (1.0 - e2) + height) * lat.sin();
    let r = p.hypot(z);
    let lat_c = (z / r).asin();

    // Schmidt semi-normalised associated Legendre functions of the
    // geocentric colatitude, and their derivatives by colatitude.
    let (x, s) = (lat_c.sin(), lat_c.cos());
    let size = index(DEGREE, DEGREE) + 1;
    let (mut p_nm, mut dp_nm) = (vec![0.0; size], vec![0.0; size]);
    p_nm[0] = 1.0;
    for n in 1..=DEGREE {
        for m in 0..=n {
            let i = index(n, m);
            if n == m {
                let j = index(n - 1, m - 1);
                p_nm[i] = s * p_nm[j];
                dp_nm[i] = s * dp_nm[j] + x * p_nm[j];
            } else {
                let j = index(n - 1, m);
                let (p2, dp2, k) = if n >= 2 && m <= n - 2 {
                    let k = ((n - 1) * (n - 1) - m * m) as f64 / ((2 * n - 1) * (2 * n - 3)) as f64;
                    (p_nm[index(n - 2, m)], dp_nm[index(n - 2, m)], k)
                } else {
                    (0.0, 0.0, 0.0)
                };
                p_nm[i] = x * p_nm[j] - k * p2;
                dp_nm[i] = x * dp_nm[j] - s * p_nm[j] - k * dp2;
            }
        }
    }
    // The recursion gives Gauss-normalised values: scale them to Schmidt.
    let mut schmidt = vec![0.0; size];
    schmidt[0] = 1.0;
    for n in 1..=DEGREE {
        schmidt[index(n, 0)] = schmidt[index(n - 1, 0)] * (2 * n - 1) as f64 / n as f64;
        for m in 1..=n {
            let two = if m == 1 { 2.0 } else { 1.0 };
            schmidt[index(n, m)] =
                schmidt[index(n, m - 1)] * ((n - m + 1) as f64 * two / (n + m) as f64).sqrt();
        }
    }

    // North, east and down components in the geocentric frame.
    let (mut north, mut east, mut down) = (0.0, 0.0, 0.0);
    let ratio = REFERENCE_RADIUS / r;
    for n in 1..=DEGREE {
        let scale = ratio.powi(n as i32 + 2);
        for m in 0..=n {
            let i = index(n, m);
            let g = model.g[i] + t * model.g_dot[i];
            let h = model.h[i] + t * model.h_dot[i];
            let (sin_ml, cos_ml) = (m as f64 * lon).sin_cos();
            let p = schmidt[i] * p_nm[i];
            let dp = schmidt[i] * dp_nm[i];
            let gh = g * cos_ml + h * sin_ml;
            north += scale * gh * dp;
            east += scale * m as f64 * (g * sin_ml - h * cos_ml) * p;
            down -= scale * (n as f64 + 1.0) * gh * p;
        }
    }
    // At a geographic pole the east component divides by zero; the
    // declination there means nothing and the blackout test removes it.
    east /= s;

    // Rotate from the geocentric to the geodetic frame.
    let tilt = lat_c - lat;
    let north = north * tilt.cos() - down * tilt.sin();
    Field {
        declination: east.atan2(north).to_degrees(),
        horizontal: north.hypot(east),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The official NOAA test file: year, height (km), latitude, longitude,
    /// declination (degrees), ..., H (nT) in column 7.
    const TEST_VALUES: &str = include_str!("magnetic/WMM2025_TestValues.txt");

    fn rows() -> Vec<[f64; 7]> {
        TEST_VALUES
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
            .map(|line| {
                let values: Vec<f64> = line
                    .split_whitespace()
                    .take(7)
                    .map(|v| v.parse().unwrap())
                    .collect();
                values.try_into().unwrap()
            })
            .collect()
    }

    #[test]
    fn the_model_matches_every_official_noaa_test_value() {
        let rows = rows();
        assert_eq!(rows.len(), 100);
        for [year, height, lat, lon, declination, _, horizontal] in rows {
            let field = field((lon, lat), height, year);
            assert!(
                (field.declination - declination).abs() < 0.01,
                "{year} {height} km {lat} {lon}: got {} want {declination}",
                field.declination
            );
            assert!(
                (field.horizontal - horizontal).abs() < 1.0,
                "{year} {height} km {lat} {lon}: H {} want {horizontal}",
                field.horizontal
            );
        }
    }

    #[test]
    fn near_the_magnetic_poles_there_is_no_variation() {
        // H is 1504 nT here (NOAA row 1), under the 2000 nT limit.
        assert_eq!(variation((-121.0, 89.0), 2025.0), None);
        // The next row has H of 2164 nT (at 48 km), outside the zone.
        assert!(variation((-96.0, 80.0), 2025.0).is_some());
    }

    #[test]
    fn variation_at_sea_level_has_the_sign_of_the_compass() {
        // NOAA row: 2027.5, 0 km, 13 S 59 W, declination -17.49 (west).
        let v = variation((-59.0, -13.0), 2027.5).unwrap();
        assert!((v - -17.49).abs() < 0.01, "{v}");
    }

    #[test]
    fn a_date_before_the_model_uses_its_first_day() {
        // A clock that reads 1970 after a power cut.
        let at_epoch = variation((-59.0, -13.0), 2025.0).unwrap();
        assert_eq!(variation((-59.0, -13.0), 1970.0), Some(at_epoch));
        assert_eq!(status(1970.0), Status::BeforeModel);
        assert_eq!(status(2025.0), Status::Valid);
        assert_eq!(status(2029.99), Status::Valid);
        assert_eq!(status(2030.0), Status::Expired);
    }

    #[test]
    fn a_live_variation_wins_over_the_model() {
        let v = in_use(Some(-3.1), Some((-59.0, -13.0)), 2027.5).unwrap();
        assert_eq!((v.degrees, v.source), (-3.1, Source::Live));
    }

    #[test]
    fn without_a_live_variation_the_model_gives_it() {
        let v = in_use(None, Some((-59.0, -13.0)), 2027.5).unwrap();
        assert!((v.degrees - -17.49).abs() < 0.01);
        assert_eq!(v.source, Source::Model);
        let v = in_use(None, Some((-59.0, -13.0)), 2031.0).unwrap();
        assert_eq!(v.source, Source::ModelExpired);
    }

    #[test]
    fn with_no_position_or_near_a_pole_there_is_no_variation() {
        assert!(in_use(None, None, 2027.5).is_none());
        assert!(in_use(None, Some((-121.0, 89.0)), 2025.0).is_none());
    }

    #[test]
    fn a_bearing_shows_true_or_magnetic_with_its_mark() {
        assert_eq!(bearing_text(45.0, None), "045°T");
        // magnetic = true - variation: 3 degrees west adds 3.
        assert_eq!(bearing_text(45.0, Some(-3.0)), "048°M");
        assert_eq!(bearing_text(2.0, Some(5.0)), "357°M");
        assert_eq!(bearing_text(359.8, None), "000°T");
    }

    #[test]
    fn the_variation_reads_in_degrees_and_minutes_with_its_side() {
        assert_eq!(variation_text(-3.1), "3°06'W");
        assert_eq!(variation_text(2.0), "2°00'E");
        assert_eq!(variation_text(-0.9999), "1°00'W");
        let gps = Variation {
            degrees: -3.1,
            source: Source::Live,
        };
        assert_eq!(bar_text(Some(gps)), "Var 3°06'W (GPS)");
        let expired = Variation {
            degrees: 1.9,
            source: Source::ModelExpired,
        };
        assert_eq!(bar_text(Some(expired)), "Var 1°54'E (WMM expired)");
        assert_eq!(bar_text(None), "Var --");
    }

    #[test]
    fn the_start_warns_once_when_the_clock_is_outside_the_model() {
        assert_eq!(start_warning(2027.0), None);
        let expired = start_warning(2030.5).unwrap();
        assert!(
            expired.contains("WMM2025") && expired.contains("2029"),
            "{expired}"
        );
        let early = start_warning(1970.0).unwrap();
        assert!(early.contains("1970"), "{early}");
    }

    #[test]
    fn a_date_after_the_model_still_gives_a_value() {
        assert!(variation((-59.0, -13.0), 2031.0).is_some());
    }

    #[test]
    fn the_decimal_year_counts_days_from_the_first_of_january() {
        let date = |y, m, d| chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(decimal_year(date(2025, 1, 1)), 2025.0);
        // 2028 is a leap year: 1 July is day 183, so 182 days have passed.
        assert!((decimal_year(date(2028, 7, 1)) - (2028.0 + 182.0 / 366.0)).abs() < 1e-12);
    }
}
