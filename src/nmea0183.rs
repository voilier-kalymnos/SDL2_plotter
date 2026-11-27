//! NMEA 0183 input: the decoder turns one sentence into a `BoatState`
//! update. Only RMC (position, speed, course), HDT and HDG (heading) are
//! read; see `docs/nmea0183-input-design.md`.

mod aivdm;
mod transport;

use std::time::{Duration, Instant};

use std::collections::HashMap;

use crate::ais::AisTarget;
use crate::boat::{BoatState, FixGuard, HEADING_TIMEOUT};
use crate::config::{Nmea0183Config, Transport};
use crate::feed::LiveFeed;

/// With no RMC for this long, GGA and GLL give the position and VTG the
/// speed and course.
const RMC_SILENCE: Duration = Duration::from_secs(5);

/// 1 knot = 1852 m per hour.
const METRES_PER_SECOND_PER_KNOT: f64 = 1852.0 / 3600.0;

/// The NMEA 0183 source task: reads lines from the configured transport,
/// decodes them and publishes on `feed`. Never returns.
pub async fn run(config: Nmea0183Config, feed: LiveFeed) {
    crate::diagnostics::record(
        "INFO",
        format!(
            "Tâche NMEA 0183 démarrée, {:?} {}",
            config.transport, config.address
        ),
    );
    let mut decoder = Decoder::with_talker(&config.talker);
    let mut last_bound = Instant::now();
    let on_line = |line: &str| {
        let now = Instant::now();
        // A line that makes the decoder panic is dropped, not the source.
        let Some(update) = crate::diagnostics::survive(|| decoder.apply(line, now)) else {
            return;
        };
        if update.boat_changed {
            let _ = feed.boat.send(decoder.boat());
        }
        if let Some(position) = update.track_point {
            let _ = feed.track.send(position);
        }
        // Silent targets, and the oldest above the cap, go at most once a
        // second, so the map stays bounded.
        let mut ais_changed = update.ais_changed;
        if now.duration_since(last_bound) >= std::time::Duration::from_secs(1) {
            ais_changed |= decoder.bound_ais(now);
            last_bound = now;
        }
        if ais_changed {
            let _ = feed.ais.send(decoder.ais_targets());
        }
    };
    match config.transport {
        Transport::Tcp => transport::tcp(&config.address, on_line).await,
        Transport::Udp => transport::udp(&config.address, on_line).await,
    }
}

/// What one sentence changed.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Update {
    /// The boat state changed: publish `Decoder::boat()`.
    pub boat_changed: bool,
    /// A new fix: one track point, `(lon, lat)`.
    pub track_point: Option<(f64, f64)>,
    /// An AIS target changed: publish `Decoder::ais_targets()`.
    pub ais_changed: bool,
}

/// The boat as the sentences so far describe it.
pub struct Decoder {
    boat: BoatState,
    /// When the last HDT came. HDG is ignored while it is fresh.
    hdt_at: Option<Instant>,
    /// When set, position sentences (RMC, GGA, GLL, VTG) from other
    /// talkers are ignored.
    talker: Option<String>,
    /// When the last RMC came. GGA, GLL and VTG are used only without one.
    rmc_at: Option<Instant>,
    /// Keeps impossible jumps (a second GPS) off the chart.
    guard: FixGuard,
    /// AIS fragments waiting for the rest of their message.
    assembler: aivdm::Assembler,
    /// AIS targets by context (`vessels.urn:mrn:imo:mmsi:<MMSI>`, as Signal
    /// K names them).
    targets: HashMap<String, AisTarget>,
}

impl Decoder {
    pub fn new() -> Self {
        Self {
            boat: BoatState::default(),
            hdt_at: None,
            talker: None,
            rmc_at: None,
            guard: FixGuard::default(),
            assembler: aivdm::Assembler::default(),
            targets: HashMap::new(),
        }
    }

    /// A decoder that reads RMC from `talker` only; an empty `talker`
    /// reads every one.
    pub fn with_talker(talker: &str) -> Self {
        Self {
            talker: (!talker.is_empty()).then(|| talker.to_string()),
            ..Self::new()
        }
    }

    pub fn boat(&self) -> BoatState {
        self.boat
    }

    pub fn ais_targets(&self) -> Vec<AisTarget> {
        self.targets.values().cloned().collect()
    }

    /// Applies `ais::bound`. Returns true when a target was removed.
    pub fn bound_ais(&mut self, now: Instant) -> bool {
        crate::ais::bound(&mut self.targets, now)
    }

