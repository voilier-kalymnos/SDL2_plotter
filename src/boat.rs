use std::time::{Duration, Instant};

use crate::overlay::Shape;

/// Below this speed over ground the COG vector is not drawn: 1.0 m/s,
/// about 1.9 kn. Under it, GPS course is mostly noise.
const MINIMUM_SPEED: f64 = 1.0;

/// No position for this long: the fix is lost.
pub const FIX_TIMEOUT: Duration = Duration::from_secs(10);
/// No heading for this long: the heading is lost.
pub const HEADING_TIMEOUT: Duration = Duration::from_secs(10);
/// No variation for this long: the magnetic model gives it.
pub const VARIATION_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug)]
pub struct BoatState {
    pub longitude: f64,
    pub latitude: f64,
    pub position_valid: bool,
    pub speed_over_ground: f64,
    pub course_over_ground: f64,
    /// True heading in radians, when a source gives one.
    pub heading: Option<f64>,
    /// When the source last received a valid position. `None` until the
    /// first fix.
    pub fixed_at: Option<Instant>,
    /// When the source last received a heading.
    pub heading_at: Option<Instant>,
    /// A heading in radians from a magnetic compass (HDG), with its
    /// deviation applied. The session adds the variation to make `heading`.
    pub magnetic_heading: Option<f64>,
    /// The magnetic variation in degrees, east positive, when a source
    /// sends one.
    pub variation: Option<f64>,
    /// When the source last received a variation.
    pub variation_at: Option<Instant>,
    /// The UTC date that the GPS gives, for the magnetic model.
    pub gps_date: Option<chrono::NaiveDate>,
}

impl BoatState {
    pub fn default() -> Self {
        BoatState {
            longitude: 0.0,
            latitude: 0.0,
            position_valid: false,
            speed_over_ground: 0.0,
            course_over_ground: 0.0,
            heading: None,
            fixed_at: None,
            heading_at: None,
            magnetic_heading: None,
            variation: None,
            variation_at: None,
            gps_date: None,
        }
    }

    /// The boat marker at the last position that had a fix, with its course
    /// vector when under way and its heading line, both only while the fix
    /// is valid. Nothing before the first fix. Where it falls on screen is
    /// the painter's business.
    pub fn shape(&self) -> Option<Shape> {
        self.fixed_at?;
        let fix = self.position_valid;
        let moving = self.speed_over_ground > MINIMUM_SPEED;
        Some(Shape::Boat {
            at: (self.longitude, self.latitude),
            // The hull points along the heading, else the course while
            // moving; it keeps that direction when the fix is lost.
            bow: self.heading.or(moving.then_some(self.course_over_ground)),
            course: self.course(),
            heading: self.heading.filter(|_| fix),
            fix,
        })
    }

    /// This state as it is at `now`: the fix is lost when no position came
    /// for `FIX_TIMEOUT`, the heading when none came for `HEADING_TIMEOUT`,
    /// the variation when none came for `VARIATION_TIMEOUT`.
    /// The last position stays, for the grey marker.
    pub fn expired(self, now: Instant) -> BoatState {
        let fresh = |at: Option<Instant>, timeout: Duration| {
            at.is_some_and(|at| now.saturating_duration_since(at) < timeout)
        };
        BoatState {
            position_valid: self.position_valid && fresh(self.fixed_at, FIX_TIMEOUT),
            heading: self
                .heading
                .filter(|_| fresh(self.heading_at, HEADING_TIMEOUT)),
            magnetic_heading: self
                .magnetic_heading
                .filter(|_| fresh(self.heading_at, HEADING_TIMEOUT)),
            variation: self
                .variation
                .filter(|_| fresh(self.variation_at, VARIATION_TIMEOUT)),
            ..self
        }
    }

    /// The course over ground in radians, when it means something: a valid
    /// fix and more than `MINIMUM_SPEED`.
    pub fn course(&self) -> Option<f64> {
        (self.position_valid && self.speed_over_ground > MINIMUM_SPEED)
            .then_some(self.course_over_ground)
    }

    pub fn position(self) -> (f64, f64) {
        (self.longitude, self.latitude)
    }
}

/// Faster than this between two fixes is not a boat: 50 kn, in m/s.
const MAX_SPEED: f64 = 50.0 * 1852.0 / 3600.0;
/// GPS noise allowed on top of `MAX_SPEED`, in metres.
const JUMP_ALLOWANCE_METERS: f64 = 50.0;
/// A minute with no rejected fix ends an episode: the next one is logged.
const REJECTION_QUIET: Duration = Duration::from_secs(60);
/// Fixes in a row that agree with each other, but not with the boat,
/// before the boat moves to them.
const AGREEING_FIXES: u32 = 3;

