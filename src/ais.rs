use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::overlay::{AisStyle, Category, Mark, Shape};

/// A target not heard from for this long is dropped from the display.
pub const TARGET_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Removes targets that have not been heard from within the timeout.
/// Returns true when at least one target was removed.
pub fn retain_fresh(targets: &mut HashMap<String, AisTarget>, now: Instant) -> bool {
    let before = targets.len();
    targets.retain(|_, target| target.is_fresh(now));
    targets.len() != before
}

/// At most this many targets are kept. A busy port has a few hundred.
pub const MAX_TARGETS: usize = 500;

/// Keeps the target map bounded for a passage of weeks: removes silent
/// targets (`retain_fresh`), then the least recently heard ones above
/// `MAX_TARGETS`. Static data (name) lives in the target, so it goes with
/// it. Returns true when at least one target was removed.
pub fn bound(targets: &mut HashMap<String, AisTarget>, now: Instant) -> bool {
    let mut removed = retain_fresh(targets, now);
    if targets.len() > MAX_TARGETS {
        let mut by_age: Vec<(Instant, String)> = targets
            .iter()
            .map(|(context, target)| (target.last_update, context.clone()))
            .collect();
        by_age.sort();
        let excess = targets.len() - MAX_TARGETS;
        for (_, context) in by_age.into_iter().take(excess) {
            targets.remove(&context);
        }
        removed = true;
    }
    removed
}

/// The same rule for the displayed list. Returns true when one was removed.
pub fn expire(targets: &mut Vec<AisTarget>, now: Instant) -> bool {
    let before = targets.len();
    targets.retain(|target| target.is_fresh(now));
    targets.len() != before
}

#[derive(Clone, Debug)]
pub struct AisTarget {
    pub context: String,
    pub name: Option<String>,
    pub position: Option<(f64, f64)>,
    pub course_over_ground: Option<f64>,
    pub speed_over_ground: Option<f64>,
    /// True heading in radians, when the target sends one.
    pub heading: Option<f64>,
    /// AIS ship type, 1 to 99, once a static report gave it.
    pub ship_type: Option<u8>,
    /// AIS navigational status, 0 to 14 (class A position reports).
    pub nav_status: Option<u8>,
    /// A class B transponder (small craft).
    pub class_b: bool,
    pub call_sign: Option<String>,
    pub destination: Option<String>,
    pub length_m: Option<u16>,
    pub beam_m: Option<u16>,
    pub draught_m: Option<f64>,
    last_update: Instant,
}

impl AisTarget {
    pub(crate) fn new(context: String) -> Self {
        Self {
            context,
            name: None,
            position: None,
            course_over_ground: None,
            speed_over_ground: None,
            heading: None,
            ship_type: None,
            nav_status: None,
            class_b: false,
            call_sign: None,
            destination: None,
            length_m: None,
            beam_m: None,
            draught_m: None,
            last_update: Instant::now(),
        }
    }

    pub fn is_fresh(&self, now: Instant) -> bool {
        now.duration_since(self.last_update) < TARGET_TIMEOUT
    }

    pub(crate) fn update_received(&mut self) {
        self.last_update = Instant::now();
    }

    /// The MMSI, the last part of the context.
    fn label_mmsi(&self) -> String {
        self.context
            .rsplit(':')
            .next()
            .unwrap_or(&self.context)
            .to_string()
    }

    fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            self.context
                .rsplit(':')
                .next()
                .unwrap_or(&self.context)
                .to_string()
        })
    }
}

