
// Install SDL2 driver
// sudo apt update && sudo apt install -y libsdl2-2.0-0
// sudo apt install -y libsdl2-ttf-2.0-0 libsdl2-image-2.0-0 libsdl2-mixer-2.0-0
// sudo apt install libdrm-dev libgbm-dev libegl1-mesa-dev
// pkill -9 plotter
// sudo apt install libsdl2-gfx-1.0-0

mod mbtiles_renderer;
mod screen;
mod state;
mod signalk;
mod boat;
mod ais;
mod topbar;
mod buttombar;
mod nav;
mod route;
mod mesure;
mod gps;

use crate::boat::BoatState;
use mbtiles_renderer::MBTilesRenderer;
use sdl2::event::Event;
use std::time::{Duration, Instant};
use sdl2::pixels::PixelFormatEnum;
use sdl2::ttf::Sdl2TtfContext;
use sdl2::keyboard::Mod;
use tokio::sync::watch;
use std:: path::Path;
use chrono::Local;

const MAP_WORLD	:&str			= "assets/OSM.mbtiles";
const STATE_FILE :&str  		= "state.json"; 
const DEFAULT_TRACK_DIR: &str	= "track/";
const DEFAULT_TRACK_PATH: &str	= "track/default.gpx";
const LONG_CLICK: Duration 		= Duration::from_millis(500);
const MOVE_TOLERANCE: i32 		= 5;

#[tokio::main]
async fn main() -> anyhow::Result<()> {

	let mut sdl = screen::MoteurSdl::init().map_err(anyhow::Error::msg)?;
	let mut map = screen::Map::new(&sdl.ttf_context, &sdl.context).map_err(anyhow::Error::msg)?;

	let texture_creator = map.canvas.texture_creator();
	let mut texture = texture_creator
		.create_texture_streaming(PixelFormatEnum::RGBA32, map.screen_width, map.screen_height)
		.map_err(anyhow::Error::msg)?;
	let pitch = (map.screen_width * 4) as usize;

	let mut plotter = Plotter::new(&sdl.ttf_context, map.screen_width, map.screen_height)?;
	plotter.main_loop(&mut sdl.event_pump, &mut map, &mut texture, pitch)
	}

struct Press {
    start: Instant,
    x: i32,
    y: i32,
    moved: bool,
    long_fired: bool,
}

struct Plotter<'ttf> {
	renderer: MBTilesRenderer,
	state: state::State,
	running: bool,
	dirty: bool,
	is_draging: bool,
	xdrag:i32,
	ydrag:i32,
	drag_start: (i32,i32),
	boat: BoatState,
	topbar: topbar::Topbar<'ttf>,
	buttombar: buttombar::Buttombar<'ttf>,
	nav:nav::Nav,
	route:route::Route,
	mesure:mesure::Mesure,
	charts_path: Vec<String>,
	charts_name: Vec<String>,
	press: Option<Press>,
	selected: Option<usize>,
	state_rx: watch::Receiver<BoatState>,
    track_rx: tokio::sync::mpsc::UnboundedReceiver<(f64, f64)>,
    ais_rx: watch::Receiver<Vec<ais::AisTarget>>,
    status_rx: watch::Receiver<String>,
}

impl<'ttf> Plotter<'ttf> {

