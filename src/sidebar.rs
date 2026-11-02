use sdl2::pixels::Color;
use sdl2::rect::Rect;
use sdl2::render::{BlendMode, WindowCanvas};
use sdl2::ttf::{Font, Sdl2TtfContext};

use crate::State;

const COLOR: Color = Color::RGBA(220, 220, 220, 200);
const WIDTH :u32 = 150;
const HEIGHT:u32 = 40;

const FONT_FILE: &str = "assets/Montserrat-Regular.ttf";
const FONT_SIZE: u16 = 16;

const EDIT_ROUTE :&str	= "Edit Route";
const EDIT_WP    :&str	= "Edit Waypoint";
const MESURE     :&str	= "Mesure";
const FOLLOW     :&str	= "Follow";

pub struct Sidebar<'ttf> {
    font: Font<'ttf, 'static>,
    x0: i32,
}

impl<'ttf> Sidebar<'ttf> {
    pub fn new(ttf_context: &'ttf Sdl2TtfContext, width: u32) -> Result<Self, String> {
        let font = ttf_context.load_font(FONT_FILE, FONT_SIZE)?;
		let x0 = (width - WIDTH).try_into().unwrap();
        Ok(Sidebar { font, x0 })
		}
		
	pub fn show(&self, canvas: &mut WindowCanvas, state : &State)
		{
		canvas.set_blend_mode(BlendMode::Blend);
		//let x0 = (self.width - WIDTH).try_into().unwrap();
        // Barre
        canvas.set_draw_color(COLOR);
        let _ = canvas.fill_rect(Rect::new(self.x0 , 0, WIDTH, 4*HEIGHT));
        
        let mut i :usize = 0;

        let str = if state.edit_route 	{ EDIT_ROUTE }
		else if state.edit_wp   { EDIT_WP }
		else if state.mesure    { MESURE }
		else { "None" };
				
        let _ = self.draw_text(canvas, str, self.x0, i as i32 *HEIGHT as i32, WIDTH, HEIGHT, Color::BLACK);
        i +=1;
        
        let color = if state.follow { Color::RED } else { Color::BLACK };
	let _ = self.draw_text(canvas, FOLLOW, self.x0, i as i32 *HEIGHT as i32, WIDTH, HEIGHT, color);
	i +=1;
	
	if state.track
		{
		let _ = self.draw_text(canvas, "Track record", self.x0, i as i32 *HEIGHT as i32, WIDTH, HEIGHT, Color::BLACK);
		}
	}
	
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
	}