/// The rows of a target's details window, label and value, in reading
/// order. A value the target has not sent is left out; the MMSI, class,
/// and time since the last report always show. `own` is the own boat's
/// position, for range and bearing; the bearing is magnetic with a
/// `variation`. The target's own course and heading are always true.
pub fn details(
    target: &AisTarget,
    own: Option<(f64, f64)>,
    variation: Option<f64>,
    now: Instant,
) -> Vec<(&'static str, String)> {
    let size = match (target.length_m, target.beam_m) {
        (Some(length), Some(beam)) => Some(format!("{length} × {beam} m")),
        (Some(length), None) => Some(format!("{length} m long")),
        _ => None,
    };
    let range = own.zip(target.position).map(|(own, at)| {
        let (distance, bearing) = crate::gps::distance_bearing_nm(own, at);
        format!(
            "{distance:.2} NM, bearing {}",
            crate::magnetic::bearing_text(bearing, variation)
        )
    });
    let rows: [(&'static str, Option<String>); 15] = [
        ("Name", target.name.clone()),
        ("MMSI", Some(target.label_mmsi())),
        ("Call sign", target.call_sign.clone()),
        (
            "Type",
            target
                .ship_type
                .map(|kind| format!("{} ({kind})", ship_type_name(kind))),
        ),
        (
            "Class",
            Some(if target.class_b { "B" } else { "A" }.to_string()),
        ),
        (
            "Status",
            target
                .nav_status
                .map(|status| nav_status_name(status).to_string()),
        ),
        ("Position", target.position.map(format_position)),
        (
            "Speed",
            target
                .speed_over_ground
                .map(|speed| format!("{:.1} kn", speed * 3600.0 / 1852.0)),
        ),
        (
            "Course",
            target
                .course_over_ground
                .map(|course| crate::magnetic::bearing_text(course.to_degrees(), None)),
        ),
        (
            "Heading",
            target
                .heading
                .map(|heading| crate::magnetic::bearing_text(heading.to_degrees(), None)),
        ),
        ("Size", size),
        ("Draught", target.draught_m.map(|d| format!("{d:.1} m"))),
        ("Destination", target.destination.clone()),
        ("Range", range),
        (
            "Last report",
            Some(ago(now.saturating_duration_since(target.last_update))),
        ),
    ];
    rows.into_iter()
        .filter_map(|(label, value)| value.map(|value| (label, value)))
        .collect()
}

/// `51° 29.700' N  000° 32.766' E`: degrees and decimal minutes.
fn format_position((lon, lat): (f64, f64)) -> String {
    // In thousandths of a minute, so 59.9996' rounds up to the next degree.
    let split = |value: f64| {
        let thousandths = (value.abs() * 60_000.0).round() as u64;
        (thousandths / 60_000, (thousandths % 60_000) as f64 / 1000.0)
    };
    let (lat_deg, lat_min) = split(lat);
    let (lon_deg, lon_min) = split(lon);
    format!(
        "{lat_deg:02}° {lat_min:06.3}' {}  {lon_deg:03}° {lon_min:06.3}' {}",
        if lat < 0.0 { 'S' } else { 'N' },
        if lon < 0.0 { 'W' } else { 'E' },
    )
}

fn ago(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs();
    match seconds {
        0..60 => format!("{seconds} s ago"),
        60..3600 => format!("{} min {} s ago", seconds / 60, seconds % 60),
        _ => format!("{} h {} min ago", seconds / 3600, seconds % 3600 / 60),
    }
}

/// The ITU-R M.1371 name of an AIS ship type.
pub fn ship_type_name(kind: u8) -> String {
    match kind {
        20..=29 => "Wing in ground",
        30 => "Fishing",
        31 | 32 => "Towing",
        33 => "Dredging",
        34 => "Diving",
        35 => "Military",
        36 => "Sailing",
        37 => "Pleasure craft",
        40..=49 => "High-speed craft",
        50 => "Pilot",
        51 => "Search and rescue",
        52 => "Tug",
        53 => "Port tender",
        54 => "Anti-pollution",
        55 => "Law enforcement",
        58 => "Medical transport",
        59 => "Non-combatant",
        60..=69 => "Passenger",
        70..=79 => "Cargo",
        80..=89 => "Tanker",
        90..=99 => "Other",
        _ => return format!("Type {kind}"),
    }
    .to_string()
}

/// The ITU-R M.1371 name of an AIS navigational status.
pub fn nav_status_name(status: u8) -> &'static str {
    match status {
        0 => "Under way using engine",
        1 => "At anchor",
        2 => "Not under command",
        3 => "Restricted manoeuvrability",
        4 => "Constrained by draught",
        5 => "Moored",
        6 => "Aground",
        7 => "Engaged in fishing",
        8 => "Under way sailing",
        11 => "Towing astern",
        12 => "Pushing ahead or towing alongside",
        14 => "AIS-SART active",
        _ => "Reserved",
    }
}

/// One labelled marker per target that has a position.
pub fn shapes(targets: &[AisTarget]) -> Vec<Shape> {
    shapes_without(targets, &[])
}

