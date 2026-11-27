use crate::action::AppAction;
use crate::config::FontConfig;
use crate::screen::{draw_text, TextAlign};
use crate::Mode;
use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::{BlendMode, WindowCanvas};
use sdl2::ttf::{Font, Sdl2TtfContext};

const COLOR: Color = Color::RGBA(220, 220, 220, 200);
const HOVER_COLOR: Color = Color::RGB(170, 195, 230);
const BORDER_COLOR: Color = Color::RGB(120, 120, 120);

const ITEM_WIDTH: u32 = 96; // largeur d'un titre dans la barre
const MENU_WIDTH: u32 = 170; // largeur d'un menu déroulant
const ENTRY_HEIGHT: u32 = 28; // hauteur d'une entrée du menu
pub const HEIGHT: u32 = 32; // hauteur de la barre

struct Entry {
    label: String,
    action: AppAction,
}

struct Menu {
    title: &'static str,
    entries: Vec<Entry>,
}

fn entry(label: &str, action: AppAction) -> Entry {
    Entry {
        label: label.to_string(),
        action,
    }
}

/// Menu state and hit testing. Knows nothing about SDL so it can be
/// driven from tests; `Topbar` wraps it with the font and the drawing.
pub struct MenuBar {
    menus: Vec<Menu>,
    open: Option<usize>,        // menu actuellement déroulé
    hover_title: Option<usize>, // titre survolé
    hover_entry: Option<usize>, // entrée survolée (dans le menu ouvert)
}

pub struct Topbar<'ttf> {
    font: Font<'ttf, 'static>,
    width: u32,
    bar: MenuBar,
}

impl MenuBar {
    /// The menu bar. Layers are chosen in the chart manager and photos
    /// with the photo button, so the Charts menu holds the manager and the
    /// lock only.
    pub fn new() -> Self {
        let charts = vec![
            entry("Chart manager...", AppAction::OpenChartManager),
            entry("Lock chart", AppAction::ToggleLock),
        ];
        let menus = vec![
            Menu {
                title: "App",
                entries: vec![entry("Exit", AppAction::Quit)],
            },
            Menu {
                title: "Charts",
                entries: charts,
            },
            Menu {
                title: "Route",
                entries: vec![
                    entry("Clear Route", AppAction::ClearRoute),
                    entry("Export Route GPX", AppAction::ExportRouteGpx),
                    entry("From Boat", AppAction::ToggleGoto),
                    entry("Previous Wp", AppAction::PreviousWaypoint),
                    entry("Next Wp", AppAction::NextWaypoint),
                    entry("Show Route", AppAction::ShowRoute),
                    entry("Hide Route", AppAction::HideRoute),
                ],
            },
            Menu {
                title: "Track",
                entries: vec![
                    entry("Clear Track", AppAction::ClearTrack),
                    entry("Export Track GPX", AppAction::ExportTrackGpx),
                    entry("Show Track", AppAction::ShowTrack),
                    entry("Hide Track", AppAction::HideTrack),
                ],
            },
            Menu {
                title: "Waypoints",
                entries: vec![
                    entry("Clear Waypoint", AppAction::ClearWaypoints),
                    entry("Export Waypoints GPX", AppAction::ExportWaypointsGpx),
                    entry("Show Waypoint", AppAction::ShowWaypoints),
                    entry("Hide Waypoint", AppAction::HideWaypoints),
                ],
            },
            Menu {
                title: "AIS",
                entries: vec![
                    entry("Show Ais", AppAction::ShowAis),
                    entry("Hide Ais", AppAction::HideAis),
                ],
            },
            Menu {
                title: "Mode",
                entries: vec![
                    entry("Measurement", AppAction::SetMode(Mode::Measurement)),
                    entry("Edit Route", AppAction::SetMode(Mode::EditRoute)),
                    entry("Edit Waypoint", AppAction::SetMode(Mode::EditWaypoint)),
                ],
            },
            Menu {
                title: "Orientation",
                entries: vec![
                    entry("North Up", AppAction::SetCourseUp(false)),
                    entry("Course Up", AppAction::SetCourseUp(true)),
                    entry("Heading Up", AppAction::SetHeadingUp),
                    entry("Magnetic bearings", AppAction::ToggleMagnetic),
                ],
            },
            Menu {
                title: "Follow",
                entries: vec![
                    entry("On", AppAction::FollowOn),
                    entry("Off", AppAction::FollowOff),
                ],
            },
        ];

        MenuBar {
            menus,
            open: None,
            hover_title: None,
            hover_entry: None,
        }
    }

    // ---------- Tests de position ----------

    fn menu_at(&self, x: u32) -> Option<usize> {
        let i = (x / ITEM_WIDTH) as usize;
        (i < self.menus.len()).then_some(i)
    }