    /// Reads one line received at `now`. A line that fails a check (no
    /// checksum, bad checksum, a field out of range) changes nothing.
    pub fn apply(&mut self, line: &str, now: Instant) -> Update {
        let Some(body) = checked_body(line) else {
            return Update::default();
        };
        let fields: Vec<&str> = body.split(',').collect();
        // The talker (first two characters) is ignored, except by the
        // optional RMC filter.
        match fields[0].get(2..) {
            Some("RMC") if self.talker_allowed(fields[0]) => self.rmc(&fields, now),
            // Fallbacks for a device that sends no RMC.
            Some("GGA") if self.talker_allowed(fields[0]) && self.rmc_silent(now) => {
                self.gga(&fields, now)
            }
            Some("GLL") if self.talker_allowed(fields[0]) && self.rmc_silent(now) => {
                self.gll(&fields, now)
            }
            Some("VTG") if self.talker_allowed(fields[0]) && self.rmc_silent(now) => {
                self.vtg(&fields)
            }
            Some("HDT") => {
                let update = self.heading(field(&fields, 1).and_then(degrees), now);
                if update.boat_changed {
                    self.hdt_at = Some(now);
                    self.boat.magnetic_heading = None;
                }
                update
            }
            Some("VDM") => self.ais(&fields),
            Some("HDG") => {
                // The variation is kept even when HDT gives the heading.
                if let Some(variation) = offset(field(&fields, 4), field(&fields, 5)) {
                    self.set_variation(variation, now);
                }
                if self.hdt_is_fresh(now) {
                    return Update::default();
                }
                // Deviation is optional; when it is given it must be valid.
                let deviation = match field(&fields, 2) {
                    None => Some(0.0),
                    deviation => offset(deviation, field(&fields, 3)),
                };
                let Some(heading) = field(&fields, 1)
                    .and_then(degrees)
                    .zip(deviation)
                    .map(|(magnetic, deviation)| (magnetic + deviation).rem_euclid(360.0))
                else {
                    return Update::default();
                };
                // The session adds the variation (live, else the model).
                self.boat.magnetic_heading = Some(heading.to_radians());
                self.boat.heading = None;
                self.boat.heading_at = Some(now);
                Update {
                    boat_changed: true,
                    ..Update::default()
                }
            }
            _ => Update::default(),
        }
    }

    fn rmc(&mut self, fields: &[&str], now: Instant) -> Update {
        self.rmc_at = Some(now);
        if let Some(variation) = offset(field(fields, 10), field(fields, 11)) {
            self.set_variation(variation, now);
        }
        if let Some(date) = field(fields, 9).and_then(nmea_date) {
            self.boat.gps_date = Some(date);
        }
        let valid = field(fields, 2) == Some("A") && mode_ok(field(fields, 12));
        let update = self.position(valid, fields, 3, now);
        if update.track_point.is_some() {
            self.motion(field(fields, 7), field(fields, 8));
        }
        update
    }

    fn set_variation(&mut self, degrees: f64, now: Instant) {
        self.boat.variation = Some(degrees);
        self.boat.variation_at = Some(now);
    }

    /// GGA, used while no RMC comes: quality 0 is no fix, 6 is dead
    /// reckoning (no fix either, as RMC mode E).
    fn gga(&mut self, fields: &[&str], now: Instant) -> Update {
        let valid = field(fields, 6).is_some_and(|quality| quality != "0" && quality != "6");
        self.position(valid, fields, 2, now)
    }

    /// GLL, used while no RMC comes: status A and a mode that is not N or E.
    fn gll(&mut self, fields: &[&str], now: Instant) -> Update {
        let valid = field(fields, 6) == Some("A") && mode_ok(field(fields, 7));
        self.position(valid, fields, 1, now)
    }

    /// VTG, used while no RMC comes: true course and speed in knots. The
    /// NMEA 2.3 form has unit letters (`054.7,T,034.4,M,005.5,N,010.2,K`);
    /// the older one has only the four values (`054.7,034.4,005.5,010.2`).
    fn vtg(&mut self, fields: &[&str]) -> Update {
        let (course, speed) = if fields.get(2) == Some(&"T") {
            if !mode_ok(field(fields, 9)) {
                return Update::default();
            }
            let speed = (field(fields, 6) == Some("N"))
                .then(|| field(fields, 5))
                .flatten();
            (field(fields, 1), speed)
        } else if fields.len() == 5 {
            (field(fields, 1), field(fields, 3))
        } else {
            return Update::default();
        };
        if !self.motion(speed, course) {
            return Update::default();
        }
        Update {
            boat_changed: true,
            ..Update::default()
        }
    }

