//! Everything the navigator's actions act on, with no SDL handle in it:
//! the chart view, the saved state, the route, track, waypoints and
//! measurement, the boat, and the confirmation that may be pending. The
//! action layer (`actions.rs`) is implemented on this type, so it can be
//! driven by tests with a fixture chart and temporary paths. `Plotter` owns
//! one `Session` plus the SDL widgets and channels around it.

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::confirm_dialog::Confirmation;
use crate::ais::AisTarget;
use crate::boat::BoatState;
use crate::catalogue::{Bounds, Catalogue, ChartFile, Choice, Request, Role};
use crate::collision::{CollisionRisk, CollisionThresholds, RiskKind};
use crate::config::CollisionConfig;
use crate::mbtiles_renderer::MBTilesRenderer;
use crate::mesure::Mesure;
use crate::route::Route;
use crate::state::State;
use crate::track::Track;
use crate::waypoints::Waypoints;
use crate::Mode;

/// What the centre button shows. See `Session::centre_button`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CentreButton {
    /// A tap centres on the boat; greyed and inert before the first fix.
    pub enabled: bool,
    /// The view follows the boat with no pan offset: the button is filled.
    pub centred: bool,
}

pub(crate) struct Session {
    pub renderer: MBTilesRenderer,
    pub state: State,
    pub mode: Mode,
    pub running: bool,
    pub boat: BoatState,
    /// The AIS targets currently shown, as last published by the Signal K
    /// task and thinned by `expire_ais`.
    pub ais_targets: Vec<AisTarget>,
    last_ais_expiry: Instant,
    /// The AIS target whose details window is open, by context.
    pub selected_ais: Option<String>,
    /// The AIS targets inside the collision limits, from the last fix and
    /// AIS list.
    pub collision_risks: Vec<CollisionRisk>,
    collision_config: CollisionConfig,
    pub route: Route,
    pub mesure: Mesure,
    pub track: Track,
    pub waypoints: Waypoints,
    /// Every chart file and what the navigator decided about it.
    pub catalogue: Catalogue,
    /// Shown when nothing in the catalogue fits the view (`charts.default`).
    default_chart: String,
    /// Files that failed to open, not retried every frame.
    unavailable: HashSet<String>,
    /// The last choice, as logged, so a change is logged once.
    last_choice: String,
    /// The photo layer the current round of photos started from: their
    /// order stays put while `NextPhoto` steps through them.
    pub(super) photo_round: Option<String>,
    /// Coverage outlined on the chart by the manager's "Where".
    pub highlight: Option<crate::catalogue::Bounds>,
    pub confirmation: Confirmation,
    /// Where GPX exports are written.
    pub export_dir: PathBuf,
    /// How much of the chart drag under way has been applied as a pan.
    pub(super) chart_drag: (i32, i32),
}

