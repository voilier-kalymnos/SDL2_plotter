use crate::gps;
use crate::mbtiles_renderer::ChartView;
use crate::overlay::Shape;
use sdl2::rect::Point;

pub struct Mesure {
    mark1: Option<(f64, f64)>,
    mark2: Option<(f64, f64)>,
    distance: f64,
    bearing: f64,
    /// The rhumb line distance and bearing, when they differ from the
    /// great circle enough to matter.
    rhumb: Option<(f64, f64)>,
}

/// The rhumb line shows when its bearing differs from the great circle by
/// more than this many degrees.
const RHUMB_BEARING_DIFFERENCE: f64 = 0.5;
/// The rhumb line also shows when its distance differs from the great
/// circle by more than this fraction.
const RHUMB_DISTANCE_DIFFERENCE: f64 = 0.001;

impl Mesure {
    pub fn new() -> Self {
        Mesure {
            mark1: None,
            mark2: None,
            distance: 0.0,
            bearing: 0.0,
            rhumb: None,
        }
    }

    /// Places the first mark, or places/moves the second one, at the chart
    /// position under `pixel`. A pixel with no position (beyond the poles)
    /// is ignored.
    pub fn add_mark(&mut self, view: &ChartView, pixel: Point) {
        let Some(position) = view.to_lonlat(pixel) else {
            return;
        };
        match self.mark1 {
            // Étape 1 : rien de posé -> premier point
            None => {
                self.mark1 = Some(position);
                self.mark2 = None;
                self.measure(None);
            }
            // Étapes suivantes : poser ou déplacer le point d'arrivée
            Some(mark1) => {
                self.mark2 = Some(position);
                self.measure(Some((mark1, position)));
            }
        }
    }

    /// Distance (nautical miles) and bearing (degrees) once both marks are
    /// placed.
    #[cfg(test)]
    pub fn result(&self) -> Option<(f64, f64)> {
        self.mark2.map(|_| (self.distance, self.bearing))
    }

    pub fn clear(&mut self) {
        self.mark1 = None;
        self.mark2 = None;
        self.measure(None);
    }

    /// Sets the great-circle readout of a leg, and the rhumb line readout
    /// when it differs. No leg is all zeros.
    fn measure(&mut self, leg: Option<((f64, f64), (f64, f64))>) {
        let Some((a, b)) = leg else {
            (self.distance, self.bearing, self.rhumb) = (0.0, 0.0, None);
            return;
        };
        (self.distance, self.bearing) = gps::distance_bearing_nm(a, b);
        self.rhumb = gps::rhumb_distance_bearing_nm(a, b).filter(|&(distance, bearing)| {
            let turn = (gps::wrap180(bearing - self.bearing)).abs();
            turn > RHUMB_BEARING_DIFFERENCE
                || (distance - self.distance).abs() > RHUMB_DISTANCE_DIFFERENCE * self.distance
        });
    }

    /// True when `pixel` is within `tolerance` pixels of the first mark.
    pub fn find_mark1_at(&self, view: &ChartView, pixel: Point, tolerance: i32) -> bool {
        view.nearest(self.mark1, pixel, tolerance).is_some()
    }

    /// Moves the first mark to the chart position under `pixel`.
    pub fn modify_mark1(&mut self, view: &ChartView, pixel: Point) {
        let Some(mark1) = view.to_lonlat(pixel) else {
            return;
        };
        self.mark1 = Some(mark1);
        self.measure(self.mark2.map(|mark2| (mark1, mark2)));
    }

    /// The marks, the line between them and the readout at the second mark.
    /// Bearings are magnetic with a `variation`, else true.
    pub fn shapes(&self, variation: Option<f64>) -> Vec<Shape> {
        let mut shapes = Vec::new();
        let Some(mark1) = self.mark1 else {
            return shapes;
        };
        shapes.push(Shape::Mark(mark1));
        if let Some(mark2) = self.mark2 {
            shapes.push(Shape::Mark(mark2));
            shapes.push(Shape::MeasureLine(mark1, mark2));
            let mut text = readout(self.distance, self.bearing, variation);
            if let Some((distance, bearing)) = self.rhumb {
                text += &format!(" / RL {}", readout(distance, bearing, variation));
            }
            shapes.push(Shape::Label {
                at: mark2,
                text,
                offset: (0, 0),
            });
        }
        shapes
    }
}

/// `12.3 Mn 045°T`, or metres below one nautical mile.
fn readout(distance: f64, bearing: f64, variation: Option<f64>) -> String {
    let bearing = crate::magnetic::bearing_text(bearing, variation);
    if distance >= 1.0 {
        format!("{distance:.1} Mn {bearing}")
    } else {
        format!("{:.0} m {bearing}", distance * 1852.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_marks_give_two_marks_a_line_and_a_readout_in_nautical_miles() {
        let view = ChartView::centred_on((0.0, 0.0), 6.0, 0.0, 512, 256);
        let mut mesure = Mesure::new();
        assert!(mesure.shapes(None).is_empty());
        mesure.add_mark(&view, view.to_pixel((0.0, 0.0)));
        assert_eq!(mesure.shapes(None), vec![Shape::Mark((0.0, 0.0))]);

        mesure.add_mark(&view, view.to_pixel((0.0, 1.0)));
        let shapes = mesure.shapes(None);
        assert_eq!(shapes.len(), 4, "{shapes:?}");
        assert!(matches!(shapes[2], Shape::MeasureLine(..)));
        let Shape::Label { text, .. } = &shapes[3] else {
            panic!("{shapes:?}");
        };
        assert!(text.ends_with("Mn 000°T"), "{text}");
    }

    #[test]
    fn with_a_variation_the_readout_is_magnetic() {
        let view = ChartView::centred_on((0.0, 0.0), 6.0, 0.0, 512, 256);
        let mut mesure = Mesure::new();
        mesure.add_mark(&view, view.to_pixel((0.0, 0.0)));
        mesure.add_mark(&view, view.to_pixel((0.0, 1.0)));
        let shapes = mesure.shapes(Some(-5.0));
        let Some(Shape::Label { text, .. }) = shapes.last() else {
            panic!("{shapes:?}");
        };
        assert!(text.ends_with("Mn 005°M"), "{text}");
    }

    fn readout(a: (f64, f64), b: (f64, f64)) -> String {
        let view = ChartView::centred_on(a, 3.0, 0.0, 1024, 1024);
        let mut mesure = Mesure::new();
        mesure.add_mark(&view, view.to_pixel(a));
        mesure.add_mark(&view, view.to_pixel(b));
        let shapes = mesure.shapes(None);
        let Some(Shape::Label { text, .. }) = shapes.last() else {
            panic!("{shapes:?}");
        };
        text.clone()
    }

    #[test]
    fn a_long_leg_along_a_parallel_also_shows_the_rhumb_line() {
        // 10 degrees along 60 N: great circle 300.0 NM starting near 086,
        // rhumb line 300.2 NM at 090.
        let text = readout((0.0, 60.0), (10.0, 60.0));
        // The marks snap to pixels, so only the bearings are exact here;
        // gps.rs tests the distances.
        assert!(text.contains(" Mn 086°T / RL "), "{text}");
        assert!(text.ends_with(" Mn 090°T"), "{text}");
    }

    #[test]
    fn a_short_leg_shows_the_great_circle_only() {
        // Over 5 NM the two lines differ by far less than 0.5 degrees.
        let text = readout((0.0, 50.0), (0.1, 50.05));
        assert!(!text.contains("RL"), "{text}");
    }
}