    fn entry_at(&self, x: u32, y: u32) -> Option<usize> {
        let i = self.open?;
        let x0 = i as u32 * ITEM_WIDTH;
        if y < HEIGHT || x < x0 || x >= x0 + MENU_WIDTH {
            return None;
        }
        let j = ((y - HEIGHT) / ENTRY_HEIGHT) as usize;
        (j < self.menus[i].entries.len()).then_some(j)
    }

    // ---------- Événements ----------

    /// À appeler sur un clic gauche.
    /// Returns the action of the entry chosen, if the click chose one.
    pub fn event(&mut self, x: u32, y: u32) -> Option<AppAction> {
        // Clic dans la barre : ouvre / ferme le menu
        if y < HEIGHT {
            self.open = match self.menu_at(x) {
                Some(i) if self.open != Some(i) => Some(i),
                _ => None,
            };
            self.hover_entry = None;
            return None;
        }

        // Clic dans le menu déroulant
        if let (Some(i), Some(j)) = (self.open, self.entry_at(x, y)) {
            let action = self.menus[i].entries[j].action;
            self.open = None;
            self.hover_entry = None;
            return Some(action);
        }

        // Clic ailleurs : on ferme le menu
        self.open = None;
        self.hover_entry = None;
        None
    }

    /// À appeler sur MouseMotion : gère le survol.
    /// Returns true when the highlighted title, entry or open menu changed,
    /// so the caller knows the bar needs redrawing.
    pub fn motion(&mut self, x: u32, y: u32) -> bool {
        let before = (self.open, self.hover_title, self.hover_entry);
        self.hover_title = if y < HEIGHT { self.menu_at(x) } else { None };

        // Quand un menu est ouvert, survoler un autre titre l'ouvre (comme un vrai menu)
        if self.open.is_some() {
            if let Some(i) = self.hover_title {
                self.open = Some(i);
            }
        }
        self.hover_entry = self.entry_at(x, y);
        before != (self.open, self.hover_title, self.hover_entry)
    }

    /// true si la souris est sur la barre ou sur le menu déroulé
    /// (pour ne pas transmettre l'événement à la carte).
    pub fn mouse(&self, x: u32, y: u32) -> bool {
        y <= HEIGHT || self.entry_at(x, y).is_some()
    }