impl Session {
    /// A session over the given parts, with no AIS targets yet.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        renderer: MBTilesRenderer,
        state: State,
        boat: BoatState,
        route: Route,
        track: Track,
        waypoints: Waypoints,
        catalogue: Catalogue,
        default_chart: String,
        export_dir: PathBuf,
        collision_config: CollisionConfig,
    ) -> Self {
        let mut session = Session {
            chart_drag: (0, 0),
            renderer,
            state,
            mode: Mode::default(),
            running: true,
            boat,
            ais_targets: Vec::new(),
            selected_ais: None,
            collision_risks: Vec::new(),
            collision_config,
            last_ais_expiry: Instant::now(),
            route,
            mesure: Mesure::new(),
            track,
            waypoints,
            catalogue,
            default_chart,
            unavailable: HashSet::new(),
            last_choice: String::new(),
            photo_round: None,
            highlight: None,
            confirmation: Confirmation::default(),
            export_dir,
        };
        // A state saved before layers existed: the layer of the chart it
        // showed. The renderer may have settled on another zoom.
        if session.state.layer.is_none() {
            session.state.layer = session
                .catalogue
                .visible()
                .find(|f| f.path.to_string_lossy() == session.state.map)
                .map(|f| f.layer.clone());
            if let Some(layer) = session.state.layer.clone() {
                match session.catalogue.role_of(&layer) {
                    Role::Chart => session.state.chart_layer = Some(layer),
                    Role::Satellite | Role::Map => session.state.imagery_layer = Some(layer),
                }
            }
        }
        session.state.zoom = session.renderer.zoom();
        // The renderer knows where it is looking before the first event or frame.
        session.sync_view();
        session
    }

    /// A new boat state from the source task, received at `now`. While
    /// following, the view moves with a valid fix; an invalid fix is kept
    /// (the marker goes grey) but never followed. Logs when the fix becomes valid or invalid.
    pub fn on_boat(&mut self, boat: BoatState, now: Instant) {
        // The source does not clear an old fix itself: apply the timeouts
        // first, so an update with no new position cannot revive it.
        let boat = boat.expired(now);
        log_fix_change(self.boat.position_valid, boat.position_valid);
        self.boat = boat;
        // A compass heading becomes true here, where the variation source
        // is chosen: the live one, else the magnetic model.
        if boat.heading.is_none() {
            if let Some(magnetic) = boat.magnetic_heading {
                let variation = self.variation();
                self.boat.heading = variation.map(|variation| {
                    (magnetic + variation.degrees.to_radians()).rem_euclid(std::f64::consts::TAU)
                });
            }
        }
        self.update_collision_risks();
        if self.state.follow && valid_boat_position(&boat) {
            self.state.position = Some((boat.longitude, boat.latitude));
        }
    }

    /// The variation in use: the live one, else the magnetic model's at the
    /// boat (or, before the first fix, at the chart centre). The model's
    /// date is the GPS date while the fix is valid, else the system clock.
    pub fn variation(&self) -> Option<crate::magnetic::Variation> {
        self.variation_on(chrono::Utc::now().date_naive())
    }

    /// `variation` with the system clock's date given as `today`.
    pub fn variation_on(&self, today: chrono::NaiveDate) -> Option<crate::magnetic::Variation> {
        let position = if self.boat.fixed_at.is_some() {
            Some(self.boat.position())
        } else {
            self.state.position
        };
        let date = self
            .boat
            .gps_date
            .filter(|_| self.boat.position_valid)
            .unwrap_or(today);
        let year = crate::magnetic::decimal_year(date);
        crate::magnetic::warn_once_outside_model(year);
        crate::magnetic::in_use(self.boat.variation, position, year)
    }

    /// The variation to show own-boat bearings in magnetic, or `None` for
    /// true bearings: the toggle is off, or there is no variation (near a
    /// magnetic pole).
    pub fn bearing_variation(&self) -> Option<f64> {
        self.state
            .magnetic
            .then(|| self.variation())
            .flatten()
            .map(|variation| variation.degrees)
    }

    /// The bottom bar's `COG 057°T  HDG 095°T`: magnetic with the toggle on,
    /// `--` for a course that is noise (no fix, too slow) or no heading.
    pub fn motion_text(&self) -> String {
        let variation = self.bearing_variation();
        let text = |radians: Option<f64>| match radians {
            Some(radians) => crate::magnetic::bearing_text(radians.to_degrees(), variation),
            None => "--".to_string(),
        };
        format!(
            "COG {}  HDG {}",
            text(self.boat.course()),
            text(self.boat.heading)
        )
    }

    /// Applies the fix, heading and variation timeouts
    /// (`BoatState::expired`) to the boat. Returns true when the fix, the
    /// heading or the live variation was lost, so the frame is redrawn.
    pub fn expire_boat(&mut self, now: Instant) -> bool {
        let expired = self.boat.expired(now);
        let changed = expired.position_valid != self.boat.position_valid
            || expired.heading.is_some() != self.boat.heading.is_some()
            || expired.variation.is_some() != self.boat.variation.is_some();
        log_fix_change(self.boat.position_valid, expired.position_valid);
        self.boat = expired;
        if changed {
            self.update_collision_risks();
        }
        changed
    }

    /// The AIS list as the Signal K task last published it.
    pub fn set_ais(&mut self, targets: Vec<AisTarget>) {
        self.ais_targets = targets;
        self.forget_missing_selection();
        self.update_collision_risks();
    }

    /// Recomputes `collision_risks` from the boat and the AIS list. With no
    /// valid fix there is no risk to compute.
    fn update_collision_risks(&mut self) {
        self.collision_risks = if self.collision_config.enabled {
            crate::collision::risks(
                self.boat,
                &self.ais_targets,
                CollisionThresholds {
                    cpa_nm: self.collision_config.cpa_nm,
                    tcpa_minutes: self.collision_config.tcpa_minutes,
                    proximity_nm: self.collision_config.proximity_nm,
                },
            )
        } else {
            Vec::new()
        };
    }

    pub fn collision_alarm_active(&self) -> bool {
        !self.collision_risks.is_empty()
    }

    /// True when a risk is a target close by with no motion data.
    pub fn collision_alarm_is_uncertain(&self) -> bool {
        self.collision_risks
            .iter()
            .any(|risk| risk.kind == RiskKind::ProximityUncertain)
    }

    /// The AIS target within a tap of `pixel` (20 px), by context.
    pub fn ais_at(&self, pixel: sdl2::rect::Point) -> Option<String> {
        let placed: Vec<&AisTarget> = self
            .ais_targets
            .iter()
            .filter(|target| target.position.is_some())
            .collect();
        let view = self.renderer.view();
        let index = view.nearest(placed.iter().filter_map(|t| t.position), pixel, 20)?;
        Some(placed[index].context.clone())
    }

    /// The target whose details window is open, while it is still heard.
    pub fn selected_ais_target(&self) -> Option<&AisTarget> {
        let context = self.selected_ais.as_deref()?;
        self.ais_targets.iter().find(|t| t.context == context)
    }

    /// A target that expired closes its details window.
    fn forget_missing_selection(&mut self) {
        if self.selected_ais.is_some() && self.selected_ais_target().is_none() {
            self.selected_ais = None;
        }
    }

    /// Drops targets silent for longer than `ais::TARGET_TIMEOUT`, checking
    /// at most once a second. The Signal K task applies the same rule when a
    /// delta arrives; this pass covers a connection that has gone quiet.
    /// Returns true when the list changed.
    pub fn expire_ais(&mut self, now: Instant) -> bool {
        if self.ais_targets.is_empty()
            || now.duration_since(self.last_ais_expiry) < Duration::from_secs(1)
        {
            return false;
        }
        self.last_ais_expiry = now;
        let changed = crate::ais::expire(&mut self.ais_targets, now);
        self.forget_missing_selection();
        if changed {
            self.update_collision_risks();
        }
        changed
    }

    /// The position at the centre of the screen: the boat while following,
    /// otherwise the saved position, falling back to the chart's own centre
    /// the first time. Pushes it, the pan offset and the course-up rotation
    /// into the renderer so `screen_to_gps`, `gps_to_point`, prefetching and
    /// the next frame all use the same geometry.
    /// Pushes the view derived from the state into the renderer, then shows
    /// the chart file the catalogue chooses for it.
    pub fn sync_view(&mut self) {
        self.push_view();
        self.show_chosen_chart();
    }

    /// What `Catalogue::choose` says about the current view.
    pub fn chart_choice(&self) -> Choice<'_> {
        let (centre, zoom) = self.view_centre_and_zoom();
        self.catalogue.choose(Request {
            layer: self.state.layer.as_deref(),
            lock: self.state.lock.as_deref(),
            centre,
            zoom,
        })
    }

    /// The centre of the screen and the zoom being drawn, not the one
    /// reached: during the animation a file deeper than the drawing would be
    /// drawn levels above its first level (a wheel burst onto the Azores
    /// cost a 57 s frame).
    pub(super) fn view_centre_and_zoom(&self) -> ((f64, f64), i32) {
        let centre = self
            .renderer
            .view()
            .panned_center(0, 0)
            .or(self.state.position)
            .unwrap_or_default();
        (centre, self.renderer.display_zoom().round() as i32)
    }

    /// The photo sources available at the view, in the order `NextPhoto`
    /// steps through them: the round's first, else the last one used.
    fn photo_sources(&self) -> Vec<&ChartFile> {
        let (centre, zoom) = self.view_centre_and_zoom();
        let first = self
            .photo_round
            .as_deref()
            .or(self.state.imagery_layer.as_deref());
        self.catalogue.photo_sources(centre, zoom, first)
    }

    /// The chosen layer is a photo layer.
    pub(super) fn on_photo_layer(&self) -> bool {
        self.state
            .layer
            .as_deref()
            .is_some_and(|layer| self.catalogue.role_of(layer) != Role::Chart)
    }

    /// The file shown is a photo.
    pub(super) fn showing_photo(&self) -> bool {
        self.chart_choice()
            .file
            .is_some_and(|f| self.catalogue.role_of(&f.layer) != Role::Chart)
    }

    /// Where the shown photo is among the sources here, 0-based.
    fn photo_index(&self, sources: &[&ChartFile]) -> Option<usize> {
        let shown = self.chart_choice().file?;
        if self.catalogue.role_of(&shown.layer) == Role::Chart {
            return None;
        }
        sources.iter().position(|f| f.layer == shown.layer)
    }

    /// The photo layer one `NextPhoto` goes to; `None` means the chart
    /// (from a photo) or nothing at all (from a chart).
    pub(super) fn next_photo_layer(&self) -> Option<String> {
        let sources = self.photo_sources();
        let next = match self.photo_index(&sources) {
            Some(k) => k + 1,
            None if self.showing_photo() => sources.len(),
            None => 0,
        };
        sources.get(next).map(|f| f.layer.clone())
    }

    /// The centre button: usable once the boat has had a fix; shown as
    /// centred while the view follows the boat with no pan offset.
    pub fn centre_button(&self) -> CentreButton {
        let enabled = self.boat.fixed_at.is_some();
        CentreButton {
            enabled,
            centred: enabled && self.state.follow && self.state.map_offset == (0, 0),
        }
    }

    /// The photo button, while a photo source is available here or a photo
    /// is shown: what a tap goes to, and which source is on.
    pub fn photo_button(&self) -> Option<PhotoButton> {
        let sources = self.photo_sources();
        let showing = self.showing_photo();
        if sources.is_empty() && !showing {
            return None;
        }
        let index = self.photo_index(&sources);
        let next = match index {
            Some(k) => sources.get(k + 1),
            None if showing => None,
            None => sources.first(),
        };
        let (preview, preview_kind) = match next {
            Some(file) => (Some(Preview::of(file)), self.catalogue.role_of(&file.layer)),
            None => {
                let (centre, zoom) = self.view_centre_and_zoom();
                let chart = self.catalogue.choose(Request {
                    layer: self.state.chart_layer.as_deref(),
                    lock: None,
                    centre,
                    zoom,
                });
                (chart.file.map(Preview::of), Role::Chart)
            }
        };
        Some(PhotoButton {
            preview,
            preview_kind,
            position: index.map(|k| (k + 1, sources.len())),
        })
    }

    /// What the bottom bar says about the chart shown.
    pub fn chart_status(&self) -> String {
        let choice = self.chart_choice();
        let mut text = match choice.file {
            Some(file) => format!("{}: {}", file.layer, file.name),
            None => "Overview map".to_string(),
        };
        if let Some(levels) = choice.overscale {
            text.push_str(&format!(" · OVERSCALE ×{}", 1u32 << levels.clamp(0, 20)));
        }
        if let (Some(from), Some(file)) = (choice.fallback_from, choice.file) {
            text.push_str(&format!(" · No {from} here, showing {}", file.layer));
        }
        if let Some(file) = choice.more_detail {
            text.push_str(&format!(" · More detail: {}", file.name));
        }
        if self.state.lock.is_some() && choice.file.is_some() {
            text.push_str(if choice.outside_lock {
                " · LOCKED · Outside locked chart"
            } else {
                " · LOCKED"
            });
        }
        if choice
            .file
            .is_some_and(|f| self.catalogue.role_of(&f.layer) != Role::Chart)
        {
            // A photo or a land map: nothing on screen gives a depth.
            text.push_str(" · NO DEPTHS");
        }
        text
    }

    /// The deepest zoom worth zooming in to at the view centre.
    pub fn zoom_limit(&self) -> i32 {
        self.chart_choice()
            .zoom_limit
            .unwrap_or(0)
            .max(self.renderer.top_zoom())
    }

    fn show_chosen_chart(&mut self) {
        // A photo layer with no file over the centre: back to the chart
        // for good, so the photo never comes back without a tap. Zooming
        // out past its first level is not leaving it: the chart shows as a
        // fallback and the photo returns on zooming in.
        let photo_left = self.on_photo_layer() && {
            let (centre, _) = self.view_centre_and_zoom();
            self.state
                .layer
                .as_deref()
                .is_some_and(|layer| !self.catalogue.covers(layer, centre))
        };
        if photo_left {
            crate::diagnostics::record("INFO", "Photo hors couverture, retour à la carte");
            self.choose_layer(self.state.chart_layer.clone());
        }
        let choice = self.chart_choice();
        let target = choice.file.map_or_else(
            || self.default_chart.clone(),
            |file| file.path.to_string_lossy().into_owned(),
        );
        let (_, zoom) = self.view_centre_and_zoom();
        let screen = Bounds::of_screen(self.renderer.view().lonlat_bounds());
        let fills: Vec<&ChartFile> = choice
            .file
            .map(|shown| self.catalogue.fills(shown, screen, zoom))
            .unwrap_or_default();
        let mut summary = describe_choice(&choice, &target);
        if !fills.is_empty() {
            let names: Vec<&str> = fills.iter().map(|f| f.name.as_str()).collect();
            summary.push_str(&format!(", dessous: {}", names.join(", ")));
        }
        let fills: Vec<(String, Vec<i32>)> = fills
            .iter()
            .map(|f| (f.path.to_string_lossy().into_owned(), f.zooms.clone()))
            .filter(|(path, _)| !self.unavailable.contains(path))
            .collect();
        if summary != self.last_choice {
            crate::diagnostics::record("DEBUG", format!("Carte choisie: {summary}"));
            self.last_choice = summary;
        }
        if target != self.renderer.path() && !self.unavailable.contains(&target) {
            // Same zoom, same view: the file changes under it, offset included.
            match self.renderer.change_map(&target) {
                Ok(()) => {
                    crate::diagnostics::record("INFO", format!("Carte affichée: {target}"));
                    self.state.map = target;
                }
                Err(error) => self.mark_unavailable(target, error),
            }
        }
        let wanted: Vec<(&str, &[i32])> = fills
            .iter()
            .map(|(path, levels)| (path.as_str(), levels.as_slice()))
            .collect();
        if wanted.iter().map(|w| w.0).ne(self.renderer.fill_paths()) {
            for (path, error) in self.renderer.set_fills(&wanted) {
                self.mark_unavailable(path, error);
            }
        }
    }

    fn mark_unavailable(&mut self, path: String, error: rusqlite::Error) {
        crate::diagnostics::record("ERROR", format!("Impossible d'ouvrir {path}: {error}"));
        self.unavailable.insert(path);
    }

    fn push_view(&mut self) {
        let saved = *self
            .state
            .position
            .get_or_insert_with(|| self.renderer.initial_center());
        let (lon, lat) = if self.state.follow && valid_boat_position(&self.boat) {
            (self.boat.longitude, self.boat.latitude)
        } else {
            saved
        };
        // Heading up or course up only with a valid fix (and a heading);
        // otherwise north up.
        let heading = self.boat.heading.filter(|h| h.is_finite());
        let rotation = match (self.boat.position_valid, heading) {
            (true, Some(heading)) if self.state.heading_up => heading,
            (true, _) if self.state.course_up && self.boat.course_over_ground.is_finite() => {
                self.boat.course_over_ground
            }
            _ => 0.0,
        };
        self.renderer.set_rotation(rotation);
        self.renderer.look_at(lon, lat, self.state.map_offset);
    }
}