	fn new(ttf_context: &'ttf Sdl2TtfContext, width: u32, height: u32) -> anyhow::Result<Self> {
		let boat = BoatState::default();
		let (state_tx, state_rx) = watch::channel(boat);
		let (track_tx, track_rx) = tokio::sync::mpsc::unbounded_channel();
		let (ais_tx, ais_rx) = watch::channel(Vec::new());
		let (status_tx, status_rx) = watch::channel(String::new());
		tokio::spawn(signalk::signalk_task(state_tx, track_tx, ais_tx, status_tx,));

		let charts_path = state::list_maps("map");
		let charts_name: Vec<String> = charts_path
			.iter()
			.map(|path| {
				std::path::Path::new(path)
					.file_stem()
					.and_then(|name| name.to_str())
					.unwrap_or(path)
					.to_string()
			})
			.collect();
		let initial_map = charts_path
			.first()
			.cloned()
			.unwrap_or_else(|| MAP_WORLD.to_string());
		let state = state::State::new(STATE_FILE, initial_map)
			.map_err(anyhow::Error::msg)?;
		let renderer = MBTilesRenderer::new(
			&state.map.clone(), 
			state.zoom, 
			width, 
			height)?;
		
		let topbar = topbar::Topbar::new(ttf_context, width, charts_name.clone())
			.map_err(anyhow::Error::msg)?;
		let buttombar 	= buttombar::Buttombar::new(ttf_context, width, height)
			.expect("failed to build buttombar bar");
		let nav 		= nav::Nav::new();
		let route  	= route::Route::new().map_err(anyhow::Error::msg)?;
		let mesure 	= mesure::Mesure::new().map_err(anyhow::Error::msg)?;

		Ok(Self { 	renderer, 
					state, running: true, 
					dirty: true, 
					is_draging: false, 
					xdrag: 0,
					ydrag: 0,
					drag_start: (0,0),
					boat, 
					topbar, 
					buttombar,
					nav,
					route,
					mesure,
					charts_path, 
					charts_name, 
					press: None, 
					selected: None,
					state_rx, 
					track_rx, 
					ais_rx, 
					status_rx })
	}

	fn main_loop(
			&mut self,
			event_pump: &mut sdl2::EventPump,
			map: &mut screen::Map<'_>,
			texture: &mut sdl2::render::Texture,
			pitch: usize,
		) -> anyhow::Result<()> {
		while self.running {

			/////////////////////   Event /////////////////////////
			for event in event_pump.poll_iter() {
				self.match_event(event, map, texture)
					.map_err(anyhow::Error::msg)?;
				}

			//////////////////////   Position  /////////////////////////
			if self.state_rx.has_changed()? {
				let boat = *self.state_rx.borrow_and_update();
				self.boat = boat;

				if self.state.follow {
					self.state.position = Some((boat.longitude, boat.latitude));
					}
    			self.dirty = true;
				}
			
			self.long_click();

			//////////////////////   Display /////////////////////////
			if self.dirty && !self.is_draging {
				self.display(map, texture, pitch).map_err(anyhow::Error::msg)?;
				self.dirty = false;
				}
			
			/////// Sleep to limit CPU usage /////////////////////////
			std::thread::sleep(Duration::from_millis(16));
			} // end while self.running

			//match self.track.save(Path::new(DEFAULT_TRACK_PATH)) {
			//			Ok(()) => 	{
			//						println!("Trace sauvegardée");
			//						},
			//			Err(e) => eprintln!("Erreur sauvegarde : {e}"),
			//			}
			let _ = self.state.save(STATE_FILE);
			Ok(())
			}

	fn display(&mut self, map: &mut screen::Map<'_>, texture: &mut sdl2::render::Texture, pitch: usize) -> anyhow::Result<()> {
		
		////////////////// Map display ///////////////////////
		if self.state.position == None{
			self.state.position = Some(self.renderer.tiles_center().ok_or(anyhow::anyhow!("no tiles center!!!"))?);
			}
		let (lon, lat) = self.state.position.ok_or(anyhow::anyhow!("position de la carte inconnue"))?;
		let pixels = self.renderer.get_raw_rgba(lon, lat, (0, 0));
		texture
			.update(None, &pixels, pitch)
			.map_err(|error| anyhow::anyhow!(error))?;
		map.show_map(texture).map_err(anyhow::Error::msg)?;

		//////////////// Mesure display   ////////////  
	    if self.state.mesure{
	        self.mesure.show(&self.renderer, map);
	    	}
		
		////////////////// Route display /////////////////
		self.display_route(map);

		////////// Affichage du bateau  ////////////////////////////////////
	    self.boat.show( &self.renderer, map);

		/////////////// Topbar display ///////////////////////
		self.topbar.show(&mut map.canvas);

		/////////////// Buttombar display ////////////////////
		self.display_buttombar(map);

		map.night_layer(0).map_err(anyhow::Error::msg)?;

		/////////////// preload tiles on the edge ////////////////////////////É	²
		self.renderer.prefetch_step(8);
		Ok(())
		}

