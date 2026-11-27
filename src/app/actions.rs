use chrono::Local;

use super::confirm_dialog::ConfirmAction;
use super::session::Session;
use super::AppAction;
use crate::catalogue::Role;
use crate::Mode;

impl Session {
    pub(super) fn execute_action(&mut self, action: AppAction) {
        crate::diagnostics::record("DEBUG", format!("Action: {}", action.name()));

        match action {
            AppAction::SetMode(mode) => self.mode = mode,
            AppAction::CycleMode => {
                self.mode = match self.mode {
                    Mode::EditRoute => Mode::EditWaypoint,
                    Mode::EditWaypoint => Mode::Measurement,
                    Mode::Measurement => Mode::EditRoute,
                };
            }
            // The window belongs to the shell (`Plotter::execute_action`).
            AppAction::OpenChartManager => {}
            AppAction::ToggleLock => {
                self.state.lock = match self.state.lock.take() {
                    Some(_) => None,
                    None => self.chart_choice().file.map(|f| f.file_name.clone()),
                };
            }
            AppAction::NextPhoto => match self.next_photo_layer() {
                Some(layer) => self.choose_layer(Some(layer)),
                None if self.showing_photo() => self.choose_layer(self.state.chart_layer.clone()),
                None => crate::diagnostics::record("INFO", "Aucune photo ici"),
            },
            AppAction::CentreOnBoat => {
                if self.boat.fixed_at.is_none() {
                    crate::diagnostics::record("INFO", "Pas encore de position du bateau");
                } else {
                    self.state.follow = true;
                    self.state.map_offset = (0, 0);
                    self.state.position = Some(self.boat.position());
                }
            }
            AppAction::ToggleFollow => {
                self.state.follow = !self.state.follow;
                self.center_map_on_visible_center();
            }
            AppAction::FollowOn => {
                self.state.follow = true;
                self.center_map_on_visible_center();
            }
            AppAction::FollowOff => {
                self.state.follow = false;
                self.center_map_on_visible_center();
            }
            AppAction::ToggleGoto => {
                self.state.goto = !self.state.goto;
                self.state.nav_id = 0;
            }
            AppAction::ClearRoute => {
                self.confirmation.request(ConfirmAction::ClearRoute);
            }
            AppAction::PreviousWaypoint => {
                self.state.nav_id = self.state.nav_id.saturating_sub(1);
            }
            AppAction::NextWaypoint => {
                let last = self.route.get_nb_wp().saturating_sub(1);
                self.state.nav_id = self.state.nav_id.saturating_add(1).min(last);
            }
            AppAction::Quit => self.running = false,
            AppAction::ClearTrack => {
                self.confirmation.request(ConfirmAction::ClearTrack);
            }
            AppAction::ExportRouteGpx => {
                let now = Local::now();
                let path = self
                    .export_dir
                    .join(format!("Route-{}.gpx", now.format("%Y-%m-%d_%H-%M-%S")));
                match self.route.export_gpx(&path) {
                    Ok(()) => crate::diagnostics::record(
                        "INFO",
                        format!("Route exportée: {}", path.display()),
                    ),
                    Err(error) => {
                        crate::diagnostics::record(
                            "ERROR",
                            format!("Erreur export route GPX: {error}"),
                        );
                    }
                }
            }
            AppAction::ExportTrackGpx => {
                let now = Local::now();
                let path = self
                    .export_dir
                    .join(format!("Track-{}.gpx", now.format("%Y-%m-%d_%H-%M-%S")));
                match self.track.export_gpx(&path) {
                    Ok(()) => crate::diagnostics::record(
                        "INFO",
                        format!("Trace exportée: {}", path.display()),
                    ),
                    Err(error) => {
                        crate::diagnostics::record(
                            "ERROR",
                            format!("Erreur export trace GPX: {error}"),
                        );
                    }
                }
            }
            AppAction::ClearWaypoints => {
                self.confirmation.request(ConfirmAction::ClearWaypoints);
            }
            AppAction::ExportWaypointsGpx => {
                let now = Local::now();
                let path = self
                    .export_dir
                    .join(format!("Waypoints-{}.gpx", now.format("%Y-%m-%d_%H-%M-%S")));
                match self.waypoints.export_gpx(&path) {
                    Ok(()) => crate::diagnostics::record(
                        "INFO",
                        format!("Waypoints exportés: {}", path.display()),
                    ),
                    Err(error) => {
                        crate::diagnostics::record(
                            "ERROR",
                            format!("Erreur export waypoints GPX: {error}"),
                        );
                    }
                }
            }
            AppAction::Pan(dx, dy) => {
                if self.following_boat() {
                    // The boat stays the anchor; the drag is an offset from it.
                    self.state.map_offset.0 += dx;
                    self.state.map_offset.1 += dy;
                } else if let Some(center) = self.renderer.view().panned_center(dx, dy) {
                    // Nothing to be offset from: the saved position moves.
                    self.state.position = Some(center);
                    self.state.map_offset = (0, 0);
                }
            }
            AppAction::ZoomIn => {
                // As deep as the most detailed file here goes, no deeper.
                if self.state.zoom < self.zoom_limit() {
                    self.anchor_on_visible_center();
                    self.zoom_to(self.state.zoom + 1);
                }
            }
            AppAction::ZoomOut => {
                // Below every file's first level the default chart shows.
                if self.state.zoom > 0 {
                    self.anchor_on_visible_center();
                    self.zoom_to(self.state.zoom - 1);
                }
            }
            AppAction::HideWaypoints => self.state.display_waypoint = false,
            AppAction::ShowWaypoints => self.state.display_waypoint = true,
            AppAction::HideRoute => self.state.display_route = false,
            AppAction::ShowRoute => self.state.display_route = true,
            AppAction::HideTrack => self.state.display_track = false,
            AppAction::ShowTrack => self.state.display_track = true,
            AppAction::HideAis => self.state.display_ais = false,
            AppAction::ShowAis => self.state.display_ais = true,
            AppAction::SetCourseUp(enabled) => {
                self.state.course_up = enabled;
                self.state.heading_up = false;
            }
            AppAction::SetHeadingUp => {
                self.state.heading_up = true;
                self.state.course_up = false;
            }
            AppAction::ToggleMagnetic => self.state.magnetic = !self.state.magnetic,
        }

        self.sync_view();
    }

