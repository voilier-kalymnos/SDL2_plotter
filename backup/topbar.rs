

use sdl2::{ttf::{Sdl2TtfContext, Font}};

use sdl2::rect::{Rect};
use sdl2::render::{TextureQuery,WindowCanvas};
use sdl2::pixels::Color;
use serde::{Serialize, Deserialize};

const COLOR    : Color = gfx_color(200, 220, 220, 220);
const ITEM_WIDTH :u32 = 120;

pub const SETUP 	:&str = "Charts";
pub const EDIT_ROUTE    :&str = "Edit Route";
pub const CLEAR_ROUTE   :&str = "Clear Route";
pub const FOLLOW   	:&str = "Center Boat";
pub const NAV           :&str = "Navigation";
pub const PREVIOUS      :&str = "Previous Wp";
pub const NEXT          :&str = "Next Wp";
pub const QUITTE	:&str = "Exit";

const FONT_FILE     	:&str = "assets/Montserrat-Regular.ttf";
const FONT_SIZE     	:u16  = 16;
const HEIGHT	    	:u32  = 32;

const ITEMS: [&str; 8] = 
    [
    SETUP,
    EDIT_ROUTE,
    CLEAR_ROUTE,
    FOLLOW,
    NAV,
    PREVIOUS,
    NEXT,
    QUITTE,
    ];

const TOGGLES: [bool; 8] =
    [
    false,
    true,
    false,
    true,
    true,
    false,
    false,
    false
    ];
    
#[derive(Serialize, Deserialize)]
#[derive(Debug)]
struct Item 
	{
        name  : String,
        id    : u8,
        state : bool,
        toggle: bool,
        }
        
pub struct Topbar <'ttf>
	{
	font	: Font<'ttf, 'static>,
	width	: u32,
	items   : Vec<Item>,
	}

impl <'ttf>Topbar <'ttf>
	{
	pub fn new(ttf_context: &'ttf Sdl2TtfContext, width : u32) -> Result<Self, String>
		{
		let font = ttf_context.load_font(FONT_FILE, FONT_SIZE)?;
		
		
		let items: Vec<Item> = ITEMS
		    .iter()
		    .enumerate()
		    .map(|(i, name)| Item {
			name: name.to_string(),
			id : i as u8,
			state : false,
			toggle : TOGGLES[i],
		    })
		    .collect();

		//println!("items {:?}", items);
		Ok(Topbar { font : font, items : items, width : width })
				
		}
	
	pub fn show(&mut self, canvas: &mut WindowCanvas)
		{
		canvas.set_draw_color(COLOR);
		canvas.set_viewport(Rect::new(0, 0, self.width, HEIGHT));
		let r = sdl2::rect::Rect::new(0, 0, self.width, HEIGHT);
		canvas.fill_rect(r).unwrap(); 
		
		let items: Vec<(String, u8, bool)> = self.items
		    .iter()
		    .map(|item| (item.name.clone(), item.id as u8, item.state))
		    .collect();

		for (name, id, state) in items {
			let _ = self.show_item(canvas, &name, id, state);
			}
		canvas.set_viewport(None);
		}
		
	fn show_item(&mut self, canvas: &mut WindowCanvas,  texte: &str,id  :u8, state :bool) -> Result<(), String> 
		{
		let texture_creator = canvas.texture_creator();
		let font = &self.font;
		let color = match state
						{
						false => {sdl2::pixels::Color::BLACK}
						true  => {sdl2::pixels::Color::RED}
						};
		let surface = font
				.render(&texte)
				.blended(color)
				.map_err(|e| e.to_string())?;    
		let texture = texture_creator
		    .create_texture_from_surface(&surface)
		    .map_err(|e| e.to_string())?;

		let TextureQuery { width, height, .. } = texture.query();
		let gap = ITEM_WIDTH as i32 - width as i32;
		let cible = Rect::new((id as i32 *ITEM_WIDTH as i32)+(gap/2), 7, width, height);

		canvas.copy(&texture, None, Some(cible))?;
		unsafe {texture.destroy();}
		Ok(())
		}
		
	pub fn event(&mut self, x :u32, y :u32, ) -> Option<(&str, bool)>
		{
		if y > HEIGHT
			{
			return None
			}
		else	{
			for item in &mut self.items
				{
				let x0 = item.id as u32 * ITEM_WIDTH;
				let x1 = x0 +ITEM_WIDTH;
				if (x0..x1).contains(&x)
					{
					if item.toggle
						{
						item.state = !item.state;
						}
					return Some((&item.name,item.state))
					}
				}
			return None
			}	
		}
	pub fn mouse(&mut self, y :u32, ) -> bool
		{
		if y <= HEIGHT
			{
			return true
			}
		else{
			return false
			}
		}
	}
	
const fn gfx_color(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color::RGBA(a, b, g, r) 
        }