	fn display_buttombar(&mut self, map: &mut screen::Map<'_>){
		
		let message_left = 
			if self.state.goto{
				let (dist_wp, bearing_wp, dist_total) = self.route.calc_route_mn_from_boat(self.nav.get_id(), self.boat.position());
				format!("From Boat to Next Waypoint :  {:.1} Mn   {:.0}° / DTD : {:.1} Mn",dist_wp, bearing_wp, dist_total)
				} 
			else 
				{
				self.route.show_route_len()
				};
		
		let mut message_right =
			if self.state.edit_route{
				"Mode Edit Route".to_string()
				}
			else if self.state.edit_wp{
				"Mode Edit Waypoint".to_string()
				}
			else if self.state.mesure{
				"Mode Mesure".to_string()
				}
			else{
				"No Mode".to_string()
				};

		message_right += 
			if self.state.follow{
				" / Follow"
				}
			else{
				""
				};
	    self.buttombar.show(&mut map.canvas,&message_left, &message_right);
		}

	fn display_route(&mut self,  map: &mut screen::Map<'_>){
 		if self.state.goto{
			self.route.show_route_from_boat(&self.renderer, map, self.state.edit_route,self.nav.get_id(), self.boat.position());
			}
	    else{
			self.route.show_route_since_id(&self.renderer, map, self.state.edit_route, 0);
			}
		}

	fn handle_move_map(
		&mut self,
		x: i32,
		y: i32,
		xrel: i32,
		yrel: i32,
		map: &mut screen::Map<'_>,
		texture: &sdl2::render::Texture,
	) -> Result<(), String> {
		if let Some(p) = self.press.as_mut(){ 
			if !p.moved {
				let (dx, dy) = (x - p.x, y - p.y);
				if dx * dx + dy * dy > MOVE_TOLERANCE * MOVE_TOLERANCE {   // demarrage du drag
					println!("Start drag map");
					p.moved = true;
					self.is_draging = true;
					self.drag_start = (x, y);
					self.xdrag = 0;
					self.ydrag = 0;
					//map.cursor_hand.set();
					}
				}
			if p.moved && self.is_draging {
				//println!("Drag map");
				self.xdrag += xrel;
				self.ydrag += yrel;
				map.drag_map(texture, self.xdrag, self.ydrag)?;
				}
			}
		Ok(())
		}

	fn handle_click_mesure(&mut self,map: &mut screen::Map<'_>,x :i32, y: i32){

			println!("Mesure click");
			let dx = map.screen_width as i32 / 2 - x;
			let dy = map.screen_height as i32 / 2 - y;
			self.mesure.add_mark(&self.renderer,dx,dy);
			self.dirty = true;
			}

	fn handle_click_edit_route(&mut self,map: &mut screen::Map<'_>,x :i32, y: i32){

			println!("Edit route click");
			self.selected = self.route.find_point_at(&self.renderer, (x, y), 8);
			println!("selected {:?}",self.selected);
			if self.selected == None
				{
				if let Some(k) = self.route.find_midpoint_at(&self.renderer, (x, y), 8) // nouveau point entre k et k+1
					{
					let (longitude, latitude) = self.route.midpoints()[k];
					self.route.insert(k, longitude, latitude);     
					println!("Ajoute un point entre deux wp");
					}
				else 	
					{                      // nouveau point en fin de route
					let dx = map.screen_width as i32 / 2 - x;
					let dy = map.screen_height as i32 / 2 - y;
					let (longitude,latitude) = self.renderer.screen_to_gps(dx, dy);
					self.route.add(longitude,latitude);
					println!("Ajoute un point à la fin");
					//println!("route {route:?}");
					}
				}
		self.route.save();
		self.dirty = true;
			}

	fn handle_click_edit_wp(&mut self,map: &mut screen::Map<'_>,x :i32, y: i32){
			println!("Edit waypoint click");

			}