    pub(super) fn apply_confirmation(&mut self, action: ConfirmAction) {
        match action {
            ConfirmAction::ClearRoute => {
                self.state.goto = false;
                if let Err(error) = self.route.clear() {
                    crate::diagnostics::record(
                        "ERROR",
                        format!("Erreur sauvegarde route: {error}"),
                    );
                }
            }
            ConfirmAction::ClearTrack => {
                self.track.clear(std::time::Instant::now());
            }
            ConfirmAction::ClearWaypoints => {
                self.waypoints.reset();
                if let Err(error) = self.waypoints.save() {
                    crate::diagnostics::record(
                        "ERROR",
                        format!("Erreur sauvegarde des waypoints: {error}"),
                    );
                }
            }
        }
        self.sync_view();
    }

    /// True while the view is anchored on a live boat fix.
    fn following_boat(&self) -> bool {
        self.state.follow && super::session::valid_boat_position(&self.boat)
    }

    /// Unless the boat is the anchor, folds the pan offset into the saved
    /// position so that whatever changes the zoom keeps the visible centre.
    /// A pixel offset is a different distance at each zoom; a position is not.
    fn anchor_on_visible_center(&mut self) {
        if !self.following_boat() {
            self.center_map_on_visible_center();
        }
    }

    /// The navigator chose a layer: automatic switching stays in it, the
    /// lock is released, and the layer is remembered for its role.
    pub(super) fn choose_layer(&mut self, layer: Option<String>) {
        self.state.lock = None;
        let was_photo = self.on_photo_layer();
        if let Some(layer) = &layer {
            match self.catalogue.role_of(layer) {
                Role::Chart => self.state.chart_layer = Some(layer.clone()),
                Role::Satellite | Role::Map => self.state.imagery_layer = Some(layer.clone()),
            }
        }
        self.state.layer = layer;
        // A round of photos starts from a chart and keeps its order to the
        // end, even though the last one used changes on every step.
        if !self.on_photo_layer() {
            self.photo_round = None;
        } else if !was_photo {
            self.photo_round = self.state.layer.clone();
        }
    }

    fn zoom_to(&mut self, zoom: i32) {
        self.renderer.zoom_to(zoom);
        self.state.zoom = zoom;
    }

