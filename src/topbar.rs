use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::{BlendMode, WindowCanvas};
use sdl2::ttf::{Font, Sdl2TtfContext};


const COLOR: Color = Color::RGBA(220, 220, 220, 200);
const HOVER_COLOR: Color = Color::RGB(170, 195, 230);
const BORDER_COLOR: Color = Color::RGB(120, 120, 120);

const ITEM_WIDTH: u32   = 80; // largeur d'un titre dans la barre
const MENU_WIDTH: u32   = 170; // largeur d'un menu déroulant
const ENTRY_HEIGHT: u32 = 28; // hauteur d'une entrée du menu
const HEIGHT: u32       = 32; // hauteur de la barre

pub const EDIT_ROUTE: &str  = "Edit Route";
pub const EDIT_WP: &str     = "Edit Waypoint";
pub const EDIT_MESURE: &str = "Measurement";
pub const CLEAR_ROUTE: &str = "Clear Route";
pub const FOLLOW: &str      = "Follow Boat";
pub const GOTO: &str        = "From Boat";
pub const PREVIOUS: &str    = "Previous Wp";
pub const NEXT: &str        = "Next Wp";
pub const QUITTE: &str      = "Exit";
pub const CLEAR_TRACK: &str = "Clear Track";
pub const SAVE_TRACK: &str  = "Save Track";
pub const CLEAR_WP: &str    = "Clear Waypoint";
pub const SAVE_WP: &str     = "Save Waypoint";
pub const SHOW_WP: &str     = "Show Waypoint";
pub const HIDE_WP: &str     = "Hide Waypoint";
pub const SHOW_TRACK: &str  = "Show Track";
pub const HIDE_TRACK: &str  = "Hide Track";
pub const SHOW_ROUTE: &str  = "Show Route";
pub const HIDE_ROUTE: &str  = "Hide Route";
pub const SHOW_AIS: &str    = "Show Ais";
pub const HIDE_AIS: &str    = "Hide Ais";

const FONT_FILE: &str = "assets/Montserrat-Regular.ttf";
const FONT_SIZE: u16 = 16;


struct Menu {
    title: &'static str,
    entries: Vec<String>,
}

pub struct Topbar<'ttf> {
    font: Font<'ttf, 'static>,
    width: u32,
    menus: Vec<Menu>,
    open: Option<usize>,        // menu actuellement déroulé
    hover_title: Option<usize>, // titre survolé
    hover_entry: Option<usize>, // entrée survolée (dans le menu ouvert)
}

impl<'ttf> Topbar<'ttf> {
    pub fn new(ttf_context: &'ttf Sdl2TtfContext, width: u32, names :Vec<String>) -> Result<Self, String> {
        let font = ttf_context.load_font(FONT_FILE, FONT_SIZE)?;


        println!("names : {:?}", names);
        
        let menus = vec![
            Menu { title: "App", entries: vec![
                    QUITTE.into(),
                    ] },
            Menu { title: "Charts", entries: names.clone()
                     },
            Menu {
                title: "Mode",
                entries: vec![
                    EDIT_MESURE.into(),
                    EDIT_ROUTE.into(),
                    EDIT_WP.into(),
                    FOLLOW.into(),
                    ] },
            Menu {
                title: "Route",
                entries: vec![
                    CLEAR_ROUTE.into(),
                    GOTO.into(),
                    PREVIOUS.into(),
                    NEXT.into(),
                    SHOW_ROUTE.into(),
                    HIDE_ROUTE.into(),
                    ] },
            Menu {
                title: "Track",
                entries: vec![
                    CLEAR_TRACK.into(),
                    SAVE_TRACK.into(),
                    SHOW_TRACK.into(),
                    HIDE_TRACK.into(),
                    ] },
            Menu {
                title: "Waypoints",
                entries: vec![
                    CLEAR_WP.into(),
                    SAVE_WP.into(),
                    SHOW_WP.into(),
                    HIDE_WP.into(),
                    ] },
            Menu {
                title: "AIS",
                entries: vec![
                    SHOW_AIS.into(),
                    HIDE_AIS.into(),
                    ] },
            ];

        Ok(Topbar { font, width, menus, open: None, hover_title: None, hover_entry: None })
    }