/// Keeps impossible jumps off the chart: a second GPS on the same network,
/// or one bad fix. A fix is accepted when the speed it implies since the
/// last accepted fix is below 50 kn (plus 50 m of noise, over at least one
/// second). Otherwise it is rejected, unless it is the third in a row of
/// fixes that agree with each other: then the boat really is there (the
/// first GPS stopped, or came back after a gap). An accepted fix breaks
/// the row, so a second stream that mixes with the first never wins.
#[derive(Default)]
pub struct FixGuard {
    last: Option<((f64, f64), Instant)>,
    candidate: Option<((f64, f64), Instant, u32)>,
    rejected: crate::diagnostics::Repeated,
    last_rejection: Option<Instant>,
}

impl FixGuard {
    /// True when `position` (`(lon, lat)`, already range-checked) received
    /// at `now` may be shown and recorded.
    pub fn accept(&mut self, position: (f64, f64), now: Instant) -> bool {
        let Some((last, last_at)) = self.last else {
            self.last = Some((position, now));
            return true;
        };
        if plausible(last, last_at, position, now) {
            self.last = Some((position, now));
            self.candidate = None;
            return true;
        }
        let in_a_row = match self.candidate {
            Some((candidate, at, count)) if plausible(candidate, at, position, now) => count + 1,
            _ => 1,
        };
        if in_a_row >= AGREEING_FIXES {
            crate::diagnostics::record(
                "WARN",
                format!(
                    "Position du bateau déplacée: {AGREEING_FIXES} positions concordantes loin de la précédente"
                ),
            );
            self.last = Some((position, now));
            self.candidate = None;
            return true;
        }
        self.candidate = Some((position, now, in_a_row));
        if let Some(line) = self.rejection_line(now) {
            crate::diagnostics::record("WARN", line);
        }
        false
    }

    /// The log line for a rejection at `now`, if any. Two GPS streams give
    /// a rejection every second for as long as they run, with good fixes
    /// between: logged once, then every thousandth time, and again at once
    /// after a minute with no rejection (a new episode).
    fn rejection_line(&mut self, now: Instant) -> Option<String> {
        if self
            .last_rejection
            .is_some_and(|at| now.saturating_duration_since(at) >= REJECTION_QUIET)
        {
            self.rejected.clear();
        }
        self.last_rejection = Some(now);
        self.rejected
            .note("Position improbable ignorée (plus de 50 nœuds depuis la précédente; deux GPS ?)")
    }
}