    pub(super) fn center_map_on_visible_center(&mut self) {
        if let Some(position) = self.state.position {
            self.state.position = Some(
                self.renderer
                    .center_with_offset(position, self.state.map_offset),
            );
            self.state.map_offset = (0, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::session::test_support::{
        session, session_with_bounded_charts, session_with_charts,
    };
    use super::*;

    #[test]
    fn show_and_hide_actions_flip_the_overlay_flags() {
        let mut fx = session(&[2]);
        fx.session.execute_action(AppAction::HideRoute);
        assert!(!fx.session.state.display_route);
        fx.session.execute_action(AppAction::ShowRoute);
        assert!(fx.session.state.display_route);
        fx.session.execute_action(AppAction::HideAis);
        assert!(!fx.session.state.display_ais);
        fx.session.execute_action(AppAction::HideWaypoints);
        assert!(!fx.session.state.display_waypoint);
        fx.session.execute_action(AppAction::HideTrack);
        assert!(!fx.session.state.display_track);
        fx.session.execute_action(AppAction::SetCourseUp(true));
        assert!(fx.session.state.course_up);
        fx.finish();
    }

    #[test]
    fn next_waypoint_stops_at_the_last_point_and_previous_at_the_first() {
        let mut fx = session(&[2]);
        for _ in 0..5 {
            fx.session.execute_action(AppAction::NextWaypoint);
        }
        assert_eq!(fx.session.state.nav_id, 2, "three points: ids 0..=2");
        for _ in 0..5 {
            fx.session.execute_action(AppAction::PreviousWaypoint);
        }
        assert_eq!(fx.session.state.nav_id, 0);
        fx.finish();
    }

    #[test]
    fn toggling_goto_restarts_navigation_at_the_first_waypoint() {
        let mut fx = session(&[2]);
        fx.session.execute_action(AppAction::NextWaypoint);
        fx.session.execute_action(AppAction::ToggleGoto);
        assert!(fx.session.state.goto);
        assert_eq!(fx.session.state.nav_id, 0);
        fx.finish();
    }

    #[test]
    fn magnetic_bearings_toggle_and_use_the_variation_in_use() {
        let mut fx = session(&[2]);
        let now = std::time::Instant::now();
        let boat = crate::boat::BoatState {
            variation: Some(-3.1),
            variation_at: Some(now),
            ..valid_boat()
        };
        fx.session.on_boat(boat, now);
        assert!(!fx.session.state.magnetic, "off by default");
        assert_eq!(fx.session.bearing_variation(), None, "true bearings");
        fx.session.execute_action(AppAction::ToggleMagnetic);
        assert!(fx.session.state.magnetic);
        assert_eq!(fx.session.bearing_variation(), Some(-3.1));
        fx.session.execute_action(AppAction::ToggleMagnetic);
        assert_eq!(fx.session.bearing_variation(), None);
        fx.finish();
    }

    fn valid_boat() -> crate::boat::BoatState {
        crate::boat::BoatState {
            longitude: 0.0,
            latitude: 0.0,
            position_valid: true,
            speed_over_ground: 4.0,
            course_over_ground: 0.0,
            heading: None,
            fixed_at: Some(std::time::Instant::now()),
            heading_at: None,
            ..crate::boat::BoatState::default()
        }
    }

    fn visible_centre(session: &Session) -> (f64, f64) {
        session
            .renderer
            .view()
            .to_lonlat(sdl2::rect::Point::new(256, 128))
            .unwrap()
    }

    fn assert_close(actual: (f64, f64), expected: (f64, f64)) {
        // Offsets are whole pixels; allow one pixel of rounding at zoom 2.
        assert!(
            (actual.0 - expected.0).abs() < 0.4 && (actual.1 - expected.1).abs() < 0.4,
            "got {actual:?}, expected {expected:?}"
        );
    }

    #[test]
    fn panning_while_following_without_a_fix_moves_the_position_like_not_following() {
        let mut fx = session(&[2]);
        fx.session.state.follow = true;
        fx.session.state.position = Some((0.0, 0.0));
        fx.session.sync_view();
        fx.session.execute_action(AppAction::Pan(100, 0));
        assert_eq!(
            fx.session.state.map_offset,
            (0, 0),
            "no boat to be offset from"
        );
        let (lon, _) = fx.session.state.position.unwrap();
        assert!(lon < 0.0, "the saved position moved: {lon}");
        fx.finish();
    }

    #[test]
    fn zooming_keeps_the_visible_centre_when_the_view_was_dragged() {
        let mut fx = session(&[2, 3]);
        fx.session.state.follow = false;
        fx.session.state.position = Some((0.0, 0.0));
        fx.session.state.map_offset = (90, 30);
        fx.session.sync_view();
        let before = visible_centre(&fx.session);

        fx.session.execute_action(AppAction::ZoomOut);
        std::thread::sleep(std::time::Duration::from_millis(300));
        fx.session.renderer.advance_zoom_animation();
        fx.session.sync_view();

        assert_close(visible_centre(&fx.session), before);
        assert_eq!(fx.session.state.map_offset, (0, 0));
        fx.finish();
    }

    #[test]
    fn panning_while_following_drags_the_chart_by_an_offset() {
        let mut fx = session(&[2]);
        fx.session.state.follow = true;
        fx.session.boat = valid_boat();
        fx.session.execute_action(AppAction::Pan(30, -10));
        fx.session.execute_action(AppAction::Pan(5, 5));
        assert_eq!(fx.session.state.map_offset, (35, -5));
        fx.finish();
    }

    #[test]
    fn centre_on_boat_follows_the_boat_from_wherever_the_chart_was() {
        let mut fx = session(&[2]);
        fx.session.boat = crate::boat::BoatState {
            longitude: 10.0,
            latitude: 20.0,
            ..valid_boat()
        };
        fx.session.state.follow = false;
        fx.session.state.position = Some((0.0, 0.0));
        fx.session.state.map_offset = (40, -30);
        assert!(!fx.session.centre_button().centred);

        fx.session.execute_action(AppAction::CentreOnBoat);
        assert!(fx.session.state.follow);
        assert_eq!(fx.session.state.map_offset, (0, 0));
        let (lon, lat) = visible_centre(&fx.session);
        assert!((lon - 10.0).abs() < 1e-6 && (lat - 20.0).abs() < 1e-6);
        assert!(fx.session.centre_button().centred);
        fx.finish();
    }

    #[test]
    fn centre_on_boat_with_a_stale_fix_goes_to_the_last_position() {
        let mut fx = session(&[2]);
        fx.session.boat = crate::boat::BoatState {
            longitude: 10.0,
            latitude: 20.0,
            position_valid: false,
            ..valid_boat()
        };
        fx.session.state.follow = false;
        fx.session.state.position = Some((0.0, 0.0));
        assert!(fx.session.centre_button().enabled);
        fx.session.execute_action(AppAction::CentreOnBoat);
        assert!(fx.session.state.follow);
        let (lon, lat) = visible_centre(&fx.session);
        assert!((lon - 10.0).abs() < 1e-6 && (lat - 20.0).abs() < 1e-6);
        fx.finish();
    }

    #[test]
    fn centre_on_boat_does_nothing_before_the_first_fix() {
        let mut fx = session(&[2]);
        fx.session.boat = crate::boat::BoatState::default();
        fx.session.state.follow = false;
        fx.session.state.position = Some((0.0, 0.0));
        assert!(!fx.session.centre_button().enabled);
        fx.session.execute_action(AppAction::CentreOnBoat);
        assert!(!fx.session.state.follow);
        assert_eq!(fx.session.state.position, Some((0.0, 0.0)));
        fx.finish();
    }

    #[test]
    fn heading_up_turns_the_chart_to_the_heading_and_north_up_without_one() {
        let mut fx = session(&[2]);
        fx.session.boat = crate::boat::BoatState {
            course_over_ground: 0.5,
            heading: Some(1.2),
            heading_at: Some(std::time::Instant::now()),
            ..valid_boat()
        };
        // A screen angle of 0 means the chart is turned to that course.
        let up = |fx: &super::super::session::test_support::Fixture, course: f64| {
            fx.session.renderer.view().course_to_screen(course).abs() < 1e-9
        };
        fx.session.execute_action(AppAction::SetHeadingUp);
        assert!(fx.session.state.heading_up && !fx.session.state.course_up);
        assert!(up(&fx, 1.2), "heading up");

        fx.session.execute_action(AppAction::SetCourseUp(true));
        assert!(!fx.session.state.heading_up && fx.session.state.course_up);
        assert!(up(&fx, 0.5), "course up");

        fx.session.execute_action(AppAction::SetHeadingUp);
        fx.session.boat.heading = None;
        fx.session.sync_view();
        assert!(up(&fx, 0.0), "no heading: north up");

        fx.session.boat.heading = Some(1.2);
        fx.session.boat.position_valid = false;
        fx.session.sync_view();
        assert!(up(&fx, 0.0), "no fix: north up");

        fx.session.execute_action(AppAction::SetCourseUp(false));
        assert!(!fx.session.state.heading_up && !fx.session.state.course_up);
        fx.finish();
    }

    #[test]
    fn a_chart_drag_applied_in_steps_ends_where_one_pan_would() {
        // Not following; following; following course up (the boat turned).
        for (follow, course_up) in [(false, false), (true, false), (true, true)] {
            let run = |steps: &[(i32, i32)]| {
                let mut fx = session(&[2]);
                fx.session.state.follow = follow;
                fx.session.state.course_up = course_up;
                fx.session.state.position = Some((10.0, 20.0));
                fx.session.boat = crate::boat::BoatState {
                    longitude: 10.0,
                    latitude: 20.0,
                    course_over_ground: 0.7,
                    ..valid_boat()
                };
                fx.session.sync_view();
                for &total in steps {
                    fx.session.drag_chart(total);
                }
                fx.session.end_chart_drag(*steps.last().unwrap());
                let landed = (visible_centre(&fx.session), fx.session.state.map_offset);
                fx.finish();
                landed
            };
            let (stepped, stepped_offset) = run(&[(40, -10), (90, 5), (150, 30)]);
            let (once, once_offset) = run(&[(150, 30)]);
            assert!(
                (stepped.0 - once.0).abs() < 1e-6 && (stepped.1 - once.1).abs() < 1e-6,
                "follow {follow}, course up {course_up}: {stepped:?} vs {once:?}"
            );
            assert_eq!(stepped_offset, once_offset);
        }
    }

    #[test]
    fn the_dragged_texture_shows_only_the_part_of_the_drag_not_yet_applied() {
        let mut fx = session(&[2]);
        fx.session.state.follow = false;
        fx.session.state.position = Some((0.0, 0.0));
        fx.session.sync_view();
        assert_eq!(fx.session.chart_drag_shift((25, 5)), (25, 5));
        fx.session.drag_chart((40, -10));
        assert_eq!(fx.session.chart_drag_shift((90, 5)), (50, 15));
        fx.session.end_chart_drag((90, 5));
        assert_eq!(
            fx.session.chart_drag_shift((7, 7)),
            (7, 7),
            "the next drag starts afresh"
        );
        fx.session.drag_chart((30, 0));
        fx.session.start_chart_drag();
        assert_eq!(
            fx.session.chart_drag_shift((4, 4)),
            (4, 4),
            "a drag whose release was lost leaves nothing behind"
        );
        fx.finish();
    }

    #[test]
    fn panning_while_not_following_moves_the_saved_position() {
        let mut fx = session(&[2]);
        fx.session.state.follow = false;
        fx.session.state.position = Some((0.0, 0.0));
        fx.session.sync_view();
        // dx is centre minus pointer: +100 is a point 100 px left of centre.
        fx.session.execute_action(AppAction::Pan(100, 0));
        let (lon, lat) = fx.session.state.position.unwrap();
        assert!(lon < 0.0, "lon {lon}");
        assert!(lat.abs() < 1e-9, "lat {lat}");
        assert_eq!(fx.session.state.map_offset, (0, 0));
        fx.finish();
    }

    #[test]
    fn turning_follow_off_folds_the_pan_offset_into_the_position() {
        let mut fx = session(&[2]);
        fx.session.state.follow = true;
        fx.session.boat = valid_boat();
        fx.session.state.position = Some((0.0, 0.0));
        fx.session.execute_action(AppAction::Pan(100, 0));
        fx.session.execute_action(AppAction::FollowOff);
        assert!(!fx.session.state.follow);
        assert_eq!(fx.session.state.map_offset, (0, 0));
        let (lon, _) = fx.session.state.position.unwrap();
        assert!(
            lon < 0.0,
            "the dragged view is now the saved position, lon {lon}"
        );
        fx.finish();
    }

    #[test]
    fn zoom_actions_record_the_chart_zoom_in_the_state() {
        let mut fx = session(&[2, 3]);
        fx.session.execute_action(AppAction::ZoomOut);
        assert_eq!(fx.session.state.zoom, 2);
        fx.session.execute_action(AppAction::ZoomIn);
        assert_eq!(fx.session.state.zoom, 3);
        fx.session.execute_action(AppAction::ZoomIn);
        assert_eq!(fx.session.state.zoom, 3, "top of the only chart: no change");
        fx.finish();
    }

    #[test]
    fn clearing_the_route_waits_for_confirmation_and_then_saves_an_empty_route() {
        let mut fx = session(&[2]);
        fx.session.execute_action(AppAction::ClearRoute);
        assert!(fx.session.confirmation.is_open());
        assert_eq!(fx.session.route.get_nb_wp(), 3, "nothing cleared yet");

        assert_eq!(fx.session.confirmation.resolve(false), None);
        assert_eq!(fx.session.route.get_nb_wp(), 3, "cancelled");

        fx.session.state.goto = true;
        fx.session.execute_action(AppAction::ClearRoute);
        let action = fx.session.confirmation.resolve(true).unwrap();
        assert_eq!(action, ConfirmAction::ClearRoute);
        fx.session.apply_confirmation(action);
        assert_eq!(fx.session.route.get_nb_wp(), 0);
        assert!(!fx.session.state.goto, "cannot navigate an empty route");
        let saved = crate::geo_data::read(&fx.dir.join("route.json")).unwrap();
        assert!(saved.is_empty(), "the empty route reached disk");
        fx.finish();
    }

    #[test]
    fn choosing_a_layer_shows_its_best_file() {
        let mut fx =
            session_with_charts(&[("A_Navionics.mbtiles", &[2]), ("B_BingSat.mbtiles", &[2])]);
        fx.session.choose_layer(Some("Bing satellite".into()));
        fx.session.sync_view();
        assert_eq!(fx.session.state.layer.as_deref(), Some("Bing satellite"));
        assert!(fx.session.state.map.ends_with("B_BingSat.mbtiles"));
        assert!(fx.session.renderer.path().ends_with("B_BingSat.mbtiles"));
        fx.finish();
    }

    #[test]
    fn zooming_in_switches_to_the_more_detailed_file_of_the_layer_and_back() {
        let mut fx = session_with_charts(&[
            ("Ocean_Navionics.mbtiles", &[1, 2, 3]),
            ("Coast_Navionics.mbtiles", &[4, 5]),
        ]);
        fx.session.state.zoom = 3;
        fx.session.renderer.zoom_to(3);
        fx.session.sync_view();
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Ocean_Navionics.mbtiles"));

        assert_eq!(
            fx.session.renderer.fill_paths(),
            Vec::<&str>::new(),
            "Coast starts deeper"
        );
        fx.session.execute_action(AppAction::ZoomIn);
        assert_eq!(fx.session.state.zoom, 4);
        settle(&mut fx);
        assert!(
            fx.session
                .renderer
                .fill_paths()
                .first()
                .is_some_and(|p| p.ends_with("Ocean_Navionics.mbtiles")),
            "the ocean chart fills in beyond the coast chart"
        );
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Coast_Navionics.mbtiles"));
        assert!(fx.session.state.map.ends_with("Coast_Navionics.mbtiles"));

        fx.session.execute_action(AppAction::ZoomIn);
        fx.session.execute_action(AppAction::ZoomIn);
        assert_eq!(fx.session.state.zoom, 5, "no deeper than the deepest file");

        for _ in 0..2 {
            fx.session.execute_action(AppAction::ZoomOut);
        }
        assert_eq!(fx.session.state.zoom, 3);
        settle(&mut fx);
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Ocean_Navionics.mbtiles"));
        fx.finish();
    }

    #[test]
    fn below_every_files_first_level_the_default_chart_shows_and_zooming_in_returns() {
        let mut fx = session_with_charts(&[
            ("World.mbtiles", &[0, 1]),
            ("Ocean_Navionics.mbtiles", &[3, 4]),
        ]);
        fx.session.state.layer = Some("Navionics".into());
        fx.session.state.zoom = 3;
        fx.session.renderer.zoom_to(3);
        settle(&mut fx);
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Ocean_Navionics.mbtiles"));
        fx.session.execute_action(AppAction::ZoomOut);
        settle(&mut fx);
        assert!(
            fx.session.renderer.path().ends_with("World.mbtiles"),
            "World is a chart in the default layer, the fallback"
        );
        fx.session.execute_action(AppAction::ZoomIn);
        settle(&mut fx);
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Ocean_Navionics.mbtiles"));
        assert_eq!(fx.session.state.zoom, 3, "back at the level it left");
        assert!(
            fx.session.state.map.ends_with("Ocean_Navionics.mbtiles"),
            "the state records the detail chart again"
        );
        fx.finish();
    }

    /// An ocean chart at levels 1-3 and a coast chart at 4-5, one layer.
    /// Lets the zoom animation finish and syncs, as the loop would.
    fn settle(fx: &mut super::super::session::test_support::Fixture) {
        std::thread::sleep(std::time::Duration::from_millis(260));
        fx.session.renderer.advance_zoom_animation();
        fx.session.sync_view();
    }

    fn ocean_and_coast() -> super::super::session::test_support::Fixture {
        let mut fx = session_with_charts(&[
            ("Ocean_Navionics.mbtiles", &[1, 2, 3]),
            ("Coast_Navionics.mbtiles", &[4, 5]),
        ]);
        fx.session.state.zoom = 3;
        fx.session.renderer.zoom_to(3);
        fx.session.sync_view();
        fx
    }

    /// Regression: the file must follow the zoom being drawn, not the zoom
    /// reached, or a wheel burst opens a deep file while the drawing is
    /// still levels above it (the 57 s frame).
    #[test]
    fn mid_animation_the_file_follows_the_zoom_being_drawn() {
        let mut fx = ocean_and_coast();
        fx.session.execute_action(AppAction::ZoomIn);
        assert_eq!(fx.session.state.zoom, 4);
        assert!(
            fx.session
                .renderer
                .path()
                .ends_with("Ocean_Navionics.mbtiles"),
            "still drawn at zoom 3: the coast chart (from 4) waits"
        );
        settle(&mut fx);
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Coast_Navionics.mbtiles"));
        fx.finish();
    }

    #[test]
    fn a_locked_chart_stays_through_zooming_and_says_what_it_hides() {
        let mut fx = ocean_and_coast();
        fx.session.execute_action(AppAction::ToggleLock);
        assert_eq!(
            fx.session.state.lock.as_deref(),
            Some("Ocean_Navionics.mbtiles")
        );

        fx.session.execute_action(AppAction::ZoomIn);
        assert_eq!(fx.session.state.zoom, 4);
        settle(&mut fx);
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Ocean_Navionics.mbtiles"));
        let status = fx.session.chart_status();
        assert!(
            status.starts_with("Navionics: Ocean_Navionics"),
            "{}",
            status
        );
        assert!(status.contains("OVERSCALE ×2"), "{}", status);
        assert!(
            status.contains("More detail: Coast_Navionics"),
            "{}",
            status
        );
        assert!(status.contains("LOCKED"), "{}", status);
        assert!(!fx.session.showing_photo());

        fx.session.execute_action(AppAction::ToggleLock);
        assert_eq!(fx.session.state.lock, None);
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Coast_Navionics.mbtiles"));
        let status = fx.session.chart_status();
        assert!(!status.contains("LOCKED") && !status.contains("OVERSCALE"));
        fx.finish();
    }

    #[test]
    fn choosing_a_layer_releases_the_lock() {
        let mut fx = ocean_and_coast();
        fx.session.execute_action(AppAction::ToggleLock);
        fx.session.choose_layer(Some("Navionics".into()));
        assert_eq!(fx.session.state.lock, None);
        fx.finish();
    }

    #[test]
    fn a_photo_is_switched_on_and_off_by_hand() {
        let mut fx = session_with_charts(&[
            ("Ocean_Navionics.mbtiles", &[1, 2, 3]),
            ("Isle_BingSat.mbtiles", &[1, 2, 3]),
        ]);
        fx.session.state.layer = Some("Navionics".into());
        fx.session.sync_view();
        assert!(!fx.session.showing_photo());

        fx.session.execute_action(AppAction::NextPhoto);
        assert_eq!(fx.session.state.layer.as_deref(), Some("Bing satellite"));
        assert!(fx.session.renderer.path().ends_with("Isle_BingSat.mbtiles"));
        let status = fx.session.chart_status();
        assert!(fx.session.showing_photo());
        assert!(status.ends_with(" · NO DEPTHS"), "{status}");
        assert!(status.starts_with("Bing satellite: "), "{}", status);

        fx.session.execute_action(AppAction::NextPhoto);
        assert_eq!(fx.session.state.layer.as_deref(), Some("Navionics"));
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Ocean_Navionics.mbtiles"));
        assert!(!fx.session.showing_photo());
        fx.finish();
    }

    #[test]
    fn a_chart_says_nothing_about_depths_and_no_chart_here_is_the_overview_map() {
        let mut fx =
            session_with_bounded_charts(&[("Isle_Navionics.mbtiles", &[1, 2, 3], "0,0,40,40")]);
        fx.session.state.follow = false;
        fx.session.state.position = Some((20.0, 20.0));
        fx.session.sync_view();
        assert!(!fx.session.chart_status().contains("NO DEPTHS"));
        fx.session.state.position = Some((-100.0, 20.0));
        fx.session.sync_view();
        assert_eq!(fx.session.chart_status(), "Overview map");
        fx.finish();
    }

    #[test]
    fn a_map_comes_after_the_satellites_and_says_no_depths() {
        let mut fx = session_with_bounded_charts(&[
            ("Ocean_Navionics.mbtiles", &[1, 2, 3], "-180,-85,180,85"),
            ("Isle_OSM.mbtiles", &[1, 2, 3], "0,0,40,40"),
            ("Isle_BingSat.mbtiles", &[1, 2, 3], "0,0,40,40"),
        ]);
        fx.session.state.follow = false;
        fx.session.state.position = Some((20.0, 20.0));
        fx.session.state.layer = Some("Navionics".into());
        fx.session.state.chart_layer = Some("Navionics".into());
        fx.session.state.zoom = 3;
        fx.session.renderer.zoom_to(3);
        settle(&mut fx);

        fx.session.execute_action(AppAction::NextPhoto);
        assert!(fx.session.renderer.path().ends_with("Isle_BingSat.mbtiles"));
        let button = fx.session.photo_button().unwrap();
        assert!(ends(&button.preview, "Isle_OSM.mbtiles"));
        assert_eq!(button.preview_kind, Role::Map);

        fx.session.execute_action(AppAction::NextPhoto);
        assert!(fx.session.renderer.path().ends_with("Isle_OSM.mbtiles"));
        assert_eq!(fx.session.photo_button().unwrap().position, Some((2, 2)));
        let status = fx.session.chart_status();
        assert!(status.starts_with("OpenStreetMap: "), "{status}");
        assert!(status.ends_with(" · NO DEPTHS"), "{status}");

        fx.session.execute_action(AppAction::NextPhoto);
        assert_eq!(fx.session.state.layer.as_deref(), Some("Navionics"));
        fx.finish();
    }

    #[test]
    fn without_imagery_files_next_photo_changes_nothing() {
        let mut fx = ocean_and_coast();
        fx.session.execute_action(AppAction::NextPhoto);
        assert_eq!(fx.session.state.layer.as_deref(), Some("Navionics"));
        fx.finish();
    }

    /// A world ocean chart and two photos of one island (0..40 E, 0..40 N),
    /// all at levels 1-3, viewed over the island at zoom 3 on the chart.
    fn island() -> super::super::session::test_support::Fixture {
        let mut fx = session_with_bounded_charts(&[
            ("Ocean_Navionics.mbtiles", &[1, 2, 3], "-180,-85,180,85"),
            ("Isle_BingSat.mbtiles", &[1, 2, 3], "0,0,40,40"),
            ("Isle_GoogleSat.mbtiles", &[1, 2, 3], "0,0,40,40"),
        ]);
        fx.session.state.follow = false;
        fx.session.state.position = Some((20.0, 20.0));
        fx.session.state.layer = Some("Navionics".into());
        fx.session.state.chart_layer = Some("Navionics".into());
        fx.session.state.zoom = 3;
        fx.session.renderer.zoom_to(3);
        settle(&mut fx);
        fx
    }

    fn ends(preview: &Option<super::super::session::Preview>, name: &str) -> bool {
        preview.as_ref().is_some_and(|p| p.path.ends_with(name))
    }

    #[test]
    fn zooming_out_past_a_photos_first_level_keeps_the_photo_layer() {
        let mut fx = island();
        fx.session.execute_action(AppAction::NextPhoto);
        fx.session.state.zoom = 0;
        fx.session.renderer.zoom_to(0);
        settle(&mut fx);
        assert_eq!(fx.session.state.layer.as_deref(), Some("Bing satellite"));
        assert!(!fx.session.renderer.path().ends_with("Isle_BingSat.mbtiles"));
        fx.session.state.zoom = 3;
        fx.session.renderer.zoom_to(3);
        settle(&mut fx);
        assert!(fx.session.renderer.path().ends_with("Isle_BingSat.mbtiles"));
        fx.finish();
    }

    #[test]
    fn next_photo_steps_through_the_sources_here_and_back_to_the_chart() {
        let mut fx = island();
        let button = fx.session.photo_button().expect("photos here");
        assert_eq!(button.position, None);
        assert!(ends(&button.preview, "Isle_BingSat.mbtiles"));
        assert_eq!(button.preview_kind, Role::Satellite);

        fx.session.execute_action(AppAction::NextPhoto);
        assert!(fx.session.renderer.path().ends_with("Isle_BingSat.mbtiles"));
        let button = fx.session.photo_button().unwrap();
        assert_eq!(button.position, Some((1, 2)));
        assert!(ends(&button.preview, "Isle_GoogleSat.mbtiles"));

        fx.session.execute_action(AppAction::NextPhoto);
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Isle_GoogleSat.mbtiles"));
        let button = fx.session.photo_button().unwrap();
        assert_eq!(button.position, Some((2, 2)));
        assert!(ends(&button.preview, "Ocean_Navionics.mbtiles"));
        assert_eq!(button.preview_kind, Role::Chart);

        fx.session.execute_action(AppAction::NextPhoto);
        assert_eq!(fx.session.state.layer.as_deref(), Some("Navionics"));
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Ocean_Navionics.mbtiles"));
        fx.finish();
    }

    #[test]
    fn the_last_photo_used_comes_first() {
        let mut fx = island();
        fx.session.execute_action(AppAction::NextPhoto);
        fx.session.execute_action(AppAction::NextPhoto);
        fx.session.execute_action(AppAction::NextPhoto);
        let button = fx.session.photo_button().unwrap();
        assert!(ends(&button.preview, "Isle_GoogleSat.mbtiles"));
        fx.finish();
    }

    #[test]
    fn no_button_and_no_change_where_no_photo_exists() {
        let mut fx = island();
        fx.session.state.position = Some((-100.0, 20.0));
        fx.session.sync_view();
        assert!(fx.session.photo_button().is_none());
        fx.session.execute_action(AppAction::NextPhoto);
        assert_eq!(fx.session.state.layer.as_deref(), Some("Navionics"));
        fx.finish();
    }

    #[test]
    fn leaving_the_photos_coverage_drops_back_to_the_chart_for_good() {
        let mut fx = island();
        fx.session.execute_action(AppAction::NextPhoto);
        assert!(fx.session.renderer.path().ends_with("Isle_BingSat.mbtiles"));

        fx.session.state.position = Some((-100.0, 20.0));
        fx.session.sync_view();
        assert_eq!(fx.session.state.layer.as_deref(), Some("Navionics"));

        fx.session.state.position = Some((20.0, 20.0));
        fx.session.sync_view();
        assert_eq!(fx.session.state.layer.as_deref(), Some("Navionics"));
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Ocean_Navionics.mbtiles"));
        fx.finish();
    }

    #[test]
    fn a_tap_releases_the_lock() {
        let mut fx = island();
        fx.session.execute_action(AppAction::ToggleLock);
        assert!(fx.session.state.lock.is_some());
        fx.session.execute_action(AppAction::NextPhoto);
        assert_eq!(fx.session.state.lock, None);
        assert!(fx.session.renderer.path().ends_with("Isle_BingSat.mbtiles"));
        fx.finish();
    }

    #[test]
    fn a_chart_that_fails_to_open_is_logged_once_and_the_current_one_stays() {
        let mut fx = session_with_charts(&[
            ("Ocean_Navionics.mbtiles", &[1, 2]),
            ("Coast_Navionics.mbtiles", &[3]),
        ]);
        fx.session.state.zoom = 2;
        fx.session.renderer.zoom_to(2);
        fx.session.sync_view();
        std::fs::remove_file(fx.dir.join("charts/Coast_Navionics.mbtiles")).unwrap();

        fx.session.execute_action(AppAction::ZoomIn);
        assert_eq!(fx.session.state.zoom, 3);
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Ocean_Navionics.mbtiles"));
        fx.session.sync_view();
        assert!(fx
            .session
            .renderer
            .path()
            .ends_with("Ocean_Navionics.mbtiles"));
        fx.finish();
    }

    #[test]
    fn exporting_waypoints_writes_a_gpx_file_into_the_export_directory() {
        let mut fx = session(&[2]);
        fx.session.waypoints.add((1.0, 2.0), "Mooring".to_string());
        fx.session.execute_action(AppAction::ExportWaypointsGpx);
        let exports: Vec<_> = std::fs::read_dir(&fx.session.export_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(exports.len(), 1, "{exports:?}");
        let contents = std::fs::read_to_string(&exports[0]).unwrap();
        assert!(contents.contains("<wpt") && contents.contains("Mooring"));
        fx.finish();
    }

    #[test]
    fn quit_stops_the_session_and_cycle_mode_visits_every_mode() {
        let mut fx = session(&[2]);
        assert_eq!(fx.session.mode, Mode::Measurement);
        fx.session.execute_action(AppAction::CycleMode);
        assert_eq!(fx.session.mode, Mode::EditRoute);
        fx.session.execute_action(AppAction::CycleMode);
        assert_eq!(fx.session.mode, Mode::EditWaypoint);
        fx.session.execute_action(AppAction::CycleMode);
        assert_eq!(fx.session.mode, Mode::Measurement);
        fx.session.execute_action(AppAction::Quit);
        assert!(!fx.session.running);
        fx.finish();
    }
}
