

use sdl2::{ttf::{Sdl2TtfContext, Font}};

use sdl2::rect::{Rect};
use sdl2::render::{TextureQuery,WindowCanvas};
use sdl2::pixels::Color;

const COLOR    : Color = gfx_color(200, 220, 220, 220);

const FONT_FILE    	:&str = "assets/Montserrat-Regular.ttf";
const FONT_SIZE    	:u16  = 16;
const HEIGHT	    :u32  = 32;



        
pub struct Buttombar <'ttf>
	{
	font	: Font<'ttf, 'static>,
	width	: u32,
	height  : u32,
	}

impl <'ttf>Buttombar <'ttf>
	{
	pub fn new(ttf_context: &'ttf Sdl2TtfContext, width : u32, height :u32) -> Result<Self, String>
		{
		let font = ttf_context.load_font(FONT_FILE, FONT_SIZE)?;
		Ok(Buttombar { font : font, width : width, height : height })
				
		}
	
	pub fn show(&mut self, canvas: &mut WindowCanvas, message1 :&str, message2 :&str)
		{
		canvas.set_draw_color(COLOR);
		canvas.set_viewport(Rect::new(0, (self.height - HEIGHT).try_into().unwrap(), self.width, HEIGHT));
		let r = sdl2::rect::Rect::new(0, 0, self.width, HEIGHT);
		canvas.fill_rect(r).unwrap(); 
		let _ = self.show_message_left(canvas, &message1);
		let _ = self.show_message_right(canvas, &message2);
		canvas.set_viewport(None);
		}



	fn show_message_left(&mut self, canvas: &mut WindowCanvas,  texte: &str) -> Result<(), String> 
		{
		let texture_creator = canvas.texture_creator();
		let font = &self.font;
		let color = sdl2::pixels::Color::BLACK;
		let surface = font
				.render(&texte)
				.blended(color)
				.map_err(|e| e.to_string())?;    
		let texture = texture_creator
		    .create_texture_from_surface(&surface)
		    .map_err(|e| e.to_string())?;

		let TextureQuery { width, height, .. } = texture.query();
		let cible = Rect::new(20, 7, width, height);

		canvas.copy(&texture, None, Some(cible))?;

		unsafe {texture.destroy();}
		Ok(())
		}
	

	fn show_message_right(&mut self, canvas: &mut WindowCanvas,  texte: &str) -> Result<(), String> 
		{
		let texture_creator = canvas.texture_creator();
		let font = &self.font;
		let color = sdl2::pixels::Color::BLACK;
		let surface = font
				.render(&texte)
				.blended(color)
				.map_err(|e| e.to_string())?;    
		let texture = texture_creator
		    .create_texture_from_surface(&surface)
		    .map_err(|e| e.to_string())?;

		let TextureQuery { width, height, .. } = texture.query();
		let cible = Rect::new(self.width as i32 - width as i32 - 20, 7, width, height);

		canvas.copy(&texture, None, Some(cible))?;

		unsafe {texture.destroy();}
		Ok(())
		}
	}	

const fn gfx_color(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color::RGBA(a, b, g, r) 
        }
