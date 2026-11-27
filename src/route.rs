use crate::geo_data::{GeoFile, GeoPoint};
use crate::gps;
use crate::mbtiles_renderer::ChartView;
use crate::overlay::Shape;
use crate::Mode;
use sdl2::rect::Point;
use std::io;
use std::path::Path;

#[derive(Debug)]
pub struct Route {
    track: Vec<(f64, f64)>,
    distance: Vec<f64>,
    restante: Vec<f64>,
    longueur: f64,
    /// The file this route is saved to.
    file: GeoFile<GeoPoint>,
}

impl Route {
    /// Loads the route saved at `json_path`. A missing file is an empty
    /// route, a damaged one is set aside, and a legacy `.gpx` beside it is
    /// migrated once (see `geo_data::GeoFile`). `save` writes back to the
    /// same path. Never fails: a saved file cannot stop the app starting.
    pub fn load(json_path: &Path) -> Self {
        let file = Self::file_at(json_path);
        let track: Vec<(f64, f64)> = file.load().into_iter().map(Into::into).collect();
        crate::diagnostics::record("INFO", format!("Route chargée: {} points", track.len()));
        Route::from_points_in(track, file)
    }

    fn file_at(path: &Path) -> GeoFile<GeoPoint> {
        GeoFile::new(path).with_legacy_file("gpx", |legacy| {
            crate::gpx::load_points(legacy)
                .map(|points| points.into_iter().map(Into::into).collect())
        })
    }

    /// A route the tests never save; its path is in the temp directory so
    /// a save by mistake cannot reach `default/`.
    #[cfg(test)]
    fn from_points(track: Vec<(f64, f64)>) -> Self {
        Self::from_points_at(
            track,
            &std::env::temp_dir().join("plotter-route-not-saved.json"),
        )
    }

    /// A route over `track` that `save` will write to `path`.
    #[cfg(test)]
    pub(crate) fn from_points_at(track: Vec<(f64, f64)>, path: &Path) -> Self {
        Self::from_points_in(track, Self::file_at(path))
    }

    fn from_points_in(track: Vec<(f64, f64)>, file: GeoFile<GeoPoint>) -> Self {
        let mut route = Route {
            track,
            distance: Vec::new(),
            restante: Vec::new(),
            longueur: 0.0,
            file,
        };
        route.calc_route();
        route
    }
    /// The route as shapes. With `navigation = Some((nav_id, boat))` the
    /// first shape is the leg from the boat to the active waypoint and the
    /// route is drawn from that waypoint on; otherwise the whole route.
    /// While editing, the midpoint handles of the drawn legs follow.
    pub fn shapes(&self, mode: Mode, navigation: Option<(usize, (f64, f64))>) -> Vec<Shape> {
        let mut shapes = Vec::new();
        let from = match navigation {
            Some((id, boat)) => {
                // An empty route has nothing to navigate to.
                let Some(id) = self.active_waypoint(id) else {
                    return shapes;
                };
                shapes.push(Shape::Route(vec![boat, self.track[id]]));
                id
            }
            None => 0,
        };
        let rest = self.track.get(from..).unwrap_or(&[]);
        if !rest.is_empty() {
            shapes.push(Shape::Route(rest.to_vec()));
        }
        if mode == Mode::EditRoute && rest.len() >= 2 {
            let handles = self.midpoints();
            shapes.push(Shape::RouteHandles(
                handles.get(from..).unwrap_or(&[]).to_vec(),
            ));
        }
        shapes
    }

    pub fn show_route_len(&self) -> String {
        // An empty sum is -0.0 in Rust, which would print as "-0.0".
        format!("route : {:.1} Mn", self.longueur.max(0.0))
    }

    /// Index of the waypoint being navigated to, clamped to the last point.
    /// `None` when the route is empty.
    pub fn active_waypoint(&self, id: usize) -> Option<usize> {
        let last = self.track.len().checked_sub(1)?;
        Some(id.min(last))
    }

    pub fn save(&self) -> io::Result<()> {
        let records: Vec<GeoPoint> = self.track.iter().copied().map(Into::into).collect();
        self.file.save(&records)
    }

