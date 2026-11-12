
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
mod route;
mod mesure;
mod gps;
mod track;
mod waypoints;


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
	let mut map   = screen::Map::new(&sdl.ttf_context, &sdl.context).map_err(anyhow::Error::msg)?;

	let texture_creator = map.canvas.texture_creator();
	let mut texture = texture_creator
		.create_texture_streaming(PixelFormatEnum::RGBA32, map.screen_width, map.screen_height)
		.map_err(anyhow::Error::msg)?;
	let pitch = (map.screen_width * 4) as usize;

	let mut plotter = Plotter::new(&sdl.context, &sdl.ttf_context, map.screen_width, map.screen_height)?;
	plotter.main_loop(&mut sdl.event_pump, &mut map, &mut texture, pitch)
	}

#[derive(Debug, PartialEq, Clone, Copy, Default)]
pub enum Mode {
	EditRoute,
	EditWaypoint,
	#[default]
	Measurement,
}

fn valid_boat_position(boat: &BoatState) -> bool {
	boat.position_valid
		&& boat.longitude.is_finite()
		&& boat.latitude.is_finite()
		&& (-180.0..=180.0).contains(&boat.longitude)
		&& (-90.0..=90.0).contains(&boat.latitude)
}

enum AppAction {
	SetMode(Mode),
	CycleMode,
	ToggleFollow,
	ToggleGoto,
	ClearRoute,
	PreviousWaypoint,
	NextWaypoint,
	Quit,
	ClearTrack,
	SaveTrack,
	ClearWaypoints,
	SaveWaypoints,
	ChangeChart(usize),
	Pan(i32, i32),
	ZoomIn,
	ZoomOut,
}

struct Press {
    start: Instant,
    x: i32,
    y: i32,
    moved: bool,
    long_fired: bool,
}

struct PendingWaypoint {
	position: (f64, f64),
	name: String,
}

 #[derive(Debug)]
enum Mouse {
	Hand,
	Cross,
	Arrow,
	Move,
	#[allow(dead_code)]
	Wait,
}

struct Plotter<'ttf> {
	renderer: MBTilesRenderer,
	state: state::State,
	mode: Mode,
	running: bool,
	dirty: bool,
	is_draging: bool,
	xdrag:i32,
	ydrag:i32,
	drag_start: (i32,i32),
	boat: BoatState,
	topbar: topbar::Topbar<'ttf>,
	buttombar: buttombar::Buttombar<'ttf>,
	route:route::Route,
	mesure:mesure::Mesure,
	track:track::Track,
	waypoints:waypoints::Waypoints,
	pending_waypoint: Option<PendingWaypoint>,
	text_input: sdl2::keyboard::TextInputUtil,
	charts_path: Vec<String>,
	charts_name: Vec<String>,
	press: Option<Press>,
	selected: Option<usize>,
	mouse: Mouse,
	state_rx: watch::Receiver<BoatState>,
    track_rx: tokio::sync::mpsc::UnboundedReceiver<(f64, f64)>,
    ais_rx: watch::Receiver<Vec<ais::AisTarget>>,
	#[allow(dead_code)]
    status_rx: watch::Receiver<String>,
	ais_targets: Vec<ais::AisTarget>,
	last_ais_cleanup: Instant,
}

impl<'ttf> Plotter<'ttf> {