    /// A position from `fields[at..at + 4]` (lat, N/S, lon, E/W). When the
    /// sentence says there is no fix, the fix is lost and the position
    /// kept; a bad coordinate changes nothing.
    fn position(&mut self, valid: bool, fields: &[&str], at: usize, now: Instant) -> Update {
        if !valid {
            self.boat.position_valid = false;
            return Update {
                boat_changed: true,
                ..Update::default()
            };
        }
        let (Some(lat), Some(lon)) = (
            coordinate(field(fields, at), field(fields, at + 1), 2, 'N', 'S'),
            coordinate(field(fields, at + 2), field(fields, at + 3), 3, 'E', 'W'),
        ) else {
            return Update::default();
        };
        if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lon) {
            return Update::default();
        }
        if !self.guard.accept((lon, lat), now) {
            return Update::default();
        }
        self.boat.latitude = lat;
        self.boat.longitude = lon;
        self.boat.position_valid = true;
        self.boat.fixed_at = Some(now);
        Update {
            boat_changed: true,
            track_point: Some((lon, lat)),
            ..Update::default()
        }
    }

    /// Speed in knots and true course in degrees; an empty or bad field
    /// keeps the last value. Returns true when either changed.
    fn motion(&mut self, knots: Option<&str>, course: Option<&str>) -> bool {
        let mut changed = false;
        if let Some(knots) = knots.and_then(number).filter(|knots| *knots >= 0.0) {
            self.boat.speed_over_ground = knots * METRES_PER_SECOND_PER_KNOT;
            changed = true;
        }
        if let Some(course) = course.and_then(degrees) {
            self.boat.course_over_ground = course.to_radians();
            changed = true;
        }
        changed
    }

    /// True when no RMC came for `RMC_SILENCE`: GGA, GLL and VTG are used.
    fn rmc_silent(&self, now: Instant) -> bool {
        self.rmc_at
            .is_none_or(|at| now.saturating_duration_since(at) >= RMC_SILENCE)
    }

    /// One `!AIVDM` fragment. The target is changed when a whole message
    /// is in.
    fn ais(&mut self, fields: &[&str]) -> Update {
        let Some(message) = self
            .assembler
            .push(fields)
            .and_then(|(payload, fill)| aivdm::decode(&payload, fill))
        else {
            return Update::default();
        };
        let (mmsi, report, data, class_b) = match message {
            aivdm::Message::Position(report) => {
                let data = aivdm::StaticData {
                    name: report.name.clone(),
                    ship_type: report.ship_type,
                    ..aivdm::StaticData::default()
                };
                (report.mmsi, Some(report.clone()), data, report.class_b)
            }
            aivdm::Message::Static {
                mmsi,
                data,
                class_b,
            } => (mmsi, None, data, class_b),
        };
        let context = format!("vessels.urn:mrn:imo:mmsi:{mmsi}");
        let target = self
            .targets
            .entry(context.clone())
            .or_insert_with(|| AisTarget::new(context));
        if let Some(report) = report {
            if report.position.is_some() {
                target.position = report.position;
            }
            target.speed_over_ground = report.speed_over_ground;
            target.course_over_ground = report.course_over_ground;
            target.heading = report.heading;
            target.nav_status = report.nav_status;
        }
        // A static report carries part of the data; keep the rest.
        let keep = |new: Option<String>, old: &mut Option<String>| {
            if new.is_some() {
                *old = new;
            }
        };
        keep(data.name, &mut target.name);
        keep(data.call_sign, &mut target.call_sign);
        keep(data.destination, &mut target.destination);
        target.ship_type = data.ship_type.or(target.ship_type);
        target.length_m = data.length_m.or(target.length_m);
        target.beam_m = data.beam_m.or(target.beam_m);
        target.draught_m = data.draught_m.or(target.draught_m);
        target.class_b = class_b;
        target.update_received();
        Update {
            ais_changed: true,
            ..Update::default()
        }
    }

    fn talker_allowed(&self, address: &str) -> bool {
        self.talker
            .as_deref()
            .is_none_or(|talker| address.get(..2) == Some(talker))
    }

    /// HDT is true heading, so it wins over HDG while it comes in.
    fn hdt_is_fresh(&self, now: Instant) -> bool {
        self.hdt_at
            .is_some_and(|at| now.saturating_duration_since(at) < HEADING_TIMEOUT)
    }

    /// A true heading in degrees, or nothing to change.
    fn heading(&mut self, degrees_true: Option<f64>, now: Instant) -> Update {
        let Some(heading) = degrees_true else {
            return Update::default();
        };
        self.boat.heading = Some(heading.rem_euclid(360.0).to_radians());
        self.boat.heading_at = Some(now);
        Update {
            boat_changed: true,
            ..Update::default()
        }
    }
}

/// The text between `$` and `*` when the line is a sentence with a correct
/// checksum. A leading tag block (`\...\`) is skipped; its own checksum
/// is not checked.
fn checked_body(line: &str) -> Option<&str> {
    let mut line = line.trim();
    if let Some(rest) = line.strip_prefix('\\') {
        line = &rest[rest.find('\\')? + 1..];
    }
    // `$` for data, `!` for encapsulated data (AIS).
    let line = line.strip_prefix(['$', '!'])?;
    let (body, checksum) = line.rsplit_once('*')?;
    let expected = hex_byte(checksum)?;
    let actual = body.bytes().fold(0u8, |acc, b| acc ^ b);
    (actual == expected && body.len() >= 5).then_some(body)
}

/// Exactly two hex digits. (`u8::from_str_radix` alone also takes a sign.)
fn hex_byte(text: &str) -> Option<u8> {
    if text.len() != 2 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u8::from_str_radix(text, 16).ok()
}

/// Field `index`, or `None` when it is missing or empty.
fn field<'a>(fields: &[&'a str], index: usize) -> Option<&'a str> {
    fields.get(index).copied().filter(|f| !f.is_empty())
}

fn number(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|n| n.is_finite())
}

/// An angle in degrees, 0..=360.
fn degrees(text: &str) -> Option<f64> {
    number(text).filter(|d| (0.0..=360.0).contains(d))
}