    pub fn export_gpx(&self, path: &Path) -> io::Result<()> {
        let document = crate::gpx::write_route("Plotter", &self.track);
        crate::geo_data::atomic_write(path, document.as_bytes())
    }

    pub fn clear(&mut self) -> io::Result<()> {
        self.track.clear();
        self.distance.clear();
        self.restante.clear();
        self.longueur = 0.0;
        self.save()
    }

    pub fn remove(&mut self, i: usize) {
        self.track.remove(i);
        self.calc_route();
    }

    pub fn modify(&mut self, i: usize, longitude: f64, latitude: f64) {
        self.track[i] = (longitude, latitude);
        self.calc_route();
    }

    pub fn add(&mut self, longitude: f64, latitude: f64) {
        self.track.push((longitude, latitude));
        self.calc_route();
    }

    pub fn insert(&mut self, i: usize, longitude: f64, latitude: f64) {
        self.track.insert(i + 1, (longitude, latitude));
        self.calc_route();
    }

    fn calc_route(&mut self) {
        self.distance.clear();

        for w in self.track.windows(2) {
            self.distance.push(gps::leg_nm(w[0], w[1]).0);
        }

        self.longueur = self.distance.iter().sum();

        self.restante = vec![0.0; self.track.len()];
        let mut reste = 0.0;
        for i in (0..self.distance.len()).rev() {
            reste += self.distance[i];
            self.restante[i] = reste;
        }
    }

    /// Rhumb line distance and true bearing from the boat to the active
    /// waypoint, plus the distance to the end of the route. `None` when the
    /// route is empty.
    pub fn calc_route_mn_from_boat(&self, id: usize, boat: (f64, f64)) -> Option<(f64, f64, f64)> {
        let id = self.active_waypoint(id)?;
        let (dist_wp, bearing_wp) = gps::leg_nm(boat, self.track[id]);
        let dist_total = dist_wp + self.restante[id];
        Some((dist_wp, bearing_wp, dist_total))
    }

    /// The route point drawn within `radius` pixels of `pixel`, if any.
    pub fn find_point_at(&self, view: &ChartView, pixel: Point, radius: i32) -> Option<usize> {
        view.nearest(self.track.iter().copied(), pixel, radius)
    }

    /// The leg whose midpoint handle is within `radius` pixels of `pixel`.
    pub fn find_midpoint_at(&self, view: &ChartView, pixel: Point, radius: i32) -> Option<usize> {
        view.nearest(self.midpoints(), pixel, radius)
    }

    pub fn midpoints(&self) -> Vec<(f64, f64)> {
        self.track
            .windows(2)
            // Halfway along the rhumb line the leg is drawn as.
            .map(|w| {
                gps::rhumb_point(w[0], w[1], 0.5)
                    .unwrap_or(((w[0].0 + w[1].0) / 2.0, (w[0].1 + w[1].1) / 2.0))
            })
            .collect()
    }
    /*
    pub fn get_wp(&self, id: usize) -> Option<(f64, f64)>
        {
        let last = self.track.len().checked_sub(1)?; // None si le Vec est vide
        Some(self.track[id.min(last)])
        }
    */
    pub fn get_nb_wp(&self) -> usize {
        self.track.len()
    }
}

#[cfg(test)]
mod tests {
    use super::Route;
    use crate::overlay::Shape;
    use crate::Mode;

    #[test]
    fn navigation_shapes_draw_the_boat_leg_then_the_remaining_legs_and_handles_when_editing() {
        let route = Route::from_points(vec![(0.0, 0.0), (0.0, 1.0), (0.0, 2.0)]);
        let boat = (0.5, 0.5);

        let shapes = route.shapes(Mode::EditRoute, Some((1, boat)));
        assert_eq!(
            shapes,
            vec![
                Shape::Route(vec![boat, (0.0, 1.0)]),
                Shape::Route(vec![(0.0, 1.0), (0.0, 2.0)]),
                Shape::RouteHandles(vec![(0.0, 1.5)]),
            ]
        );

        let shapes = route.shapes(Mode::Measurement, None);
        assert_eq!(
            shapes,
            vec![Shape::Route(vec![(0.0, 0.0), (0.0, 1.0), (0.0, 2.0)])]
        );

        assert!(Route::from_points(Vec::new())
            .shapes(Mode::EditRoute, Some((0, boat)))
            .is_empty());
    }