	fn new(
		sdl_context: &sdl2::Sdl,
		ttf_context: &'ttf Sdl2TtfContext,
		width: u32,
		height: u32,
	) -> anyhow::Result<Self> {
		let text_input = sdl_context
			.video()
			.map_err(anyhow::Error::msg)?
			.text_input();
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
		let route  		= route::Route::new().map_err(anyhow::Error::msg)?;
		let mesure 		= mesure::Mesure::new().map_err(anyhow::Error::msg)?;
		let track    	= track::Track::new(Path::new(DEFAULT_TRACK_PATH))?;
		let waypoints= waypoints::Waypoints::new()?;


		Ok(Self { 	renderer, 
					state, 
					mode: Mode::default(),
					running: true, 
					dirty: true, 
					is_draging: false, 
					xdrag: 0,
					ydrag: 0,
					drag_start: (0,0),
					boat, 
					topbar, 
					buttombar,
					route,
					mesure,
					track,
					waypoints,
					pending_waypoint: None,
					text_input,
					charts_path, 
					charts_name, 
					press: None, 
					selected: None,
					mouse: Mouse::Hand,
					state_rx, 
					track_rx, 
					ais_rx, 
					status_rx,
					ais_targets: Vec::new(),
					last_ais_cleanup: Instant::now(),
				 })
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
			
			////////////////////// Track update ///////////////////////
			while let Ok(position) = self.track_rx.try_recv() {
				self.track.add(position);
				self.dirty = true;
				}

			//////////////////////   Position update  /////////////////////////
			if self.state_rx.has_changed()? {
				let boat = *self.state_rx.borrow_and_update();
				self.boat = boat;

				if self.state.follow && valid_boat_position(&boat) {
					self.state.position = Some((boat.longitude, boat.latitude));
					}
    			self.dirty = true;
				}
			
			self.long_click();

			//////////////// Ais Update //////////////////////////
			if self.ais_rx.has_changed().unwrap_or(false) {
				self.ais_targets = self.ais_rx.borrow_and_update().clone();
				self.dirty = true;
        		}

			if !self.ais_targets.is_empty()
					&& self.last_ais_cleanup.elapsed() >= Duration::from_secs(1)
						{
						let previous_count = self.ais_targets.len();
						let now = Instant::now();
						self.ais_targets.retain(|target| target.is_fresh(now));
						if self.ais_targets.len() != previous_count {
							self.dirty = true;
							}
						self.last_ais_cleanup = now;
						}

			//////////////////////   Display /////////////////////////
			if self.dirty && !self.is_draging {
				self.display(map, texture, pitch).map_err(anyhow::Error::msg)?;
				self.dirty = false;
				}
			
			/////////////////  Mouse //////////////////////////////////
			self.mouse(map);

			/////////////// preload tiles on the edge ///////////////////////////
			self.renderer.prefetch_step(8);

			/////// Sleep to limit CPU usage /////////////////////////
			std::thread::sleep(Duration::from_millis(16));
			} // end while self.running