/// True when going from `from` at `from_at` to `to` at `now` is within
/// `MAX_SPEED`, plus the noise allowance.
fn plausible(from: (f64, f64), from_at: Instant, to: (f64, f64), now: Instant) -> bool {
    let seconds = now
        .saturating_duration_since(from_at)
        .as_secs_f64()
        .max(1.0);
    crate::gps::distance_meters(from, to) <= MAX_SPEED * seconds + JUMP_ALLOWANCE_METERS
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn fixed(now: Instant) -> BoatState {
        BoatState {
            longitude: -4.5,
            latitude: 48.4,
            position_valid: true,
            speed_over_ground: 5.0,
            course_over_ground: 1.0,
            heading: Some(0.5),
            fixed_at: Some(now),
            heading_at: Some(now),
            ..BoatState::default()
        }
    }

    #[test]
    fn a_live_variation_expires_after_ten_seconds_like_the_fix() {
        let now = Instant::now();
        let boat = BoatState {
            variation: Some(-3.1),
            variation_at: Some(now),
            magnetic_heading: Some(1.0),
            ..fixed(now)
        };
        let soon = boat.expired(now + Duration::from_secs(9));
        assert_eq!(soon.variation, Some(-3.1));
        assert_eq!(soon.magnetic_heading, Some(1.0));
        let late = boat.expired(now + Duration::from_secs(10));
        assert_eq!(late.variation, None);
        assert_eq!(late.magnetic_heading, None, "it goes with the heading");
    }

    #[test]
    fn a_fix_shows_with_course_and_heading() {
        let boat = fixed(Instant::now());
        assert_eq!(
            boat.shape(),
            Some(Shape::Boat {
                at: (-4.5, 48.4),
                bow: Some(0.5),
                course: Some(1.0),
                heading: Some(0.5),
                fix: true
            })
        );
    }

    #[test]
    fn the_course_shows_only_when_under_way() {
        let mut boat = fixed(Instant::now());
        boat.speed_over_ground = 0.2;
        assert!(matches!(
            boat.shape(),
            Some(Shape::Boat { course: None, .. })
        ));
    }

    #[test]
    fn a_lost_fix_shows_at_the_last_position_with_no_vectors() {
        let mut boat = fixed(Instant::now());
        boat.position_valid = false;
        assert_eq!(
            boat.shape(),
            Some(Shape::Boat {
                at: (-4.5, 48.4),
                bow: Some(0.5),
                course: None,
                heading: None,
                fix: false
            })
        );
    }

    #[test]
    fn the_hull_points_along_the_heading_else_the_course_when_moving() {
        let mut boat = fixed(Instant::now());
        assert!(matches!(boat.shape(), Some(Shape::Boat { bow: Some(b), .. }) if b == 0.5));
        boat.heading = None;
        assert!(
            matches!(boat.shape(), Some(Shape::Boat { bow: Some(b), .. }) if b == 1.0),
            "no heading: the course"
        );
        boat.speed_over_ground = 0.2;
        assert!(
            matches!(boat.shape(), Some(Shape::Boat { bow: None, .. })),
            "stopped with no heading: no direction known"
        );
    }

    #[test]
    fn no_marker_before_the_first_fix() {
        assert_eq!(BoatState::default().shape(), None);
    }

    #[test]
    fn the_fix_and_the_heading_go_stale_after_ten_seconds() {
        let now = Instant::now();
        let boat = fixed(now);
        let just_in = boat.expired(now + Duration::from_millis(9_900));
        assert!(just_in.position_valid);
        assert_eq!(just_in.heading, Some(0.5));
        let stale = boat.expired(now + Duration::from_secs(10));
        assert!(!stale.position_valid);
        assert_eq!(stale.heading, None);
        assert_eq!(stale.position(), (-4.5, 48.4), "the last position is kept");
    }

    // One minute of latitude is one nautical mile (1852 m), so positions
    // here are built from minutes, independently of the guard's maths.
    const MINUTE: f64 = 1.0 / 60.0;

    fn at(minutes_north: f64) -> (f64, f64) {
        (0.5, 51.5 + minutes_north * MINUTE)
    }

    #[test]
    fn the_first_fix_and_normal_movement_are_accepted() {
        let t = Instant::now();
        let mut guard = FixGuard::default();
        assert!(guard.accept(at(0.0), t));
        // 5 kn for one second: 2.6 m.
        assert!(guard.accept(at(0.0014), t + Duration::from_secs(1)));
        // A second sentence for the same fix, at the same instant.
        assert!(guard.accept(at(0.0014), t + Duration::from_secs(1)));
    }

    #[test]
    fn a_jump_faster_than_50_knots_is_rejected() {
        let t = Instant::now();
        let mut guard = FixGuard::default();
        assert!(guard.accept(at(0.0), t));
        // 2 NM in one second is 7200 kn.
        assert!(!guard.accept(at(2.0), t + Duration::from_secs(1)));
        // 40 kn for a minute: 0.67 NM, fine.
        assert!(guard.accept(at(0.66), t + Duration::from_secs(60)));
    }

    #[test]
    fn a_second_stream_that_mixes_with_the_first_never_wins() {
        let t = Instant::now();
        let mut guard = FixGuard::default();
        for second in 0..20u64 {
            let now = t + Duration::from_secs(second);
            let moved = second as f64 * 0.0014;
            assert!(guard.accept(at(moved), now), "stream A, second {second}");
            assert!(
                !guard.accept(at(2.0 + moved), now),
                "stream B, second {second}"
            );
        }
    }

    #[test]
    fn three_fixes_in_a_row_that_agree_move_the_boat() {
        let t = Instant::now();
        let mut guard = FixGuard::default();
        assert!(guard.accept(at(0.0), t));
        // The first receiver stopped; the second one is 2 NM away.
        assert!(!guard.accept(at(2.0), t + Duration::from_secs(1)));
        assert!(!guard.accept(at(2.0014), t + Duration::from_secs(2)));
        assert!(guard.accept(at(2.0028), t + Duration::from_secs(3)));
        assert!(guard.accept(at(2.0042), t + Duration::from_secs(4)));
    }

    #[test]
    fn a_lasting_rejection_is_logged_once_and_again_after_a_quiet_minute() {
        let t = Instant::now();
        let mut guard = FixGuard::default();
        assert!(guard.rejection_line(t).is_some());
        for second in 1..600u64 {
            let now = t + Duration::from_secs(second);
            assert!(guard.rejection_line(now).is_none(), "second {second}");
        }
        let after_quiet = t + Duration::from_secs(599 + 61);
        assert!(guard.rejection_line(after_quiet).is_some(), "a new episode");
    }

    #[test]
    fn a_long_gap_allows_a_long_way() {
        let t = Instant::now();
        let mut guard = FixGuard::default();
        assert!(guard.accept(at(0.0), t));
        // 10 NM after an hour without a fix is 10 kn.
        assert!(guard.accept(at(10.0), t + Duration::from_secs(3600)));
    }

    #[test]
    fn the_heading_goes_stale_on_its_own() {
        let now = Instant::now();
        let mut boat = fixed(now + Duration::from_secs(8));
        boat.heading_at = Some(now);
        let later = boat.expired(now + Duration::from_secs(11));
        assert!(later.position_valid);
        assert_eq!(later.heading, None);
    }
}