/// The photo button: the file whose tile previews where a tap goes (the
/// next photo source, or the chart after the last), and while a photo is
/// shown, which source of how many it is (1-based).
#[derive(Debug, Clone, PartialEq)]
pub struct PhotoButton {
    pub preview: Option<Preview>,
    /// What the preview is, for the label drawn when it has no tile there.
    pub preview_kind: Role,
    /// Which source is shown, of how many.
    pub position: Option<(usize, usize)>,
}

/// The file a thumbnail is cut from, with the levels the catalogue read
/// from its tiles (so drawing never scans the file for them).
#[derive(Debug, Clone, PartialEq)]
pub struct Preview {
    pub path: PathBuf,
    pub levels: Vec<i32>,
}

impl Preview {
    fn of(file: &ChartFile) -> Self {
        Preview {
            path: file.path.clone(),
            levels: file.zooms.clone(),
        }
    }
}

/// One line for the log: the file and every indication that goes with it.
fn describe_choice(choice: &Choice, target: &str) -> String {
    let mut line = target.to_string();
    if let Some(levels) = choice.overscale {
        line.push_str(&format!(", surzoom ×{}", 1u32 << levels.clamp(0, 20)));
    }
    if let Some(file) = choice.more_detail {
        line.push_str(&format!(", plus détaillée: {}", file.name));
    }
    if let Some(layer) = choice.fallback_from {
        line.push_str(&format!(", rien dans la couche {layer} ici"));
    }
    if choice.outside_lock {
        line.push_str(", hors de la carte verrouillée");
    }
    line
}