			match self.track.save(Path::new(DEFAULT_TRACK_PATH)) {
						Ok(()) => 	{
									println!("Trace sauvegardée");
									},
						Err(e) => eprintln!("Erreur sauvegarde : {e}"),
						}
			let _ = self.state.save(STATE_FILE);
			Ok(())
			}

	fn display(&mut self, map: &mut screen::Map<'_>, texture: &mut sdl2::render::Texture, pitch: usize) -> anyhow::Result<()> {
		
		////////////////// Map display ///////////////////////
		if self.state.position == None{
			self.state.position = Some(self.renderer.tiles_center().ok_or(anyhow::anyhow!("no tiles center!!!"))?);
			}
		let (lon, lat) = match self.state.follow { // mode fixed or follow boat
			true if valid_boat_position(&self.boat) => {
				( self.boat.longitude,  self.boat.latitude)
				}
			_ => self.state.position.ok_or(anyhow::anyhow!("position de la carte inconnue"))?,
			};
		let pixels = self.renderer.get_raw_rgba(lon, lat, self.state.map_offset);
		texture
			.update(None, &pixels, pitch)
			.map_err(|error| anyhow::anyhow!(error))?;
		map.show_map(texture).map_err(anyhow::Error::msg)?;

		//////////////// Mesure display   ////////////  
	    if self.mode == Mode::Measurement {
	        self.mesure.show(&self.renderer, map);
	    	}
		
		////////////////// Route display /////////////////
		self.display_route(map);

		/////////////////// Boat display  /////////////////
	    self.boat.show( &self.renderer, map);

		////////// Ais Display ////////////////////////////
		ais::show_targets(&self.ais_targets, &self.renderer, map)
			.map_err(anyhow::Error::msg)?;

		/////////// Display track ///////////////////////////////////////////
	    self.track.show(&mut map.canvas, &self.renderer, Some(self.boat.position()));

		//////////// Dispaly Waypoints ///////////////////////
		self.waypoints.show(map, &self.renderer).map_err(anyhow::Error::msg)?;

		/////////////// Topbar display ///////////////////////
		self.topbar.show(&mut map.canvas);

		/////////////// Buttombar display ////////////////////
		self.display_buttombar(map);

		map.night_layer(0).map_err(anyhow::Error::msg)?;

		Ok(())
		}

	fn mouse(&mut self, map: &mut screen::Map){
		//-println!("Mouse : {:?}", self.mouse);
		match self.mouse{
			Mouse::Arrow => map.cursor_arrow.set(), 
			Mouse::Cross => map.cursor_cross.set(), 
			Mouse::Hand  => map.cursor_hand.set(),
			Mouse::Move  => map.cursor_size.set(), 
			Mouse::Wait  => map.cursor_wait.set(), 
			}
		}

	fn display_buttombar(&mut self, map: &mut screen::Map<'_>){
		if let Some(pending) = &self.pending_waypoint {
			let message_left = format!("Nom du waypoint : {}", pending.name);
			let message_right = "Entrée : valider | Échap : annuler";
			self.buttombar.show(&mut map.canvas, &message_left, message_right);
			return;
		}
		
		let message_left = 
			if self.state.goto{
				let (dist_wp, bearing_wp, dist_total) = self.route.calc_route_mn_from_boat(self.state.nav_id, self.boat.position());
				format!("From Boat to Next Waypoint :  {:.1} Mn   {:.0}° / DTD : {:.1} Mn",dist_wp, bearing_wp, dist_total)
				} 
			else 
				{
				self.route.show_route_len()
				};
		
		let mut message_right =
			match self.mode{
				Mode::EditRoute 	=> "Mode Edit Route".to_string(),
				Mode::EditWaypoint => "Mode Edit Waypoint".to_string(),
				Mode::Measurement 	=> "Mode Measurement".to_string(),
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
			self.route.show_route_from_boat(&self.renderer, map, self.mode,self.state.nav_id, self.boat.position());
			}
	    else{
			self.route.show_route_since_id(&self.renderer, map, self.mode, 0);
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
				self.mouse = Mouse::Hand;
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
			if self.selected == None{
				if let Some(k) = self.route.find_midpoint_at(&self.renderer, (x, y), 8) // nouveau point entre k et k+1
					{
					let (longitude, latitude) = self.route.midpoints()[k];
					self.route.insert(k, longitude, latitude);     
					println!("Add waypoint between two waypoint");
					}
				else 	
					{                      // nouveau point en fin de route
					let dx = map.screen_width as i32 / 2 - x;
					let dy = map.screen_height as i32 / 2 - y;
					let (longitude,latitude) = self.renderer.screen_to_gps(dx, dy);
					self.route.add(longitude,latitude);
					println!("Add waypoint at the end of the route");
					//println!("route {route:?}");
					}
				self.mouse = Mouse::Cross;
				}
			self.route.save();
			self.dirty = true;
			}

	fn handle_click_edit_wp(&mut self,map: &mut screen::Map<'_>,x :i32, y: i32){
			println!("Edit waypoint click");
			let dx = map.screen_width as i32 / 2 - x;
			let dy = map.screen_height as i32 / 2 - y;
			self.pending_waypoint = Some(PendingWaypoint {
				position: self.renderer.screen_to_gps(dx, dy),
				name: String::new(),
			});
			self.text_input.start();
			self.dirty = true;
			}

	fn handle_short_click(&mut self, map: &mut screen::Map<'_>, x :i32, y: i32){
		//println!("Click court en ({}, {})", x, y);
		match self.mode{
			Mode::Measurement 	=> self.handle_click_mesure(map, x,y),
			Mode::EditRoute 	=> self.handle_click_edit_route(map, x,y),
			Mode::EditWaypoint => self.handle_click_edit_wp(map, x, y),
			};
		}

	fn handle_long_click_edit_route(&mut self, x: i32, y: i32){

		println!("Delete Point on route");
		if let Some(i) = self.route.find_point_at(&self.renderer, (x, y), 8) 
			{
			self.route.remove(i);
			self.route.save();
			self.dirty = true;
			self.mouse = Mouse::Cross;
			}
		}

	fn handle_long_click_edit_wp(&mut self,x :i32, y: i32){
		println!("Delete Point waypoint");
		if let Some(i) = self.waypoints.find_point_at(&self.renderer, (x, y), 8) {
			self.waypoints.remove(i);
			if let Err(error) = self.waypoints.save() {
				eprintln!("Erreur sauvegarde des waypoints : {error}");
			}
			self.dirty = true;
			self.mouse = Mouse::Cross;
			}
		}
		
	fn handle_long_click(&mut self, x :i32, y: i32){
		println!("Click long en ({}, {})", x, y);
		match self.mode{
			Mode::EditRoute 	=> self.handle_long_click_edit_route(x, y),
			Mode::EditWaypoint => self.handle_long_click_edit_wp(x, y),
			Mode::Measurement 	=> {},
			};
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

	fn handle_pending_waypoint_event(&mut self, event: &Event) {
		match event {
			Event::TextInput { text, .. } => {
				if let Some(pending) = self.pending_waypoint.as_mut() {
					for character in text.chars().filter(|character| !character.is_control()) {
						if pending.name.chars().count() >= 64 {
							break;
						}
						pending.name.push(character);
					}
				}
				self.dirty = true;
			}
			Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::Return | sdl2::keyboard::Keycode::KpEnter), repeat: false, .. } => {
				if self.pending_waypoint.as_ref().is_some_and(|pending| !pending.name.trim().is_empty()) {
					if let Some(pending) = self.pending_waypoint.take() {
						self.text_input.stop();
						self.waypoints.add(pending.position, pending.name.trim().to_string());
						if let Err(error) = self.waypoints.save() {
							eprintln!("Erreur sauvegarde des waypoints : {error}");
						}
						println!("Waypoint ajouté : {}", pending.name.trim());
					}
				}
				self.dirty = true;
			}
			Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::Escape), repeat: false, .. } => {
				self.pending_waypoint = None;
				self.text_input.stop();
				self.dirty = true;
			}
			Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::Backspace), repeat: false, .. } => {
				if let Some(pending) = self.pending_waypoint.as_mut() {
					pending.name.pop();
				}
				self.dirty = true;
			}
			_ => {}
		}
	}

	fn handle_keyboard_event(&mut self, event: &Event, _map: &mut screen::Map<'_>) {
		if self.pending_waypoint.is_some() {
			self.handle_pending_waypoint_event(event);
			return;
		}
		match event {
			Event::KeyDown { keycode: Some(keycode), keymod, repeat: false, .. } => {
				let fast = keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
				let distance = if fast { 1000 } else { 100 };
				let action = match *keycode {
					sdl2::keyboard::Keycode::Escape => Some(AppAction::Quit),
					sdl2::keyboard::Keycode::Up => Some(AppAction::Pan(0, distance)),
					sdl2::keyboard::Keycode::Down => Some(AppAction::Pan(0, -distance)),
					sdl2::keyboard::Keycode::Left => Some(AppAction::Pan(distance, 0)),
					sdl2::keyboard::Keycode::Right => Some(AppAction::Pan(-distance, 0)),
					sdl2::keyboard::Keycode::KpPlus => Some(AppAction::ZoomIn),
					sdl2::keyboard::Keycode::KpMinus => Some(AppAction::ZoomOut),
					sdl2::keyboard::Keycode::F => Some(AppAction::ToggleFollow),
					sdl2::keyboard::Keycode::Tab => Some(AppAction::CycleMode),
					sdl2::keyboard::Keycode::P => Some(AppAction::PreviousWaypoint),
					sdl2::keyboard::Keycode::N => Some(AppAction::NextWaypoint),
					_ => None,
				};
				if let Some(action) = action {
					self.execute_action(action);
				}
			}
			_ => {}
			}
		}

	fn handle_topbar_event(&mut self, x: u32, y: u32) {
		if let Some(name) = self.topbar.event(x, y) {
			let action = match name.as_str() {
				topbar::EDIT_ROUTE => Some(AppAction::SetMode(Mode::EditRoute)),
				topbar::EDIT_WP => Some(AppAction::SetMode(Mode::EditWaypoint)),
				topbar::EDIT_MESURE => Some(AppAction::SetMode(Mode::Measurement)),
				topbar::CLEAR_ROUTE => Some(AppAction::ClearRoute),
				topbar::FOLLOW => Some(AppAction::ToggleFollow),
				topbar::PREVIOUS => Some(AppAction::PreviousWaypoint),
				topbar::GOTO => Some(AppAction::ToggleGoto),
				topbar::NEXT => Some(AppAction::NextWaypoint),
				topbar::QUITTE => Some(AppAction::Quit),
				topbar::CLEAR_TRACK => Some(AppAction::ClearTrack),
				topbar::SAVE_TRACK => Some(AppAction::SaveTrack),
				topbar::CLEAR_WP => Some(AppAction::ClearWaypoints),
				topbar::SAVE_WP => Some(AppAction::SaveWaypoints),
				_ => self
					.charts_name
					.iter()
					.position(|chart_name| chart_name == &name)
					.map(AppAction::ChangeChart),
			};
			if let Some(action) = action {
				self.execute_action(action);
			}
		}
	}

	fn execute_action(&mut self, action: AppAction) {
		match action {
			AppAction::SetMode(mode) => self.mode = mode,
			AppAction::CycleMode => {
				self.mode = match self.mode {
					Mode::EditRoute => Mode::EditWaypoint,
					Mode::EditWaypoint => Mode::Measurement,
					Mode::Measurement => Mode::EditRoute,
				};
			}
			AppAction::ToggleFollow => {
				self.state.follow = !self.state.follow;
				self.center_map_on_visible_center();
			}
			AppAction::ToggleGoto => self.state.goto = !self.state.goto,
			AppAction::ClearRoute => {
				self.state.goto = false;
				self.route.clear();
			}
			AppAction::PreviousWaypoint => {
				self.state.nav_id = self.state.nav_id.saturating_sub(1);
			}
			AppAction::NextWaypoint => {
				let last = self.route.get_nb_wp().saturating_sub(1);
				self.state.nav_id = self.state.nav_id.saturating_add(1).min(last);
			}
			AppAction::Quit => self.running = false,
			AppAction::ClearTrack => self.track.reset(),
			AppAction::SaveTrack => {
				println!("Save Track");
				let now = Local::now();
				let path = Path::new(DEFAULT_TRACK_DIR)
					.join(format!("Track-{}.gpx", now.format("%Y-%m-%d_%H-%M-%S")));
				match self.track.save(&path) {
					Ok(()) => println!("Trace sauvegardée : {}", path.display()),
					Err(error) => eprintln!("Erreur sauvegarde : {error}"),
				}
			}
			AppAction::ClearWaypoints => self.waypoints.reset(),
			AppAction::SaveWaypoints => {
				println!("Save Waypoints");
				if let Err(error) = self.waypoints.save() {
					eprintln!("Erreur sauvegarde des waypoints : {error}");
				}
			}
			AppAction::ChangeChart(index) => {
				match self.renderer.change_map(&self.charts_path[index]) {
					Ok(()) => {
						self.state.map = self.charts_path[index].clone();
						self.state.map_offset = (0, 0);
					}
					Err(error) => eprintln!(
						"Impossible d'ouvrir {} : {error}",
						&self.charts_path[index]
					),
				}
			}
			AppAction::Pan(dx, dy) => {
				if self.state.follow {
					self.state.map_offset.0 += dx;
					self.state.map_offset.1 += dy;
				} else {
					self.state.position = Some(self.renderer.screen_to_gps(dx, dy));
					self.state.map_offset = (0, 0);
				}
			}
			AppAction::ZoomIn => {
				//self.center_map_on_visible_center();
				self.state.zoom = self.renderer.zoom_increase(&self.state.map);
			}
			AppAction::ZoomOut => {
				//self.center_map_on_visible_center();
				self.state.zoom = self.renderer.zoom_decrease();
			}
		}
		self.dirty = true;
	}

	fn center_map_on_visible_center(&mut self) {
		if let Some(position) = self.state.position {
			self.state.position = Some(
				self.renderer
					.center_with_offset(position, self.state.map_offset),
			);
			self.state.map_offset = (0, 0);
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
					    self.dirty = true;
					    self.is_draging = false;
					    self.execute_action(AppAction::Pan(dx, dy));
						self.mouse = Mouse::Cross;
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
			self.mouse = Mouse::Move;
			}
		if self.route.find_midpoint_at(&self.renderer, (x, y), 8).is_some(){
			self.mouse = Mouse::Arrow
			}
		if let Some(p) = self.press.as_mut() {
				if let Some(i) = self.selected { // il y un waypoint sous la souris
					if !p.moved{
						p.moved = true;
						}
					if p.moved  {// deplacement du waypoint avec la souris
						let dx = map.screen_width as i32 / 2 - x;
						let dy = map.screen_height as i32 / 2 - y;
						self.mouse = Mouse::Move;
						let (lon, lat) = self.renderer.screen_to_gps(dx , dy);
						self.route.modify(i, lon, lat);
						self.dirty = true;
						println!("Deplace un point {:?}",self.selected);
						}
					}
			else{
				self.handle_move_map(x, y, xrel, yrel, map, texture)?;
				}
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
					self.mouse = Mouse::Arrow;
					}
			else{
				let (x, y) = (x as i32, y as i32);
				self.mouse = Mouse::Cross;
				match self.mode {
					Mode::EditRoute => self.handle_mouse_motion_edit_route(x, y, xrel, yrel, map, texture)?,
					Mode::EditWaypoint => self.handle_move_map(x, y, xrel, yrel, map, texture)?,
					Mode::Measurement => self.handle_move_map(x, y, xrel, yrel, map, texture)?,
					};
				}
			Ok(())
			}

	fn handle_mouse_wheel(&mut self, delta: i32) {
			println!("Handle mouse whell {}", delta);
			if delta > 0 {
				self.state.zoom = self.renderer.zoom_increase(&self.state.map); // retourne le nouveau pour information
				}
		    else if delta < 0 {
				self.state.zoom = self.renderer.zoom_decrease();   // retourne le nouveau pour information
				}
			self.state.map_offset = (0,0);
		    //map.cursor_wait.set();
		    //self.is_zooming = true;
		    self.dirty = true;
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
			Event::KeyDown { .. } | Event::TextInput { .. } => {
				//println!("Keyboard event");
				self.handle_keyboard_event(&event, map);
			}
			Event::MouseButtonDown { .. }
			| Event::MouseButtonUp { .. }
			| Event::MouseMotion { .. }
			| Event::MouseWheel { .. } => {
				if self.pending_waypoint.is_none() {
					self.handle_mouse_event(&event, map, texture)?;
				}
			}
			Event::Quit { .. } => {
				self.running = false;
			}
			_ => {}
			}
		Ok(())
		}
}

#[cfg(test)]
mod tests {
	use super::valid_boat_position;
	use crate::boat::BoatState;

	#[test]
	fn accepts_received_positions_within_gps_bounds() {
		let mut boat = BoatState::default();
		boat.position_valid = true;
		assert!(valid_boat_position(&boat));

		boat.longitude = 180.0;
		boat.latitude = 90.0;
		assert!(valid_boat_position(&boat));

		boat.longitude = -180.0;
		boat.latitude = -90.0;
		assert!(valid_boat_position(&boat));
	}

	#[test]
	fn rejects_missing_non_finite_or_out_of_range_boat_positions() {
		let mut boat = BoatState::default();
		assert!(!valid_boat_position(&boat));

		boat.position_valid = true;
		boat.longitude = f64::NAN;
		assert!(!valid_boat_position(&boat));

		boat.longitude = 0.0;
		boat.latitude = f64::INFINITY;
		assert!(!valid_boat_position(&boat));

		boat.latitude = 0.0;
		boat.longitude = 180.1;
		assert!(!valid_boat_position(&boat));

		boat.longitude = 0.0;
		boat.latitude = -90.1;
		assert!(!valid_boat_position(&boat));
	}
}