     // ---------- Affichage ----------
    pub fn show(&self, canvas: &mut WindowCanvas) {
        canvas.set_blend_mode(BlendMode::Blend);

        // Barre
        canvas.set_draw_color(COLOR);
        let _ = canvas.fill_rect(Rect::new(0, 0, self.width, HEIGHT));

        for (i, menu) in self.menus.iter().enumerate() {
            let x0 = (i as u32 * ITEM_WIDTH) as i32;
            if self.open == Some(i) || self.hover_title == Some(i) {
                canvas.set_draw_color(HOVER_COLOR);
                let _ = canvas.fill_rect(Rect::new(x0, 0, ITEM_WIDTH, HEIGHT));
            }
            let _ = self.draw_text(canvas, menu.title, x0, 0, ITEM_WIDTH, HEIGHT, Color::BLACK);
        }

        // Menu déroulant (dessiné par-dessus la carte)
        if let Some(i) = self.open {
            let menu = &self.menus[i];
            let x0 = (i as u32 * ITEM_WIDTH) as i32;
            let h = menu.entries.len() as u32 * ENTRY_HEIGHT;

            canvas.set_draw_color(COLOR);
            let _ = canvas.fill_rect(Rect::new(x0, HEIGHT as i32, MENU_WIDTH, h));

            for (j, e) in menu.entries.iter().enumerate() {
                let y = (HEIGHT + j as u32 * ENTRY_HEIGHT) as i32;
                if self.hover_entry == Some(j) {
                    canvas.set_draw_color(HOVER_COLOR);
                    let _ = canvas.fill_rect(Rect::new(x0, y, MENU_WIDTH, ENTRY_HEIGHT));
                }
                //let color = if e.state { Color::RED } else { Color::BLACK };
                // texte aligné à gauche avec une marge de 10 px
                let _ = self.draw_text(canvas, e, x0 + 10, y, 0, ENTRY_HEIGHT, Color::BLACK);
            }

            canvas.set_draw_color(BORDER_COLOR);
            let _ = canvas.draw_rect(Rect::new(x0, HEIGHT as i32, MENU_WIDTH, h));
        }
    }

    /// Dessine `texte` dans la zone (x, y, w, h).
    /// Si `w > 0` le texte est centré horizontalement, sinon aligné à gauche en x.
    /// Dans les deux cas il est centré verticalement.
    fn draw_text(
        &self,
        canvas: &mut WindowCanvas,
        texte: &str,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        color: Color,
    ) -> Result<(), String> {
        let texture_creator = canvas.texture_creator();
        let surface = self
            .font
            .render(texte)
            .blended(color)
            .map_err(|e| e.to_string())?;
        let texture = texture_creator
            .create_texture_from_surface(&surface)
            .map_err(|e| e.to_string())?;

        let (tw, th) = (surface.width(), surface.height());
        let tx = if w > 0 { x + (w as i32 - tw as i32) / 2 } else { x };
        let ty = y + (h as i32 - th as i32) / 2;

        canvas.copy(&texture, None, Some(Rect::new(tx, ty, tw, th)))?;
        unsafe { texture.destroy(); }
        Ok(())
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
    /// Retourne Some((nom, état)) quand une entrée est choisie.
    pub fn event(&mut self, x: u32, y: u32) -> Option<String> {
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
            let e = &mut self.menus[i].entries[j];
            self.open = None;
            self.hover_entry = None;
            return Some(e.to_string());
        }

        // Clic ailleurs : on ferme le menu
        self.open = None;
        self.hover_entry = None;
        None
    }

    /// À appeler sur MouseMotion : gère le survol.
    pub fn motion(&mut self, x: u32, y: u32) {
        self.hover_title = if y < HEIGHT { self.menu_at(x) } else { None };

        // Quand un menu est ouvert, survoler un autre titre l'ouvre (comme un vrai menu)
        if self.open.is_some() {
            if let Some(i) = self.hover_title {
                self.open = Some(i);
            }
        }
        self.hover_entry = self.entry_at(x, y);
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

 