pub(crate) fn valid_boat_position(boat: &BoatState) -> bool {
    boat.position_valid
        && boat.longitude.is_finite()
        && boat.latitude.is_finite()
        && (-180.0..=180.0).contains(&boat.longitude)
        && (-90.0..=90.0).contains(&boat.latitude)
}

/// The Session fixture shared by the action and tool tests.
#[cfg(test)]
pub(in crate::app) mod test_support {
    use super::Session;
    use crate::boat::BoatState;
    use crate::catalogue::Catalogue;
    use crate::config::{CollisionConfig, RenderingConfig};
    use crate::mbtiles_renderer::test_support::{fixture_chart, unique_name};
    use crate::mbtiles_renderer::MBTilesRenderer;
    use crate::{route::Route, state::State, track::Track, waypoints::Waypoints};
    use std::path::PathBuf;

    /// A Session over one fixture chart, with every file under a temp dir.
    /// The route has three points along the meridian.
    pub(in crate::app) struct Fixture {
        pub dir: PathBuf,
        pub chart: PathBuf,
        pub session: Session,
    }

    pub(in crate::app) fn session(zooms: &[i32]) -> Fixture {
        session_with_charts(&[("fixture.mbtiles", zooms)])
    }

    /// A Session over fixture charts (whole-world tiles at the given zooms)
    /// named as given in `dir/charts`, catalogued, the first one open and
    /// also the default chart.
    pub(in crate::app) fn session_with_charts(charts: &[(&str, &[i32])]) -> Fixture {
        let bounded: Vec<(&str, &[i32], &str)> = charts
            .iter()
            .map(|&(name, zooms)| (name, zooms, ""))
            .collect();
        session_with_bounded_charts(&bounded)
    }