	fn handle_short_click(&mut self, map: &mut screen::Map<'_>, x :i32, y: i32){
		//println!("Click court en ({}, {})", x, y);
		if self.state.mesure{
			self.handle_click_mesure(map, x,y);
			}
		else if self.state.edit_route{
			self.handle_click_edit_route(map, x,y);
			}
		else if self.state.edit_wp{
			self.handle_click_edit_wp(map, x, y);
			}
		}

	fn handle_long_click_edit_route(&mut self, x: i32, y: i32){

		println!("Delete Point");
		if let Some(i) = self.route.find_point_at(&self.renderer, (x, y), 8) 
			{
			self.route.remove(i);
			self.route.save();
			self.dirty = true;
			}
		}

	fn handle_long_click(&mut self, x :i32, y: i32){
		println!("Click long en ({}, {})", x, y);
		if self.state.edit_route{
			self.handle_long_click_edit_route(x, y);
			}
		}

	fn long_click(&mut self) {
		let click_position = if let Some(p) = self.press.as_mut() {
			if !p.moved && !p.long_fired && p.start.elapsed() >= LONG_CLICK {
				p.long_fired = true;
				Some((p.x, p.y))
			} else {
				None
			}
		} else {
			None
		};

		if let Some((x, y)) = click_position {
			self.handle_long_click(x, y);
		}
		}

	fn handle_keyboard_event(&mut self, event: &Event, map: &mut screen::Map<'_>) {
		match event {
			Event::KeyDown { keycode, keymod, .. } => match keycode {

				Some(sdl2::keyboard::Keycode::Escape) => {
					self.running = false;
					}

				Some(sdl2::keyboard::Keycode::Up) => {
					println!("Move map Up"); 
					self.state.position = Some(self.renderer.screen_to_gps(
						0,
						if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD){1000} else {100},
						));
					self.dirty = true;
					}

				Some(sdl2::keyboard::Keycode::Down) => {
					println!("Move map Down"); 
					self.state.position = Some(self.renderer.screen_to_gps(
						0,
						if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD){-1000} else {-100},
						));
					self.dirty = true;
					}

				Some(sdl2::keyboard::Keycode::Left) => {
					println!("Move map left"); 
					self.state.position = Some(self.renderer.screen_to_gps(
						if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD){1000} else {100},
						0,
						));
					self.dirty = true;
					}