    #[test]
    fn a_corrupt_route_file_loads_an_empty_route_and_is_set_aside() {
        let path = std::env::temp_dir().join(format!(
            "plotter-route-corrupt-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, "garbage").unwrap();
        let route = Route::load(&path);
        assert_eq!(route.get_nb_wp(), 0);
        assert!(path.with_extension("json.corrupt").exists());
        std::fs::remove_file(path.with_extension("json.corrupt")).unwrap();
    }

    #[test]
    fn empty_route_has_no_active_waypoint_and_no_navigation_data() {
        let route = Route::from_points(Vec::new());
        assert_eq!(route.active_waypoint(0), None);
        assert_eq!(route.calc_route_mn_from_boat(0, (0.0, 0.0)), None);
    }

    #[test]
    fn active_waypoint_index_is_clamped_to_the_last_point() {
        let route = Route::from_points(vec![(0.0, 0.0), (0.0, 1.0), (0.0, 2.0)]);
        assert_eq!(route.active_waypoint(1), Some(1));
        assert_eq!(route.active_waypoint(7), Some(2));
    }

    #[test]
    fn navigation_from_the_last_waypoint_position_reports_zero_distance() {
        let route = Route::from_points(vec![(0.0, 0.0), (0.0, 1.0)]);
        let (to_waypoint, _bearing, to_end) = route.calc_route_mn_from_boat(9, (0.0, 1.0)).unwrap();
        assert!(to_waypoint.abs() < 1e-9);
        assert!(to_end.abs() < 1e-9);
    }

    #[test]
    fn navigation_distance_includes_the_remaining_legs() {
        // 1 degree of latitude is 60 nautical miles.
        let route = Route::from_points(vec![(0.0, 0.0), (0.0, 1.0), (0.0, 2.0)]);
        let (to_waypoint, bearing, to_end) = route.calc_route_mn_from_boat(1, (0.0, 0.0)).unwrap();
        assert!((to_waypoint - 60.0).abs() < 0.1, "{to_waypoint}");
        assert!(bearing.abs() < 1e-6, "{bearing}");
        assert!((to_end - 120.0).abs() < 0.2, "{to_end}");
    }

    #[test]
    fn route_legs_are_rhumb_lines_so_a_leg_along_a_parallel_holds_due_east() {
        // 10 degrees along 60 N: the rhumb line is 10 * 60.0405 * cos 60 =
        // 300.20 NM at 090. The great circle is shorter (299.97 NM) and
        // starts at about 086, which a helmsman cannot hold.
        let route = Route::from_points(vec![(0.0, 60.0), (10.0, 60.0)]);
        assert_eq!(route.show_route_len(), "route : 300.2 Mn");
        let (to_waypoint, bearing, _) = route.calc_route_mn_from_boat(1, (0.0, 60.0)).unwrap();
        assert!((to_waypoint - 300.20).abs() < 0.01, "{to_waypoint}");
        assert!((bearing - 90.0).abs() < 1e-9, "{bearing}");
    }

    #[test]
    fn the_handle_of_a_leg_across_the_antimeridian_sits_on_the_leg() {
        // A plain average of 179.5 and -179.5 is 0, the far side of the world.
        let route = Route::from_points(vec![(179.5, 10.0), (-179.5, 10.0)]);
        let handle = route.midpoints()[0];
        assert!((handle.0.abs() - 180.0).abs() < 1e-9, "{handle:?}");
        assert!((handle.1 - 10.0).abs() < 1e-9, "{handle:?}");
    }

    #[test]
    fn empty_route_length_is_not_negative_zero() {
        let route = Route::from_points(Vec::new());
        assert_eq!(route.show_route_len(), "route : 0.0 Mn");
    }
}