    /// As `session_with_charts`, each file declaring the given `bounds`
    /// metadata ("west,south,east,north"; empty for none), which the
    /// catalogue takes as its coverage.
    pub(in crate::app) fn session_with_bounded_charts(charts: &[(&str, &[i32], &str)]) -> Fixture {
        let dir = std::env::temp_dir().join(format!("plotter-session-{}", unique_name()));
        let chart_dir = dir.join("charts");
        std::fs::create_dir_all(&chart_dir).unwrap();
        for (name, zooms, bounds) in charts {
            let path = chart_dir.join(name);
            std::fs::rename(fixture_chart(zooms), &path).unwrap();
            if !bounds.is_empty() {
                rusqlite::Connection::open(&path)
                    .unwrap()
                    .execute("INSERT INTO metadata VALUES ('bounds', ?1)", [bounds])
                    .unwrap();
            }
        }
        let chart = chart_dir.join(charts[0].0);
        let chart_str = chart.to_str().unwrap().to_string();
        let catalogue = Catalogue::load(&chart_dir, &dir.join("charts.json"));
        let state = State::new(dir.join("state.json"), chart_str.clone()).unwrap();
        let renderer = MBTilesRenderer::new(
            &chart_str,
            state.zoom,
            512,
            256,
            &RenderingConfig::default(),
        )
        .unwrap();
        let session = Session::new(
            renderer,
            state,
            BoatState::default(),
            Route::from_points_at(
                vec![(0.0, 0.0), (0.0, 1.0), (0.0, 2.0)],
                &dir.join("route.json"),
            ),
            Track::new(&dir.join("track.json")),
            Waypoints::empty_at(&dir.join("waypoints.json")),
            catalogue,
            chart_str,
            dir.join("export"),
            CollisionConfig::default(),
        );
        Fixture {
            dir,
            chart,
            session,
        }
    }