				Some(sdl2::keyboard::Keycode::Right) => {
					println!("Move map Right"); 
					self.state.position = Some(self.renderer.screen_to_gps(
						if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD){-1000} else {-100},
						0,
						));
					self.dirty = true;
					}

				Some(sdl2::keyboard::Keycode::KpPlus) => {
					self.state.zoom = self.renderer.zoom_increase(&self.state.map);
					//is_zooming = true;
					self.state.map_offset = (0,0);
					self.dirty = true;
					}

				Some(sdl2::keyboard::Keycode::KpMinus) => {
					self.state.zoom = self.renderer.zoom_decrease();
					//is_zooming = true;
					self.state.map_offset = (0,0);
					self.dirty = true;
					}
				Some(sdl2::keyboard::Keycode::F) => {
					self.state.follow = !self.state.follow;
					self.dirty = true;
					}
				_ => {}
			},
			_ => {}
			}
		}

	fn handle_topbar_event(&mut self, x: u32, y: u32) {
			println!("Topbar event");
			match self.topbar.event(x, y).as_deref()
			    {
			    None => {}   // simple ouverture / fermeture d'un topbar
				
				Some(topbar::EDIT_ROUTE) =>
					{
					self.state.edit_route 	= true;
					self.state.edit_wp  	= false;
					self.state.mesure  		= false;
					self.dirty 				= true;
					}
				Some(topbar::EDIT_WP) =>
					{
					self.state.edit_route 	= false;
					self.state.edit_wp  	= true;
					self.state.mesure  		= false;
					self.dirty 				= true;
					}
				Some(topbar::EDIT_MESURE) =>
					{
					self.state.edit_route 	= false;
					self.state.edit_wp  	= false;
					self.state.mesure  		= true;
					self.dirty = true;
					}
				Some(topbar::CLEAR_ROUTE) =>
					{
					self.state.goto = false;
					self.route.clear();
					self.dirty = true;
					}
				Some(topbar::FOLLOW) =>
					{
					self.state.follow = !self.state.follow;
					self.state.map_offset = (0,0);
					self.dirty = true;
					}
				Some(topbar::PREVIOUS) =>
					{
					self.nav.decrease_id();
					self.dirty = true;
					}
			    Some(topbar::GOTO) =>
					{
					self.state.goto = !self.state.goto;
					self.dirty = true;
					}
			    Some(topbar::NEXT) =>
					{
					self.nav.increase_id(self.route.get_nb_wp());
					self.dirty = true;
					}
				Some(topbar::QUITTE) =>
					{
					self.running = false;
					}
				Some(topbar::CLEAR_TRACK) =>
					{
					//self.track.reset();
					}
			    Some(topbar::SAVE_TRACK) =>	
					{
					println!("Save Track");
					let now = Local::now();
					let path = Path::new(DEFAULT_TRACK_DIR)
						.join(format!("Track-{}.gpx", now.format("%Y-%m-%d_%H-%M-%S")));
					//match self.track.save(&path)
					//	{
					//	Ok(()) => 	{
					//				println!("Trace sauvegardée : {}", path.display());
					//				},
					//	Err(e) => eprintln!("Erreur sauvegarde : {e}"),
					//	}
					}
				Some(topbar::CLEAR_WP) =>
					{
					//self.waypoints.reset();	
					}
				Some(topbar::SAVE_WP)=>
					{
					println!("Save Waypoints");
					//self.waypoints.save();
					}
				Some(name) =>  // changement de carte
					{
					if let Some(i) = self.charts_name.iter().position(|n| n == name) {
						match self.renderer.change_map(&self.charts_path[i])
							{
							Ok(()) => 
								{
								self.state.map = self.charts_path[i].clone();
								self.state.map_offset= (0,0);               // remettre le déplacement à zéro
								}
							Err(e) => eprintln!("Impossible d'ouvrir {} : {e}", &self.charts_path[i]),
							}
						}
					}
				_ => {}
				}
		}

	fn handle_mouse_down(&mut self, x :u32, y: u32) {
			if self.topbar.mouse(x, y) {
							self.handle_topbar_event(x, y);
							}
						else if self.topbar.is_open(){
							// clic en dehors d'un topbar ouvert : on le ferme et on avale le clic
							self.topbar.close();
							self.press = None;
							}
						else {			// clic sur la carte géré par MouseMotion
							self.drag_start = (x as i32, y as i32);
							self.press = Some(Press { start: Instant::now(), x: x as i32, y: y as i32, moved: false, long_fired: false });
							}
			}

	fn handle_mouse_up(&mut self, x: i32, y: i32,map: &mut screen::Map<'_>,) {
			//println!("Handle mouse up {} {}",x,y);
			if let Some(p) = self.press.take(){ // fin du deplacement de la carte
				if p.moved{
					println!("finish drag map");
					if self.is_draging  { // drag map
					    let dx = x - self.drag_start.0;
					    let dy = y - self.drag_start.1;
					    //println!("Drag terminé, déplacement total : ({}, {})", dx, dy);
					    self.state.position = Some(self.renderer.screen_to_gps(dx, dy));
					    self.dirty = true;
					    self.is_draging = false;
					    self.state.map_offset.0 += dx;
					    self.state.map_offset.1 += dy;
					    }
					}
				else if  !p.long_fired {
					self.handle_short_click(map, x, y);
					}
				}
			}

	fn handle_mouse_motion_edit_route(
			&mut self, 
			x: i32, 
			y: i32, 
			xrel: i32, 
			yrel: i32,
			map: &mut screen::Map<'_>,
			texture: &sdl2::render::Texture,) -> Result<(), String>{
		self.selected = self.route.find_point_at(&self.renderer, (x, y), 8);
		if self.selected.is_some() {
			println!("selected : {:?}", self.selected);
			//map.cursor_size.set();
			}
		if self.route.find_midpoint_at(&self.renderer, (x, y), 8).is_some(){
			//map.cursor_arrow.set();
			}
		if let Some(p) = self.press.as_mut() {
				if let Some(i) = self.selected { // il y un waypoint sous la souris
					if !p.moved{
						p.moved = true;
						}
					if p.moved  {// deplacement du waypoint avec la souris
						let dx = map.screen_width as i32 / 2 - x;
						let dy = map.screen_height as i32 / 2 - y;
						//map.cursor_size.set();
						let (lon, lat) = self.renderer.screen_to_gps(dx , dy);
						self.route.modify(i, lon, lat);
						self.dirty = true;
						println!("Deplace un point {:?}",self.selected);
						}
					}
				}
		else{
			self.handle_move_map(x, y, xrel, yrel, map, texture)?;
			}
		Ok(())
		}
		
	fn handle_mouse_motion(
		&mut self,
		x: u32,
		y: u32,
		xrel: i32,
		yrel: i32,
		map: &mut screen::Map<'_>,
		texture: &sdl2::render::Texture,
	) -> Result<(), String> {
			//println!("Handle mouse motion {} {} {} {}", x, y, xrel, yrel);
			self.topbar.motion(x as u32, y as u32);
			if  self.topbar.mouse(x as u32,y as u32) { 
					//map.cursor_arrow.set();
					}
			else{
				let (x, y) = (x as i32, y as i32);
				if self.state.edit_route {
					self.handle_mouse_motion_edit_route(x, y, xrel, yrel, map, texture)?;
					}
				else{
					self.handle_move_map(x, y, xrel, yrel, map, texture)?;
					}
				//println!("Mouse Move");
				}
			Ok(())
			}

	fn handle_mouse_wheel(&mut self, delta: i32) {
			println!("Handle mouse whell {}", delta);
			}

	fn handle_mouse_event(
		&mut self,
		event: &Event,
		map: &mut screen::Map<'_>,
		texture: &sdl2::render::Texture,
	) -> Result<(), String> {
		match event {

			Event::MouseButtonDown { mouse_btn, x, y, .. } => {
				if *mouse_btn == sdl2::mouse::MouseButton::Left {
					if let (Ok(x), Ok(y)) = (u32::try_from(*x), u32::try_from(*y)) {
						self.handle_mouse_down(x, y);
						}
					}
				}

			Event::MouseButtonUp { mouse_btn, x,y,.. } => {
				if *mouse_btn == sdl2::mouse::MouseButton::Left {
					self.handle_mouse_up(*x, *y, map);
					}
				}

			Event::MouseMotion { x, y, xrel,yrel,.. } => {
				if let (Ok(x), Ok(y), Ok(xrel), Ok(yrel)) = 
					(u32::try_from(*x), u32::try_from(*y), i32::try_from(*xrel), i32::try_from(*yrel)){
						self.handle_mouse_motion(x, y, xrel, yrel, map, texture)?;
						}
					}
			
			Event::MouseWheel { y, direction,.. } => {
				let delta = match direction {
						sdl2::mouse::MouseWheelDirection::Flipped => -y.signum(),
			_ 			=> y.signum(),
		    			};
				self.handle_mouse_wheel(delta);
				}

			_ => {}
			}
		Ok(())
		}

	fn match_event(
		&mut self,
		event: Event,
		map: &mut screen::Map<'_>,
		texture: &sdl2::render::Texture,
	) -> Result<(), String> {
		match &event {
			Event::KeyDown { .. }  => {
				//println!("Keyboard event");
				self.handle_keyboard_event(&event, map);
			}
			Event::MouseButtonDown { .. }
			| Event::MouseButtonUp { .. }
			| Event::MouseMotion { .. }
			| Event::MouseWheel { .. } => {
				self.handle_mouse_event(&event, map, texture)?;
			}
			Event::Quit { .. } => {
				self.running = false;
			}
			_ => {}
			}
		Ok(())
		}
}