    /// Ferme le menu (par ex. sur la touche Échap).
    pub fn close(&mut self) {
        self.open = None;
        self.hover_entry = None;
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

impl<'ttf> Topbar<'ttf> {
    pub fn new(
        ttf_context: &'ttf Sdl2TtfContext,
        width: u32,
        font_config: &FontConfig,
    ) -> Result<Self, String> {
        let font = ttf_context.load_font(&font_config.file, font_config.ui_size)?;
        Ok(Topbar {
            font,
            width,
            bar: MenuBar::new(),
        })
    }

    /// The window is now `width` wide.
    pub fn resize(&mut self, width: u32) {
        self.width = width;
    }

    pub fn event(&mut self, x: u32, y: u32) -> Option<AppAction> {
        self.bar.event(x, y)
    }

    pub fn motion(&mut self, x: u32, y: u32) -> bool {
        self.bar.motion(x, y)
    }

    pub fn mouse(&self, x: u32, y: u32) -> bool {
        self.bar.mouse(x, y)
    }

    pub fn close(&mut self) {
        self.bar.close()
    }

    pub fn is_open(&self) -> bool {
        self.bar.is_open()
    }

    // ---------- Affichage ----------
    pub fn show(&self, canvas: &mut WindowCanvas) {
        canvas.set_blend_mode(BlendMode::Blend);

        // Barre
        canvas.set_draw_color(COLOR);
        let _ = canvas.fill_rect(Rect::new(0, 0, self.width, HEIGHT));

        for (i, menu) in self.bar.menus.iter().enumerate() {
            let x0 = (i as u32 * ITEM_WIDTH) as i32;
            if self.bar.open == Some(i) || self.bar.hover_title == Some(i) {
                canvas.set_draw_color(HOVER_COLOR);
                let _ = canvas.fill_rect(Rect::new(x0, 0, ITEM_WIDTH, HEIGHT));
            }
            let _ = draw_text(
                canvas,
                &self.font,
                menu.title,
                Rect::new(x0, 0, ITEM_WIDTH, HEIGHT),
                TextAlign::Centered,
                Color::BLACK,
            );
        }

        // Menu déroulant (dessiné par-dessus la carte)
        if let Some(i) = self.bar.open {
            let menu = &self.bar.menus[i];
            let x0 = (i as u32 * ITEM_WIDTH) as i32;
            let h = menu.entries.len() as u32 * ENTRY_HEIGHT;

            canvas.set_draw_color(COLOR);
            let _ = canvas.fill_rect(Rect::new(x0, HEIGHT as i32, MENU_WIDTH, h));

            for (j, entry) in menu.entries.iter().enumerate() {
                let y = (HEIGHT + j as u32 * ENTRY_HEIGHT) as i32;
                if self.bar.hover_entry == Some(j) {
                    canvas.set_draw_color(HOVER_COLOR);
                    let _ = canvas.fill_rect(Rect::new(x0, y, MENU_WIDTH, ENTRY_HEIGHT));
                }
                // texte aligné à gauche avec une marge de 10 px
                let _ = draw_text(
                    canvas,
                    &self.font,
                    &entry.label,
                    Rect::new(x0 + 10, y, MENU_WIDTH - 10, ENTRY_HEIGHT),
                    TextAlign::Left,
                    Color::BLACK,
                );
            }

            canvas.set_draw_color(BORDER_COLOR);
            let _ = canvas.draw_rect(Rect::new(x0, HEIGHT as i32, MENU_WIDTH, h));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> MenuBar {
        MenuBar::new()
    }

    // Titles sit at x = index * ITEM_WIDTH; "Route" is the third menu.
    const ROUTE_X: u32 = 2 * ITEM_WIDTH + 10;

    #[test]
    fn clicking_a_title_opens_its_menu_and_clicking_again_closes_it() {
        let mut bar = bar();
        assert_eq!(bar.event(ROUTE_X, 10), None);
        assert!(bar.is_open());
        assert_eq!(bar.event(ROUTE_X, 10), None);
        assert!(!bar.is_open());
    }

    #[test]
    fn clicking_an_entry_of_the_open_menu_selects_it_and_closes_the_menu() {
        let mut bar = bar();
        bar.event(ROUTE_X, 10);
        let second_entry_y = HEIGHT + ENTRY_HEIGHT + ENTRY_HEIGHT / 2;
        assert_eq!(
            bar.event(ROUTE_X, second_entry_y),
            Some(AppAction::ExportRouteGpx)
        );
        assert!(!bar.is_open());
    }

    #[test]
    fn the_charts_menu_is_the_chart_manager_then_the_lock() {
        let mut bar = bar();
        bar.event(ITEM_WIDTH + 10, 10);
        assert_eq!(
            bar.event(ITEM_WIDTH + 10, HEIGHT + 5),
            Some(AppAction::OpenChartManager)
        );
        bar.event(ITEM_WIDTH + 10, 10);
        assert_eq!(
            bar.event(ITEM_WIDTH + 10, HEIGHT + ENTRY_HEIGHT + 5),
            Some(AppAction::ToggleLock)
        );
        assert_eq!(bar.menus[1].entries.len(), 2);
    }

    #[test]
    fn hovering_reports_a_change_only_when_the_highlight_moves() {
        let mut bar = bar();
        assert!(bar.motion(ROUTE_X, 10), "entering a title highlights it");
        assert!(!bar.motion(ROUTE_X + 5, 12), "still on the same title");
        assert!(
            bar.motion(ROUTE_X, HEIGHT + 100),
            "leaving the bar clears it"
        );
        assert!(
            !bar.motion(ROUTE_X, HEIGHT + 200),
            "nothing highlighted either way"
        );
    }

    #[test]
    fn hovering_another_title_while_open_switches_menus() {
        let mut bar = bar();
        bar.event(ROUTE_X, 10);
        assert!(bar.motion(3 * ITEM_WIDTH + 10, 10));
        let first_entry_y = HEIGHT + ENTRY_HEIGHT / 2;
        assert_eq!(
            bar.event(3 * ITEM_WIDTH + 10, first_entry_y),
            Some(AppAction::ClearTrack)
        );
    }

    #[test]
    fn every_entry_of_every_menu_maps_to_an_action() {
        let mut bar = bar();
        let menus = bar.menus.len();
        for i in 0..menus {
            let x = i as u32 * ITEM_WIDTH + 10;
            let entries = bar.menus[i].entries.len();
            assert!(entries > 0, "menu {i} is empty");
            for j in 0..entries {
                bar.close();
                bar.event(x, 10);
                let y = HEIGHT + j as u32 * ENTRY_HEIGHT + 5;
                assert!(bar.event(x, y).is_some(), "menu {i} entry {j}");
            }
        }
    }

    #[test]
    fn the_bar_claims_clicks_on_titles_and_on_open_entries_only() {
        let mut bar = bar();
        assert!(bar.mouse(ROUTE_X, 10));
        assert!(
            !bar.mouse(ROUTE_X, HEIGHT + 10),
            "closed: the chart gets it"
        );
        bar.event(ROUTE_X, 10);
        assert!(bar.mouse(ROUTE_X, HEIGHT + 10), "open: the entry gets it");
    }
}
