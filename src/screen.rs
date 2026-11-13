
use sdl2::{Sdl, ttf::{Sdl2TtfContext, Font}};
use sdl2::render::{Texture,TextureQuery,WindowCanvas};
use sdl2::render::BlendMode;
use sdl2::gfx::primitives::DrawRenderer; 
use sdl2::EventPump;
use sdl2::pixels::Color;
use sdl2::rect::{Rect};
use sdl2::rect::Point;
use sdl2::mouse::{Cursor, SystemCursor};

const FULL_SCREEN   :bool = false;
const FONT_FILE     :&str = "assets/Montserrat-Regular.ttf";
const FONT_SIZE     :u16  = 28;
const WINDOW_WIDTH  :u32  = 1600;
const WINDOW_HEIGHT :u32  = 800;
const TITLE         :&str = "KALYMNOS";
const BOAT_SIZE     :i16  = 6;
const MARK_SIZE     :i16  = 6;
const BOAT_CIRCLE   :i16  = 50;
const ROUTE_WIDTH   :u8   = 3;
const COG_SIZE      :f64  = 250.0;
const COG_THICK     :u8   = 4; 

const BOAT_COLOR     : Color = gfx_color(0, 0, 255, 255);
const MESURE_COLOR   : Color = gfx_color(0, 0, 255, 255);
const ROUTE_COLOR    : Color = gfx_color(255, 0, 0, 255);
const COG_COLOR      : Color = gfx_color(0, 0, 255, 255);
const AIS_COLOR      : Color = Color::RGB(255, 140, 0);
const BACKGROUND_MAP : Color = gfx_color(250, 250, 250, 255);

pub struct MoteurSdl {
    pub context     : Sdl,
    pub event_pump  : EventPump,
    pub ttf_context : Sdl2TtfContext,
    }

impl MoteurSdl {
    pub fn init() -> Result<Self, String> {
        let context = sdl2::init()?;
        let event_pump = context.event_pump()?;
        let ttf_context = sdl2::ttf::init().map_err(|e| e.to_string())?;

        Ok(MoteurSdl { context, event_pump, ttf_context:ttf_context })
        }
    }

pub struct Map<'ttf> {
    font                : Font<'ttf, 'static>,
    pub canvas          : WindowCanvas,
    pub screen_width    : u32,
    pub screen_height   : u32,
    pub cursor_hand     : Cursor,
    pub cursor_cross    : Cursor,
    pub cursor_arrow    : Cursor,
    pub cursor_wait     : Cursor,
    pub cursor_size     : Cursor,
    }

impl<'ttf> Map<'ttf> {
    pub fn new(ttf_context: &'ttf Sdl2TtfContext, context: &Sdl ) -> Result<Self, String> {

        let font = ttf_context.load_font(FONT_FILE, FONT_SIZE)?;
        
        let video_subsystem = context.video().unwrap();
        let window = match FULL_SCREEN 
            {
            false => video_subsystem
                .window(TITLE, WINDOW_WIDTH, WINDOW_HEIGHT)
                .position_centered()
                .build()
                .unwrap(),
            true => video_subsystem
                .window(TITLE, WINDOW_WIDTH, WINDOW_HEIGHT)
                .position_centered()
                .fullscreen_desktop()
                .build()
                .unwrap(),
            };
        
        let display_mode     = video_subsystem.current_display_mode(0).unwrap();
        let (screen_width,screen_height) = match FULL_SCREEN
            {
            false => { (WINDOW_WIDTH,WINDOW_HEIGHT) }
            true  => { (display_mode.w as u32,display_mode.h as u32) }
            };
            
        let canvas        = window.into_canvas().accelerated().build().unwrap();
        let cursor_hand   = Cursor::from_system(SystemCursor::Hand)?;
        let cursor_cross  = Cursor::from_system(SystemCursor::Crosshair)?;
        let cursor_arrow  = Cursor::from_system(SystemCursor::Arrow)?;
        let cursor_wait   = Cursor::from_system(SystemCursor::Wait)?;
        let cursor_size   = Cursor::from_system(SystemCursor::SizeAll)?;
        cursor_hand.set();
        Ok(Map { font, canvas, screen_width, screen_height, cursor_hand, cursor_cross, cursor_arrow, cursor_wait, cursor_size })
        }

    pub fn draw_mid_handles(&mut self, pts: &[Point])  {
        self.canvas.set_draw_color(ROUTE_COLOR);
        for p in pts 
            {
            let r = sdl2::rect::Rect::new(p.x - 4, p.y - 4, 12, 12);
            self.canvas.draw_rect(r).unwrap();   // contour seulement, pour les distinguer des vrais points
            }
        }

    pub fn show_cog(&mut self, mut radians :f64, pos : Point) -> Result<(), String> {

        radians -= std::f64::consts::PI / 2.0;
        let x1 = pos.x as f64 + ((radians.cos() * COG_SIZE));
        let y1 = pos.y as f64 + ((radians.sin() * COG_SIZE));

        let _  = self.canvas.thick_line(
                    pos.x as i16, 
                    pos.y as i16, 
                    x1 as i16, 
                    y1 as i16, 
                    COG_THICK, // Épaisseur de 4 pixels
                    COG_COLOR, 
                    );
        Ok(())
        }
    