/// A value with its hemisphere letter: negative for `negative`.
fn signed(
    value: Option<&str>,
    letter: Option<&str>,
    positive: char,
    negative: char,
) -> Option<f64> {
    let value = number(value?)?;
    match letter?.chars().next()? {
        c if c == positive => Some(value),
        c if c == negative => Some(-value),
        _ => None,
    }
}

/// A mode indicator (NMEA 2.3 and later) that allows a fix: anything but
/// N (not valid) and E (estimated, dead reckoning). No field: allowed.
fn mode_ok(mode: Option<&str>) -> bool {
    !matches!(mode, Some("N" | "E"))
}

/// An RMC date, `ddmmyy`. Two-digit years from 80 are 1980 to 1999, the
/// rest 2000 to 2079.
fn nmea_date(text: &str) -> Option<chrono::NaiveDate> {
    if text.len() != 6 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let part = |range: std::ops::Range<usize>| text[range].parse::<u32>().ok();
    let (day, month, year) = (part(0..2)?, part(2..4)?, part(4..6)?);
    let year = if year >= 80 { 1900 + year } else { 2000 + year };
    chrono::NaiveDate::from_ymd_opt(year as i32, month, day)
}

/// A magnetic variation or deviation in degrees, east positive, at most
/// 180 either way.
fn offset(value: Option<&str>, letter: Option<&str>) -> Option<f64> {
    signed(value, letter, 'E', 'W').filter(|degrees| degrees.abs() <= 180.0)
}