    impl Fixture {
        /// Drops the session (which holds the chart open) before deleting.
        pub(in crate::app) fn finish(self) {
            let Fixture {
                dir,
                chart,
                session,
            } = self;
            drop(session);
            drop(chart);
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}

/// Logs when the boat's fix is gained or lost.
fn log_fix_change(was_valid: bool, is_valid: bool) {
    if was_valid != is_valid {
        crate::diagnostics::record(
            "INFO",
            if is_valid {
                "Position du bateau reçue; marqueur activé"
            } else {
                "Position du bateau invalide; marqueur grisé"
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::session;
    use crate::ais::{AisTarget, TARGET_TIMEOUT};
    use crate::boat::BoatState;
    use std::time::{Duration, Instant};

    fn boat_at(lon: f64, lat: f64) -> BoatState {
        BoatState {
            longitude: lon,
            latitude: lat,
            position_valid: true,
            speed_over_ground: 5.0,
            course_over_ground: 1.0,
            heading: None,
            fixed_at: Some(Instant::now()),
            heading_at: None,
            ..BoatState::default()
        }
    }

    #[test]
    fn an_update_without_a_new_position_does_not_revive_a_stale_fix() {
        let mut fx = session(&[2]);
        let now = Instant::now();
        let mut boat = boat_at(10.0, 20.0);
        boat.fixed_at = Some(now);
        fx.session.on_boat(boat, now);
        assert!(fx.session.expire_boat(now + Duration::from_secs(11)));
        // A heading arrives; the source still says the old fix is valid.
        boat.heading = Some(1.0);
        boat.heading_at = Some(now + Duration::from_secs(12));
        fx.session.on_boat(boat, now + Duration::from_secs(12));
        assert!(!fx.session.boat.position_valid, "the old fix stays lost");
        assert_eq!(fx.session.boat.heading, Some(1.0));
        assert!(!fx.session.expire_boat(now + Duration::from_secs(12)));
        fx.finish();
    }

    /// A boat at 13 S 59 W on 2 July 2027 (2027.4986), where NOAA gives a
    /// variation of 17.49 W at 2027.5, with a compass reading 100 degrees.
    fn compass_boat(now: Instant) -> BoatState {
        BoatState {
            fixed_at: Some(now),
            magnetic_heading: Some(100.0_f64.to_radians()),
            heading_at: Some(now),
            gps_date: chrono::NaiveDate::from_ymd_opt(2027, 7, 2),
            ..boat_at(-59.0, -13.0)
        }
    }

    #[test]
    fn a_compass_heading_is_corrected_by_the_live_variation() {
        let mut fx = session(&[2]);
        let now = Instant::now();
        let boat = BoatState {
            variation: Some(-5.0),
            variation_at: Some(now),
            ..compass_boat(now)
        };
        fx.session.on_boat(boat, now);
        let heading = fx.session.boat.heading.unwrap().to_degrees();
        assert!((heading - 95.0).abs() < 1e-9, "{heading}");
        let variation = fx.session.variation().unwrap();
        assert_eq!(variation.source, crate::magnetic::Source::Live);
        fx.finish();
    }

    #[test]
    fn a_compass_heading_with_no_live_variation_is_corrected_by_the_model() {
        let mut fx = session(&[2]);
        let now = Instant::now();
        fx.session.on_boat(compass_boat(now), now);
        let heading = fx.session.boat.heading.unwrap().to_degrees();
        assert!((heading - (100.0 - 17.49)).abs() < 0.02, "{heading}");
        let variation = fx.session.variation().unwrap();
        assert_eq!(variation.source, crate::magnetic::Source::Model);
        fx.finish();
    }

    #[test]
    fn the_bar_shows_cog_and_heading_true_or_magnetic() {
        let mut fx = session(&[2]);
        let now = Instant::now();
        // COG 1 rad = 057.3 T; heading from the compass, 100 M + 5 W = 095 T.
        let boat = BoatState {
            variation: Some(-5.0),
            variation_at: Some(now),
            ..compass_boat(now)
        };
        fx.session.on_boat(boat, now);
        assert_eq!(fx.session.motion_text(), "COG 057°T  HDG 095°T");
        fx.session
            .execute_action(crate::action::AppAction::ToggleMagnetic);
        assert_eq!(fx.session.motion_text(), "COG 062°M  HDG 100°M");
        fx.finish();
    }

    #[test]
    fn the_bar_hides_a_cog_that_is_noise_and_a_heading_that_is_missing() {
        let mut fx = session(&[2]);
        let now = Instant::now();
        // Below 1 m/s the GPS course is noise.
        let slow = BoatState {
            speed_over_ground: 0.5,
            ..boat_at(10.0, 20.0)
        };
        fx.session.on_boat(slow, now);
        assert_eq!(fx.session.motion_text(), "COG --  HDG --");
        // No fix: no course, but a compass heading still shows.
        let lost = BoatState {
            position_valid: false,
            heading: Some(0.0),
            heading_at: Some(now),
            ..boat_at(10.0, 20.0)
        };
        fx.session.on_boat(lost, now);
        assert_eq!(fx.session.motion_text(), "COG --  HDG 000°T");
        fx.finish();
    }

    #[test]
    fn the_screen_is_redrawn_when_the_live_variation_expires() {
        let mut fx = session(&[2]);
        let now = Instant::now();
        let boat = BoatState {
            variation: Some(-5.0),
            variation_at: Some(now),
            heading_at: Some(now + Duration::from_secs(5)),
            fixed_at: Some(now + Duration::from_secs(5)),
            ..compass_boat(now)
        };
        fx.session.on_boat(boat, now + Duration::from_secs(5));
        assert!(!fx.session.expire_boat(now + Duration::from_secs(9)));
        assert!(
            fx.session.expire_boat(now + Duration::from_secs(10)),
            "the bar must drop the old GPS variation"
        );
        assert_eq!(fx.session.boat.variation, None);
        fx.finish();
    }

    #[test]
    fn the_model_uses_the_gps_date_with_a_fix_and_the_clock_without() {
        let mut fx = session(&[2]);
        let now = Instant::now();
        fx.session.on_boat(compass_boat(now), now);
        let clock = chrono::NaiveDate::from_ymd_opt(2031, 1, 1).unwrap();
        // With a valid fix the GPS date (2027) wins over the clock (2031).
        let source = fx.session.variation_on(clock).unwrap().source;
        assert_eq!(source, crate::magnetic::Source::Model);
        // With the fix lost the clock gives the date: past the model.
        fx.session.boat.position_valid = false;
        let source = fx.session.variation_on(clock).unwrap().source;
        assert_eq!(source, crate::magnetic::Source::ModelExpired);
        fx.finish();
    }

    #[test]
    fn a_tap_on_an_ais_target_finds_it_and_a_tap_elsewhere_does_not() {
        let mut fx = session(&[2]);
        fx.session.state.position = Some((0.0, 0.0));
        fx.session.sync_view();
        let mut ship = AisTarget::new("vessels.urn:mrn:imo:mmsi:232001001".to_string());
        ship.position = Some((0.0, 0.0));
        let mut far = AisTarget::new("vessels.urn:mrn:imo:mmsi:232002002".to_string());
        far.position = Some((60.0, 0.0));
        let mut unplaced = AisTarget::new("vessels.urn:mrn:imo:mmsi:232003003".to_string());
        unplaced.position = None;
        fx.session.set_ais(vec![far, unplaced, ship]);
        let view = fx.session.renderer.view();
        let on = view.to_pixel((0.0, 0.0));
        assert_eq!(
            fx.session
                .ais_at(sdl2::rect::Point::new(on.x + 10, on.y - 8))
                .as_deref(),
            Some("vessels.urn:mrn:imo:mmsi:232001001")
        );
        assert_eq!(
            fx.session.ais_at(sdl2::rect::Point::new(on.x + 80, on.y)),
            None
        );
        fx.finish();
    }

    #[test]
    fn a_converging_target_raises_the_alarm_and_a_lost_fix_clears_it() {
        let mut fx = session(&[2]);
        let start = Instant::now();
        let mut boat = BoatState::default();
        boat.longitude = 0.0;
        boat.latitude = 0.0;
        boat.position_valid = true;
        boat.speed_over_ground = 5.0 * 1852.0 / 3600.0;
        boat.course_over_ground = 0.0;
        boat.fixed_at = Some(start);
        fx.session.on_boat(boat, start);
        // Dead ahead, 0.5 NM, stopped: CPA 0 in 6 minutes.
        let mut ship = AisTarget::new("vessels.urn:mrn:imo:mmsi:232001001".to_string());
        ship.position = Some((0.0, 0.5 / 60.0));
        ship.course_over_ground = Some(0.0);
        ship.speed_over_ground = Some(0.0);
        fx.session.set_ais(vec![ship]);
        assert!(fx.session.collision_alarm_active());
        assert!(!fx.session.collision_alarm_is_uncertain());
        let tcpa = fx.session.collision_risks[0].tcpa_minutes;
        assert!((tcpa - 6.0).abs() < 0.05, "{tcpa}");

        fx.session
            .expire_boat(start + crate::boat::FIX_TIMEOUT + Duration::from_secs(1));
        assert!(
            !fx.session.collision_alarm_active(),
            "no CPA from a position that is no longer known"
        );
        fx.finish();
    }

    #[test]
    fn the_selected_target_closes_when_it_expires() {
        let mut fx = session(&[2]);
        let mut ship = AisTarget::new("vessels.urn:mrn:imo:mmsi:232001001".to_string());
        ship.position = Some((0.0, 0.0));
        fx.session.set_ais(vec![ship]);
        fx.session.selected_ais = Some("vessels.urn:mrn:imo:mmsi:232001001".to_string());
        assert!(fx.session.selected_ais_target().is_some());
        fx.session.set_ais(Vec::new());
        assert!(fx.session.selected_ais_target().is_none());
        assert_eq!(
            fx.session.selected_ais, None,
            "closed, not left pointing at nothing"
        );
        fx.finish();
    }

    #[test]
    fn the_boat_goes_grey_once_when_its_fix_is_stale() {
        let mut fx = session(&[2]);
        let now = Instant::now();
        let mut boat = boat_at(10.0, 20.0);
        boat.fixed_at = Some(now);
        fx.session.on_boat(boat, now);
        assert!(!fx.session.expire_boat(now + Duration::from_secs(5)));
        assert!(
            fx.session.expire_boat(now + Duration::from_secs(10)),
            "changed: redraw"
        );
        assert!(!fx.session.boat.position_valid);
        assert!(
            !fx.session.expire_boat(now + Duration::from_secs(11)),
            "already grey"
        );
        fx.finish();
    }

    #[test]
    fn a_boat_update_moves_the_view_only_while_following() {
        let mut fx = session(&[2]);
        fx.session.state.follow = false;
        fx.session.state.position = Some((0.0, 0.0));
        fx.session.on_boat(boat_at(10.0, 20.0), Instant::now());
        assert_eq!(fx.session.boat.longitude, 10.0);
        assert_eq!(fx.session.state.position, Some((0.0, 0.0)), "not following");

        fx.session.state.follow = true;
        fx.session.on_boat(boat_at(11.0, 21.0), Instant::now());
        assert_eq!(fx.session.state.position, Some((11.0, 21.0)));

        let mut invalid = boat_at(12.0, 22.0);
        invalid.position_valid = false;
        fx.session.on_boat(invalid, Instant::now());
        assert_eq!(
            fx.session.state.position,
            Some((11.0, 21.0)),
            "invalid fix is not followed"
        );
        assert!(
            !fx.session.boat.position_valid,
            "but the boat state is kept"
        );
        fx.finish();
    }

    #[test]
    fn stale_ais_targets_expire_after_the_timeout_and_at_most_once_a_second() {
        let mut fx = session(&[2]);
        fx.session.set_ais(vec![
            AisTarget::new("vessels.a".to_string()),
            AisTarget::new("vessels.b".to_string()),
        ]);
        let now = Instant::now();
        assert!(!fx.session.expire_ais(now), "fresh targets stay");
        assert_eq!(fx.session.ais_targets.len(), 2);

        let later = now + TARGET_TIMEOUT + Duration::from_secs(1);
        assert!(
            !fx.session.expire_ais(now + Duration::from_millis(500)),
            "checked less than a second ago"
        );
        assert!(fx.session.expire_ais(later), "both expired");
        assert!(fx.session.ais_targets.is_empty());
        assert!(
            !fx.session.expire_ais(later + Duration::from_secs(2)),
            "nothing left to expire"
        );
        fx.finish();
    }
}