/// As `shapes`, without the targets at `excluded`: the collision alarm
/// draws those itself.
pub fn shapes_without(targets: &[AisTarget], excluded: &[(f64, f64)]) -> Vec<Shape> {
    targets
        .iter()
        .filter_map(|target| {
            let at = target.position.filter(|at| !excluded.contains(at));
            at.map(|at| Shape::Ais {
                at,
                course: target.course_over_ground,
                heading: target.heading,
                label: target.label(),
                style: AisStyle {
                    category: Category::of(target.ship_type),
                    mark: Mark::of(target.nav_status),
                    class_b: target.class_b,
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collision_targets_can_be_hidden_from_normal_ais_shapes() {
        let mut target = AisTarget::new("vessels.target".to_string());
        target.position = Some((2.0, 3.0));
        target.course_over_ground = Some(0.0);
        assert!(shapes_without(std::slice::from_ref(&target), &[(2.0, 3.0)]).is_empty());
        assert_eq!(shapes(&[target]).len(), 1);
    }

    #[test]
    fn target_expires_after_five_minutes_without_updates() {
        let now = Instant::now();
        let mut target = AisTarget::new("vessels.test".to_string());

        target.last_update = now - Duration::from_secs(299);
        assert!(target.is_fresh(now));

        target.last_update = now - TARGET_TIMEOUT;
        assert!(!target.is_fresh(now));
    }

    #[test]
    fn retain_fresh_drops_expired_targets_and_reports_whether_any_went() {
        let now = Instant::now();
        let mut targets = std::collections::HashMap::new();
        let mut stale = AisTarget::new("vessels.stale".to_string());
        stale.last_update = now - TARGET_TIMEOUT - Duration::from_secs(1);
        targets.insert(stale.context.clone(), stale);
        targets.insert(
            "vessels.fresh".to_string(),
            AisTarget::new("vessels.fresh".to_string()),
        );

        assert!(retain_fresh(&mut targets, now));
        assert_eq!(targets.len(), 1);
        assert!(targets.contains_key("vessels.fresh"));

        assert!(!retain_fresh(&mut targets, now));
        assert_eq!(targets.len(), 1);
    }

    #[test]
    fn a_target_shape_carries_its_course_heading_and_style() {
        let mut target = AisTarget::new("vessels.urn:mrn:imo:mmsi:232001001".to_string());
        target.position = Some((0.5, 51.5));
        target.course_over_ground = Some(1.0);
        target.heading = Some(1.5);
        target.ship_type = Some(30);
        target.nav_status = Some(7);
        target.class_b = true;
        assert_eq!(
            shapes(&[target]),
            vec![Shape::Ais {
                at: (0.5, 51.5),
                course: Some(1.0),
                heading: Some(1.5),
                label: "232001001".into(),
                style: AisStyle {
                    category: Category::Fishing,
                    mark: Some(Mark::Fishing),
                    class_b: true,
                },
            }]
        );
    }

    fn sea_spirit(now: Instant) -> AisTarget {
        let mut t = AisTarget::new("vessels.urn:mrn:imo:mmsi:232001001".to_string());
        t.name = Some("SEA SPIRIT".into());
        t.call_sign = Some("ABCD123".into());
        t.ship_type = Some(70);
        t.nav_status = Some(0);
        t.position = Some((0.5461, 51.4950));
        t.speed_over_ground = Some(12.3 * 1852.0 / 3600.0);
        t.course_over_ground = Some(90.0_f64.to_radians());
        t.heading = Some(91.0_f64.to_radians());
        t.length_m = Some(70);
        t.beam_m = Some(10);
        t.draught_m = Some(4.5);
        t.destination = Some("LONDON".into());
        t.last_update = now - Duration::from_secs(5);
        t
    }

    #[test]
    fn the_details_window_shows_every_value_carefully_formatted() {
        let now = Instant::now();
        // One nautical mile due south of the target: one minute of latitude.
        let own = (0.5461, 51.4950 - 1.0 / 60.0);
        let rows = details(&sea_spirit(now), Some(own), None, now);
        let expected = [
            ("Name", "SEA SPIRIT"),
            ("MMSI", "232001001"),
            ("Call sign", "ABCD123"),
            ("Type", "Cargo (70)"),
            ("Class", "A"),
            ("Status", "Under way using engine"),
            ("Position", "51° 29.700' N  000° 32.766' E"),
            ("Speed", "12.3 kn"),
            ("Course", "090°T"),
            ("Heading", "091°T"),
            ("Size", "70 × 10 m"),
            ("Draught", "4.5 m"),
            ("Destination", "LONDON"),
            ("Range", "1.00 NM, bearing 000°T"),
            ("Last report", "5 s ago"),
        ];
        let rows: Vec<(&str, &str)> = rows.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(rows, expected);
    }

    #[test]
    fn with_magnetic_bearings_the_bearing_to_the_target_is_magnetic_and_its_course_stays_true() {
        let now = Instant::now();
        let own = (0.5461, 51.4950 - 1.0 / 60.0);
        // Variation 2 degrees west: true 000 is magnetic 002.
        let rows = details(&sea_spirit(now), Some(own), Some(-2.0), now);
        let value = |label| rows.iter().find(|(k, _)| *k == label).unwrap().1.clone();
        assert_eq!(value("Range"), "1.00 NM, bearing 002°M");
        assert_eq!(value("Course"), "090°T");
        assert_eq!(value("Heading"), "091°T");
    }

    #[test]
    fn values_not_known_are_left_out_and_the_mmsi_always_shows() {
        let now = Instant::now();
        let mut t = AisTarget::new("vessels.urn:mrn:imo:mmsi:244123456".to_string());
        t.position = Some((-4.5, -33.25));
        t.class_b = true;
        t.last_update = now - Duration::from_secs(125);
        let rows = details(&t, None, None, now);
        let rows: Vec<(&str, &str)> = rows.iter().map(|(k, v)| (*k, v.as_str())).collect();
        assert_eq!(
            rows,
            [
                ("MMSI", "244123456"),
                ("Class", "B"),
                ("Position", "33° 15.000' S  004° 30.000' W"),
                ("Last report", "2 min 5 s ago"),
            ]
        );
    }

    #[test]
    fn ship_types_and_statuses_have_names() {
        for (kind, name) in [
            (30, "Fishing"),
            (31, "Towing"),
            (36, "Sailing"),
            (37, "Pleasure craft"),
            (45, "High-speed craft"),
            (50, "Pilot"),
            (51, "Search and rescue"),
            (52, "Tug"),
            (55, "Law enforcement"),
            (65, "Passenger"),
            (80, "Tanker"),
            (12, "Type 12"),
        ] {
            assert_eq!(ship_type_name(kind), name, "{kind}");
        }
        for (status, name) in [
            (1, "At anchor"),
            (5, "Moored"),
            (7, "Engaged in fishing"),
            (8, "Under way sailing"),
            (14, "AIS-SART active"),
        ] {
            assert_eq!(nav_status_name(status), name, "{status}");
        }
    }

    #[test]
    fn the_ship_type_gives_the_colour_category() {
        for (kind, category) in [
            (Some(70), Category::Cargo),
            (Some(79), Category::Cargo),
            (Some(80), Category::Tanker),
            (Some(89), Category::Tanker),
            (Some(60), Category::Passenger),
            (Some(40), Category::HighSpeed),
            (Some(49), Category::HighSpeed),
            (Some(50), Category::Service),
            (Some(52), Category::Service),
            (Some(55), Category::Service),
            (Some(30), Category::Fishing),
            (Some(36), Category::Leisure),
            (Some(37), Category::Leisure),
            (Some(31), Category::Other),
            (Some(90), Category::Other),
            (None, Category::Other),
        ] {
            assert_eq!(Category::of(kind), category, "{kind:?}");
        }
    }

    #[test]
    fn the_nav_status_gives_the_day_shape_mark() {
        for (status, mark) in [
            (Some(0), None),
            (Some(1), Some(Mark::Ball)),
            (Some(5), Some(Mark::Ball)),
            (Some(2), Some(Mark::TwoBalls)),
            (Some(3), Some(Mark::BallDiamondBall)),
            (Some(4), Some(Mark::Cylinder)),
            (Some(6), Some(Mark::ThreeBalls)),
            (Some(7), Some(Mark::Fishing)),
            (Some(8), None),
            (None, None),
        ] {
            assert_eq!(Mark::of(status), mark, "{status:?}");
        }
    }

    #[test]
    fn the_target_list_is_capped_by_dropping_the_least_recent() {
        let now = Instant::now();
        let mut targets = HashMap::new();
        for i in 0..MAX_TARGETS + 2 {
            let mut target = AisTarget::new(format!("vessels.{i}"));
            target.last_update = now - Duration::from_millis(i as u64);
            targets.insert(target.context.clone(), target);
        }
        assert!(bound(&mut targets, now));
        assert_eq!(targets.len(), MAX_TARGETS);
        assert!(targets.contains_key("vessels.0"), "the newest stays");
        assert!(!targets.contains_key(&format!("vessels.{}", MAX_TARGETS + 1)));
        assert!(!targets.contains_key(&format!("vessels.{MAX_TARGETS}")));
        assert!(!bound(&mut targets, now), "nothing more to drop");
    }

    #[test]
    fn bound_also_drops_silent_targets() {
        let now = Instant::now();
        let mut targets = HashMap::new();
        let mut stale = AisTarget::new("vessels.stale".to_string());
        stale.last_update = now - TARGET_TIMEOUT;
        targets.insert(stale.context.clone(), stale);
        assert!(bound(&mut targets, now));
        assert!(targets.is_empty());
    }
}