/// `ddmm.mmmm` (or `dddmm.mmmm` with `degree_digits` 3) and its hemisphere
/// letter, in decimal degrees. Minutes must be below 60.
fn coordinate(
    text: Option<&str>,
    letter: Option<&str>,
    degree_digits: usize,
    positive: char,
    negative: char,
) -> Option<f64> {
    let text = text?;
    if !text.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    let dot = text.find('.').unwrap_or(text.len());
    if dot != degree_digits + 2 {
        return None;
    }
    let degrees: f64 = text[..degree_digits].parse::<u32>().ok()?.into();
    let minutes: f64 = text[degree_digits..].parse().ok()?;
    if minutes >= 60.0 {
        return None;
    }
    let value = degrees + minutes / 60.0;
    signed(Some(&value.to_string()), letter, positive, negative)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// `$body*hh` with the checksum computed here, independently of the
    /// decoder.
    fn sentence(body: &str) -> String {
        let sum = body.bytes().fold(0u8, |acc, b| acc ^ b);
        format!("${body}*{sum:02X}")
    }

    const GPSD_RMC: &str = "$GPRMC,123519,A,4807.038,N,01131.000,E,022.4,084.4,230394,003.1,W*6A";

    fn fixed_decoder(now: Instant) -> Decoder {
        let mut d = Decoder::new();
        d.apply(GPSD_RMC, now);
        d
    }

    #[test]
    fn an_rmc_with_a_fix_gives_position_speed_course_and_a_track_point() {
        let mut d = Decoder::new();
        let now = Instant::now();
        let u = d.apply(GPSD_RMC, now);
        let b = d.boat();
        assert!(u.boat_changed);
        assert!((b.latitude - 48.1173).abs() < 1e-6);
        assert!((b.longitude - 11.516_666_7).abs() < 1e-6);
        assert!((b.speed_over_ground - 22.4 * 0.514_444).abs() < 1e-3);
        assert!((b.course_over_ground - 84.4_f64.to_radians()).abs() < 1e-9);
        assert!(b.position_valid);
        assert_eq!(b.fixed_at, Some(now));
        assert_eq!(u.track_point, Some((b.longitude, b.latitude)));
    }

    #[test]
    fn a_second_gps_far_away_does_not_move_the_boat() {
        let now = Instant::now();
        let mut d = fixed_decoder(now);
        let far = sentence("GPRMC,123520,A,4907.038,N,01131.000,E,022.4,084.4,230394,003.1,W");
        let u = d.apply(&far, now + Duration::from_secs(1));
        assert_eq!(u.track_point, None);
        assert!(
            (d.boat().latitude - 48.1173).abs() < 1e-6,
            "60 NM in 1 s: ignored"
        );
        assert_eq!(d.boat().fixed_at, Some(now), "no new fix time");
    }

    #[test]
    fn status_v_is_no_fix_and_keeps_the_position() {
        let now = Instant::now();
        let mut d = fixed_decoder(now);
        let u = d.apply(
            &sentence("GPRMC,123520,V,4900.000,N,01200.000,E,022.4,084.4,230394,,"),
            now + Duration::from_secs(1),
        );
        let b = d.boat();
        assert!(u.boat_changed);
        assert_eq!(u.track_point, None);
        assert!(!b.position_valid);
        assert!((b.latitude - 48.1173).abs() < 1e-6, "position kept");
        assert_eq!(b.fixed_at, Some(now), "no new fix time");
    }

    #[test]
    fn mode_n_and_mode_e_are_no_fix() {
        for mode in ["N", "E"] {
            let mut d = Decoder::new();
            let u = d.apply(
                &sentence(&format!(
                    "GPRMC,123519,A,4807.038,N,01131.000,E,5.0,90.0,230394,,,{mode}"
                )),
                Instant::now(),
            );
            assert_eq!(u.track_point, None, "mode {mode}");
            assert!(!d.boat().position_valid, "mode {mode}");
            assert_eq!(d.boat().fixed_at, None, "mode {mode}");
        }
        let mut d = Decoder::new();
        d.apply(
            &sentence("GPRMC,123519,A,4807.038,N,01131.000,E,5.0,90.0,230394,,,A"),
            Instant::now(),
        );
        assert!(d.boat().position_valid, "mode A is a fix");
    }

    #[test]
    fn the_simulator_no_fix_rmc_is_accepted() {
        let mut d = Decoder::new();
        let u = d.apply(
            &sentence("GPRMC,120000.00,V,,,,,,,,251126,,,N"),
            Instant::now(),
        );
        assert_eq!(u.track_point, None);
        assert!(!d.boat().position_valid);
    }

    #[test]
    fn a_bad_checksum_is_ignored() {
        let mut d = Decoder::new();
        let bad = GPSD_RMC.replace("*6A", "*6B");
        assert_eq!(d.apply(&bad, Instant::now()), Update::default());
        assert_eq!(d.boat().fixed_at, None);
    }

    #[test]
    fn a_checksum_is_exactly_two_hex_digits() {
        assert_eq!(hex_byte("6A"), Some(0x6A));
        assert_eq!(hex_byte("6a"), Some(0x6A));
        assert_eq!(hex_byte("+A"), None);
        assert_eq!(hex_byte("-0"), None);
        assert_eq!(hex_byte("A"), None);
        assert_eq!(hex_byte("6A0"), None);
    }

    #[test]
    fn a_line_without_a_checksum_is_ignored() {
        let mut d = Decoder::new();
        let bare = GPSD_RMC.trim_end_matches("*6A");
        assert_eq!(d.apply(bare, Instant::now()), Update::default());
    }

    #[test]
    fn line_endings_are_accepted() {
        let mut d = Decoder::new();
        let u = d.apply(&format!("{GPSD_RMC}\r\n"), Instant::now());
        assert!(u.track_point.is_some());
    }

    #[test]
    fn a_tag_block_is_skipped() {
        let mut d = Decoder::new();
        let u = d.apply(
            &format!("\\s:sim,c:1764000000*00\\{GPSD_RMC}"),
            Instant::now(),
        );
        assert!(u.track_point.is_some());
    }

    #[test]
    fn the_talker_is_ignored() {
        for talker in ["GN", "II"] {
            let mut d = Decoder::new();
            let u = d.apply(
                &sentence(&format!(
                    "{talker}RMC,123519,A,4807.038,N,01131.000,E,022.4,084.4,230394,003.1,W"
                )),
                Instant::now(),
            );
            assert!(u.track_point.is_some(), "talker {talker}");
        }
    }

    #[test]
    fn a_talker_filter_keeps_only_that_receivers_position() {
        let mut d = Decoder::with_talker("GN");
        let now = Instant::now();
        let gp = sentence("GPRMC,123519,A,4807.038,N,01131.000,E,5.0,90.0,230394,,");
        assert_eq!(d.apply(&gp, now), Update::default(), "GP filtered out");
        let gn = sentence("GNRMC,123519,A,4807.038,N,01131.000,E,5.0,90.0,230394,,");
        assert!(d.apply(&gn, now).track_point.is_some());
        let hdt = sentence("HEHDT,10.0,T");
        assert!(d.apply(&hdt, now).boat_changed, "heading is not filtered");
    }

    #[test]
    fn extra_fields_are_accepted() {
        let mut d = Decoder::new();
        let u = d.apply(
            &sentence("GPRMC,123519,A,4807.038,N,01131.000,E,5.0,90.0,230394,,,A,V,extra"),
            Instant::now(),
        );
        assert!(u.track_point.is_some());
    }

    #[test]
    fn out_of_range_or_bad_coordinates_are_ignored() {
        for body in [
            "GPRMC,123519,A,9100.000,N,01131.000,E,5.0,90.0,230394,,",
            "GPRMC,123519,A,4860.000,N,01131.000,E,5.0,90.0,230394,,",
            "GPRMC,123519,A,4807.038,X,01131.000,E,5.0,90.0,230394,,",
            "GPRMC,123519,A,4807.038,N,18100.000,E,5.0,90.0,230394,,",
            "GPRMC,123519,A,48x7.038,N,01131.000,E,5.0,90.0,230394,,",
            "GPRMC,123519,A,,N,01131.000,E,5.0,90.0,230394,,",
        ] {
            let mut d = Decoder::new();
            let u = d.apply(&sentence(body), Instant::now());
            assert_eq!(u.track_point, None, "{body}");
            assert_eq!(d.boat().fixed_at, None, "{body}");
        }
    }

    #[test]
    fn south_and_west_are_negative() {
        let mut d = Decoder::new();
        d.apply(
            &sentence("GPRMC,123519,A,3330.000,S,07030.000,W,5.0,90.0,230394,,"),
            Instant::now(),
        );
        let b = d.boat();
        assert!((b.latitude + 33.5).abs() < 1e-9);
        assert!((b.longitude + 70.5).abs() < 1e-9);
    }

    #[test]
    fn empty_course_and_speed_keep_the_last_values() {
        let now = Instant::now();
        let mut d = fixed_decoder(now);
        d.apply(
            &sentence("GPRMC,123520,A,4807.040,N,01131.000,E,,,230394,,"),
            now,
        );
        let b = d.boat();
        assert!((b.speed_over_ground - 22.4 * 0.514_444).abs() < 1e-3);
        assert!((b.course_over_ground - 84.4_f64.to_radians()).abs() < 1e-9);
    }

    #[test]
    fn hdt_gives_a_true_heading() {
        let mut d = Decoder::new();
        let now = Instant::now();
        let u = d.apply(&sentence("HEHDT,274.07,T"), now);
        assert!(u.boat_changed);
        assert_eq!(u.track_point, None);
        let b = d.boat();
        assert!((b.heading.unwrap() - 274.07_f64.to_radians()).abs() < 1e-9);
        assert_eq!(b.heading_at, Some(now));
        assert_eq!(b.fixed_at, None, "a heading is not a fix");
    }

    /// The magnetic heading in degrees, deviation applied.
    fn magnetic(d: &Decoder) -> f64 {
        d.boat().magnetic_heading.unwrap().to_degrees()
    }

    #[test]
    fn hdt_wins_over_hdg_while_it_is_fresh() {
        let mut d = Decoder::new();
        let now = Instant::now();
        d.apply(&sentence("HEHDT,10.0,T"), now);
        let u = d.apply(
            &sentence("HCHDG,100.0,,,0.0,E"),
            now + Duration::from_secs(1),
        );
        assert_eq!(u, Update::default());
        assert!((d.boat().heading.unwrap() - 10.0_f64.to_radians()).abs() < 1e-9);
        assert_eq!(d.boat().magnetic_heading, None);
        let later = now + crate::boat::HEADING_TIMEOUT;
        d.apply(&sentence("HCHDG,100.0,,,0.0,E"), later);
        assert!(
            (magnetic(&d) - 100.0).abs() < 1e-9,
            "HDG once HDT has stopped"
        );
        assert_eq!(d.boat().heading, None, "the old HDT heading is gone");
        assert_eq!(d.boat().heading_at, Some(later));
        d.apply(&sentence("HEHDT,20.0,T"), later);
        assert_eq!(d.boat().magnetic_heading, None, "and back to HDT");
    }

    #[test]
    fn hdg_variation_is_kept_while_hdt_gives_the_heading() {
        let mut d = Decoder::new();
        let now = Instant::now();
        d.apply(&sentence("HEHDT,10.0,T"), now);
        d.apply(&sentence("HCHDG,100.0,,,4.0,E"), now);
        assert_eq!(d.boat().variation, Some(4.0));
        assert!((d.boat().heading.unwrap() - 10.0_f64.to_radians()).abs() < 1e-9);
    }

    #[test]
    fn hdg_applies_its_deviation() {
        let mut d = Decoder::new();
        d.apply(&sentence("HCHDG,100.0,2.0,E,5.0,W"), Instant::now());
        assert!((magnetic(&d) - 102.0).abs() < 1e-9);
        d.apply(&sentence("HCHDG,100.0,2.0,W,5.0,W"), Instant::now());
        assert!((magnetic(&d) - 98.0).abs() < 1e-9);
    }

    #[test]
    fn a_variation_or_deviation_beyond_180_degrees_is_ignored() {
        let mut d = Decoder::new();
        d.apply(&sentence("HCHDG,100.0,,,200.0,W"), Instant::now());
        assert_eq!(d.boat().variation, None);
        let u = d.apply(&sentence("HCHDG,100.0,190.0,E,5.0,W"), Instant::now());
        assert_eq!(u, Update::default());
        let mut d = Decoder::new();
        d.apply(
            &sentence("GPRMC,123519,A,4807.038,N,01131.000,E,5.0,90.0,230394,200.0,W"),
            Instant::now(),
        );
        assert_eq!(d.boat().variation, None, "the RMC variation was not kept");
    }

    #[test]
    fn hdg_publishes_its_own_variation_with_its_time() {
        let mut d = Decoder::new();
        let now = Instant::now();
        d.apply(&sentence("HCHDG,100.0,,,5.0,W"), now);
        assert_eq!(d.boat().variation, Some(-5.0));
        assert_eq!(d.boat().variation_at, Some(now));
        assert!((magnetic(&d) - 100.0).abs() < 1e-9);
        assert_eq!(d.boat().heading, None, "the session adds the variation");
    }

    #[test]
    fn rmc_publishes_its_variation_and_date() {
        // GPSD_RMC has 003.1,W and the date 23 March 1994.
        let now = Instant::now();
        let d = fixed_decoder(now);
        assert_eq!(d.boat().variation, Some(-3.1));
        assert_eq!(d.boat().variation_at, Some(now));
        assert_eq!(
            d.boat().gps_date,
            chrono::NaiveDate::from_ymd_opt(1994, 3, 23)
        );
    }

    #[test]
    fn hdg_without_any_variation_still_gives_a_magnetic_heading() {
        let mut d = Decoder::new();
        let u = d.apply(&sentence("HCHDG,100.0,,,,"), Instant::now());
        assert!(u.boat_changed);
        assert!((magnetic(&d) - 100.0).abs() < 1e-9);
        assert_eq!(d.boat().variation, None);
    }

    const GGA: &str = "GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,";
    const GLL: &str = "GPGLL,4916.45,N,12311.12,W,225444,A,A";
    const VTG: &str = "GPVTG,054.7,T,034.4,M,005.5,N,010.2,K,A";

    #[test]
    fn gga_gll_and_vtg_are_ignored_while_rmc_comes() {
        let now = Instant::now();
        let mut d = fixed_decoder(now);
        let soon = now + Duration::from_secs(4);
        for body in [GGA, GLL, VTG] {
            assert_eq!(d.apply(&sentence(body), soon), Update::default(), "{body}");
        }
        assert_eq!(d.boat().fixed_at, Some(now));
    }

    #[test]
    fn gga_gives_the_fix_when_no_rmc_comes() {
        let now = Instant::now();
        let mut d = Decoder::new();
        let u = d.apply(&sentence(GGA), now);
        let b = d.boat();
        assert!(b.position_valid);
        assert_eq!(b.fixed_at, Some(now));
        assert!((b.latitude - 48.1173).abs() < 1e-6);
        assert!((b.longitude - 11.516_666_7).abs() < 1e-6);
        assert_eq!(u.track_point, Some((b.longitude, b.latitude)));
    }

    #[test]
    fn the_fallback_starts_five_seconds_after_the_last_rmc() {
        let now = Instant::now();
        let mut d = fixed_decoder(now);
        let late = now + RMC_SILENCE;
        // Near the RMC fix: a far one would be rejected as a jump.
        let near = sentence("GPGLL,4807.050,N,01131.000,E,123524,A,A");
        let u = d.apply(&near, late);
        assert!(u.track_point.is_some());
        let b = d.boat();
        assert!((b.latitude - (48.0 + 7.05 / 60.0)).abs() < 1e-9);
        assert!((b.longitude - (11.0 + 31.0 / 60.0)).abs() < 1e-9);
        assert_eq!(b.fixed_at, Some(late));
    }

    #[test]
    fn vtg_gives_speed_and_course_when_no_rmc_comes() {
        let mut d = Decoder::new();
        let u = d.apply(&sentence(VTG), Instant::now());
        assert!(u.boat_changed);
        assert_eq!(u.track_point, None);
        let b = d.boat();
        assert!((b.speed_over_ground - 5.5 * 0.514_444).abs() < 1e-3);
        assert!((b.course_over_ground - 54.7_f64.to_radians()).abs() < 1e-9);
        assert_eq!(b.fixed_at, None, "VTG is not a fix");
        let no_fix = d.apply(
            &sentence("GPVTG,054.7,T,,M,005.5,N,010.2,K,N"),
            Instant::now(),
        );
        assert_eq!(no_fix, Update::default(), "mode N");
    }

    #[test]
    fn the_old_vtg_form_without_unit_letters_is_read() {
        // Before NMEA 2.3: true course, magnetic course, knots, km/h.
        let mut d = Decoder::new();
        let u = d.apply(&sentence("GPVTG,054.7,034.4,005.5,010.2"), Instant::now());
        assert!(u.boat_changed);
        let b = d.boat();
        assert!((b.speed_over_ground - 5.5 * 0.514_444).abs() < 1e-3);
        assert!((b.course_over_ground - 54.7_f64.to_radians()).abs() < 1e-9);
        let odd = d.apply(&sentence("GPVTG,054.7,034.4,005.5"), Instant::now());
        assert_eq!(odd, Update::default(), "neither form: nothing read");
    }

    #[test]
    fn gga_without_a_fix_or_estimated_is_no_fix() {
        for quality in ["0", "6"] {
            let mut d = Decoder::new();
            let body = GGA.replace(",E,1,08,", &format!(",E,{quality},08,"));
            let u = d.apply(&sentence(&body), Instant::now());
            assert!(u.boat_changed, "quality {quality}");
            assert_eq!(u.track_point, None, "quality {quality}");
            assert!(!d.boat().position_valid);
        }
        let mut d = Decoder::new();
        let u = d.apply(
            &sentence("GPGGA,120000.00,,,,,0,00,,,,,,,,,"),
            Instant::now(),
        );
        assert_eq!(u.track_point, None, "the simulator's no-fix GGA");
    }

    #[test]
    fn gll_status_v_or_mode_n_or_e_is_no_fix() {
        for body in [
            "GPGLL,4916.45,N,12311.12,W,225444,V,A",
            "GPGLL,4916.45,N,12311.12,W,225444,A,N",
            "GPGLL,4916.45,N,12311.12,W,225444,A,E",
        ] {
            let mut d = Decoder::new();
            let u = d.apply(&sentence(body), Instant::now());
            assert_eq!(u.track_point, None, "{body}");
            assert!(!d.boat().position_valid, "{body}");
        }
        let mut d = Decoder::new();
        let old = d.apply(
            &sentence("GPGLL,4916.45,N,12311.12,W,225444,A"),
            Instant::now(),
        );
        assert!(old.track_point.is_some(), "no mode field (before NMEA 2.3)");
    }

    #[test]
    fn the_talker_filter_covers_gga_and_gll() {
        let mut d = Decoder::with_talker("GN");
        assert_eq!(d.apply(&sentence(GGA), Instant::now()), Update::default());
        assert_eq!(d.apply(&sentence(GLL), Instant::now()), Update::default());
        let gn = GGA.replacen("GP", "GN", 1);
        assert!(d
            .apply(&sentence(&gn), Instant::now())
            .track_point
            .is_some());
    }

    /// `!body*hh`, as `sentence` does for `$`.
    fn bang(body: &str) -> String {
        sentence(body).replacen('$', "!", 1)
    }

    #[test]
    fn aivdm_updates_the_ais_targets() {
        let mut d = Decoder::new();
        let now = Instant::now();
        let u = d.apply(&bang("AIVDM,1,1,,A,13M@=r@01s02OwHMMkJ3Q2lt0000,0"), now);
        assert!(u.ais_changed);
        assert!(!u.boat_changed);
        let targets = d.ais_targets();
        assert_eq!(targets.len(), 1);
        let t = &targets[0];
        assert_eq!(t.context, "vessels.urn:mrn:imo:mmsi:232001001");
        let (lon, lat) = t.position.unwrap();
        assert!((lon - 0.5461).abs() < 1e-4 && (lat - 51.4950).abs() < 1e-4);
        assert!((t.heading.unwrap() - 90.0_f64.to_radians()).abs() < 1e-9);
        assert_eq!(t.name, None);
    }

    #[test]
    fn a_name_joins_the_target_with_the_same_mmsi() {
        let mut d = Decoder::new();
        let now = Instant::now();
        d.apply(&bang("AIVDM,1,1,,B,B3M@AlP0@@0`B87GIs2`j77P0000,0"), now);
        d.apply(&bang("AIVDM,1,1,,B,H3M@AlPpu8@T>1=@580000000000,0"), now);
        let targets = d.ais_targets();
        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].name.as_deref(), Some("NORDIC STAR"));
        assert!(targets[0].position.is_some());
    }

    #[test]
    fn a_target_keeps_its_type_status_and_class() {
        let mut d = Decoder::new();
        let now = Instant::now();
        d.apply(&bang("AIVDM,1,1,,A,101r4PEP00wrFE9th2572?wp2000,0"), now);
        let t = &d.ais_targets()[0];
        assert_eq!(
            (t.nav_status, t.class_b, t.ship_type),
            (Some(5), false, None)
        );
        d.apply(&bang("AIVDM,1,1,,B,B3M@AlP0@@0`B87GIs2`j77P0000,0"), now);
        d.apply(&bang("AIVDM,1,1,,B,H3M@AlTT1234001GHIJopq104220,0"), now);
        let b = d
            .ais_targets()
            .into_iter()
            .find(|t| t.context.ends_with("232002002"))
            .unwrap();
        assert_eq!(
            (b.class_b, b.ship_type, b.nav_status),
            (true, Some(36), None)
        );
    }

    #[test]
    fn a_target_keeps_its_static_data() {
        let mut d = Decoder::new();
        let (first, second) =
            "53M@=r@0000048<C7;=<D61=0U8U@000000000166@D556tfN;C3kQ3kP00000000000000".split_at(60);
        d.apply(&bang(&format!("AIVDM,2,1,3,A,{first},0")), Instant::now());
        d.apply(&bang(&format!("AIVDM,2,2,3,A,{second},2")), Instant::now());
        let t = &d.ais_targets()[0];
        assert_eq!(t.call_sign.as_deref(), Some("ABCD123"));
        assert_eq!(t.destination.as_deref(), Some("LONDON"));
        assert_eq!(
            (t.length_m, t.beam_m, t.draught_m),
            (Some(70), Some(10), Some(4.5))
        );
    }

    #[test]
    fn aivdo_own_ship_reports_are_not_targets() {
        let mut d = Decoder::new();
        let u = d.apply(
            &bang("AIVDO,1,1,,A,13M@=r@01s02OwHMMkJ3Q2lt0000,0"),
            Instant::now(),
        );
        assert_eq!(u, Update::default());
        assert!(d.ais_targets().is_empty());
    }

    #[test]
    fn a_dollar_aivdm_with_a_bad_checksum_is_ignored() {
        let mut d = Decoder::new();
        let line = bang("AIVDM,1,1,,A,13M@=r@01s02OwHMMkJ3Q2lt0000,0");
        let bad = format!("{}00", &line[..line.len() - 2]);
        assert_eq!(d.apply(&bad, Instant::now()), Update::default());
    }
}