    pub fn show_route(&mut self, pts: &[Point])  {
        for seg in pts.windows(2) 
            {
            let (a, b) = (&seg[0], &seg[1]);
            let _ = self.canvas.thick_line(
                a.x as i16, a.y as i16,
                b.x as i16, b.y as i16,
                ROUTE_WIDTH,
                ROUTE_COLOR,
            );
            }
        self.canvas.set_draw_color(ROUTE_COLOR);
        for pt in pts
            {
            let _ = self.canvas.filled_circle( pt.x as i16, pt.y as i16, BOAT_SIZE, ROUTE_COLOR);
            }
        }

    pub fn show_map(&mut self, texture:&Texture) -> Result<(), String> {
        self.canvas.copy(texture, None, None)?;
        Ok(())
        }
        
    pub fn drag_map(&mut self, texture:&Texture,x:i32,y:i32) -> Result<(), String> {
        let dst_rect = Rect::new(
            x,
            y,
            self.screen_width,
            self.screen_height,
        );
        self.canvas.set_draw_color(BACKGROUND_MAP);
        self.canvas.clear();
        self.canvas.copy(texture, None, Some(dst_rect))?;
        self.canvas.present();
        Ok(())
        }
               
    pub fn show_boat(&mut self,x:i16,y:i16) -> Result<(), String> {
        
        self.canvas.filled_circle( x, y, BOAT_SIZE, BOAT_COLOR)?;
        self.canvas.circle( x, y, BOAT_CIRCLE, BOAT_COLOR)?;
        Ok(())
        }

    pub fn show_ais_target(
        &mut self,
        x: i32,
        y: i32,
        course_over_ground: Option<f64>,
    ) -> Result<(), String> {
        if let Some(course) = course_over_ground.filter(|course| course.is_finite()) {
            let end_x = x + (course.sin() * 28.0).round() as i32;
            let end_y = y - (course.cos() * 28.0).round() as i32;
            let _ = self.canvas.thick_line(
                x as i16,
                y as i16,
                end_x as i16,
                end_y as i16,
                3,
                AIS_COLOR,
            );
        }
        self.canvas.filled_circle(x as i16, y as i16, 7, AIS_COLOR)?;
        self.canvas.circle(x as i16, y as i16, 9, Color::BLACK)?;
        Ok(())
    }

    pub fn show_mark(&mut self,x:i16,y:i16) -> Result<(), String> {

        self.canvas.filled_circle( x, y, MARK_SIZE, MESURE_COLOR)?;
        Ok(())
        }
    
    pub fn show_line(&mut self, x1: i16, y1: i16, x2: i16, y2: i16) -> Result<(), String>{
        self.canvas.set_draw_color(Color::RGB(0, 0, 255));
        self.canvas.draw_line(Point::new(x1 as i32, y1 as i32), Point::new(x2 as i32, y2 as i32))
        }
        
    pub fn night_layer( &mut self, opacity : u8 ) -> Result<(), String> {
        self.canvas.set_blend_mode(BlendMode::Blend);
        self.canvas.set_draw_color(Color::RGBA(255, 0, 0, opacity));

        let ecran_total = Rect::new(0, 0, self.screen_width, self.screen_height);
        self.canvas.fill_rect(ecran_total).unwrap();
        self.canvas.present();
        Ok(())
        }
    
    #[allow(dead_code)]
    pub fn info(&mut self, texte: &str) -> Result<(), String> {
         println!("{}",&texte);
        self.canvas.set_draw_color(Color::RGB(0,0,0));
        self.canvas.clear();
        let texture_creator = self.canvas.texture_creator();
        let font = &self.font;

        let surface = font
                        .render(texte)
                        .blended_wrapped(sdl2::pixels::Color::WHITE, 700)
                        .map_err(|e| e.to_string())?;    
        let texture = texture_creator
            .create_texture_from_surface(&surface)
            .map_err(|e| e.to_string())?;

        let TextureQuery { width, height, .. } = texture.query();
        
        let  x:i32 =((self.screen_width as i32)-(width as i32))/2;
        let  y:i32 =((self.screen_height as i32)-(height as i32))/2;
        let cible = Rect::new(x, y, width, height);
        self.canvas.copy(&texture, None, Some(cible))?;
        unsafe {texture.destroy();}
        self.canvas.present();
        Ok(())
        }
    
    pub fn show_text( &mut self, texte: &str,x :i32, y:i32 ) -> Result<(), String> {

        let texture_creator = self.canvas.texture_creator();
        let font = &self.font;

        let surface = font
                        .render(texte)
                        .blended(sdl2::pixels::Color::BLACK)
                        .map_err(|e| e.to_string())?;    
        let texture = texture_creator
            .create_texture_from_surface(&surface)
            .map_err(|e| e.to_string())?;

        let TextureQuery { width, height, .. } = texture.query();

        //let  y:i32 = (self.screen_height - 100).try_into().unwrap();
        //let  x:i32 = ((self.screen_width/2) - 200).try_into().unwrap();
        let cible = Rect::new(x, y, width, height);
        
        self.canvas.set_blend_mode(BlendMode::Blend);
        self.canvas.set_draw_color(Color::RGBA(255, 255, 255, 128));
        self.canvas.fill_rect(cible).unwrap();
        
        self.canvas.copy(&texture, None, Some(cible))?;
        unsafe {texture.destroy();}
        Ok(())
        }
    
    }
    
    
const fn gfx_color(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color::RGBA(a, b, g, r) 
        }
    
