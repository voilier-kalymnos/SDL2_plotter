
// Install SDL2 driver
// sudo apt update && sudo apt install -y libsdl2-2.0-0
// sudo apt install -y libsdl2-ttf-2.0-0 libsdl2-image-2.0-0 libsdl2-mixer-2.0-0
// sudo apt install libdrm-dev libgbm-dev libegl1-mesa-dev
// pkill -9 plotter
// sudo apt install libsdl2-gfx-1.0-0

mod mbtiles_renderer;
mod screen;
mod signalk;
mod route;
mod mesure;
mod topbar;
mod buttombar;
mod nav;
mod gps;
mod boat;
mod track;
mod state;

use mbtiles_renderer::MBTilesRenderer;
use crate::state::State;
use std::process;
use crate::boat::BoatState;

use sdl2::pixels::PixelFormatEnum;
use sdl2::event::Event;
use sdl2::mouse::MouseButton;
use sdl2::keyboard::{Keycode, Mod};
use tokio::sync::watch;
use std::time::{Duration, Instant};
use std:: path::Path;
use chrono::Local;

const MAP_WORLD	:&str			= "assets/OSM.mbtiles";
const STATE_FILE :&str  		= "state.json"; 
const DEFAULT_TRACK_DIR: &str	= "track/";
const DEFAULT_TRACK_PATH: &str	= "track/default.gpx";

const LONG_CLICK: Duration 		= Duration::from_millis(500);
const MOVE_TOLERANCE: i32 		= 5;
const TRACK_MINI :f64			= 20.0; // métres
const TRACK_TOLERANCE :f64		= 20.0;  // métres
const TRACK_GAP :f64			= 1500.0;    // métres

struct Press {
    start: Instant,
    x: i32,
    y: i32,
    moved: bool,
    long_fired: bool,
}

 #[tokio::main]
async fn main() ->Result<(), Box<dyn std::error::Error>> {
    
    let charts_path = list_maps("map");
    let charts_name: Vec<String> = charts_path
            .iter()
            .map(|c| display_name(c).to_string())
            .collect();
    
    let mut state = State::load(STATE_FILE).unwrap_or_else(|_| State 
	{
	edit_route : false,
	edit_wp    : false,
	mesure     : true,
	goto       : false,
	follow     : false,
	map	   : charts_path[0].clone(),
	position   : None,
	zoom       : 0,
	map_offset : (0,0),
	});
	
    let mut route  				= route::Route::new()?;
    let mut mesure 				= mesure::Mesure::new()?;
    let mut boat  			= boat::BoatState::default();
    let mut screen   		= screen::MoteurSdl::init()?;
    let mut map      			= screen::Map::new(&screen.ttf_context, &screen.context)?;
    let mut topbar    		= topbar::Topbar::new(&screen.ttf_context, map.screen_width, charts_name.clone()).expect("failed to build topbar");    
    let mut buttombar 	= buttombar::Buttombar::new(&screen.ttf_context, map.screen_width,  map.screen_height).expect("failed to build buttombar bar");
    let mut nav 					= nav::Nav::new();
    let mut track       			= track::Track::new(
		TRACK_MINI,
		TRACK_TOLERANCE,
		TRACK_GAP,
		Path::new(DEFAULT_TRACK_PATH)
	)?;
    
    let _ = map.info("Initialise Mbtiles...");
    let mut renderer	= match MBTilesRenderer::new(
		&state.map,
		state.zoom,
		map.screen_width,
		map.screen_height,
		){
		Ok(r) => r,
		Err(e) => {
		    let msg = format!("{e}");
		    let _ = map.info(&msg);
		    std::thread::sleep(std::time::Duration::from_secs(4));
		    process::exit(1);
		    }
		};

    if state.position == None
	{
	state.position = Some(renderer.tiles_center().ok_or("no tiles center!!!")?);
	}
    if let Some((lon, lat)) = state.position 
	{
	println!("center map longitude : {} / latitude : {}", lon, lat);
	}

    let texture_creator =  map.canvas.texture_creator();
    let mut texture =
        texture_creator.create_texture_streaming(PixelFormatEnum::RGBA32, map.screen_width, map.screen_height)?;
    let pitch = (map.screen_width * 4) as usize; // 4 octets par pixel (RGBA8)

    // Channels de communication avec la tache signalk (donnees pures, pas de SDL).
    let (state_tx, mut state_rx)   = watch::channel(BoatState::default());
    let (status_tx, mut status_rx) = watch::channel(String::new());
    tokio::spawn(signalk::signalk_task(state_tx, status_tx));

    // Initialisation des variables
    
    let mut is_dragging = false;
    let mut dirty 	= false;
    let mut is_zooming  = false;
    let mut drag_start 	= (0i32, 0i32);
    let mut xdrag	= 0;
    let mut ydrag 	= 0;


    let mut press: Option<Press> = None;
    let mut selected: Option<usize>;


///////////////////////////////////////////////////////////////////////////////////////////////////////
    'main: loop {
//////////////////////////////////////////////////////////////////////////////////////////////////////
        // Affiche les messages de statut envoyes par la tache signalk
        // (connexion, souscription, attente position, reconnexion...).
        if status_rx.has_changed().unwrap_or(false) {
            let msg = status_rx.borrow_and_update().clone();
            if !msg.is_empty() {
		println!("status : {}",&msg);
                let _ = map.info(&msg);
            }
        }

        // Recupere le dernier etat du  bateau connu (non bloquant).
	
        if state_rx.has_changed().unwrap_or(false) {
            let data = *state_rx.borrow_and_update();
	    boat.longitude = data.longitude;
	    boat.latitude  = data.latitude;
	    boat.course_over_ground = data.course_over_ground;
	    boat.speed_over_ground = data.speed_over_ground;
	    //println!("boatstate longitude : {} / latitude : {} / cos : {} / sog : {}",boat.longitude,boat.latitude,boat.course_over_ground,boat.speed_over_ground,);
	    dirty = true;
	    
            if !is_dragging 
		{
		boat.longitude 	   = data.longitude;
		boat.latitude  	   = data.latitude;
		boat.show(&renderer, &mut map);
		}
            if state.follow
		{
		state.position = Some((data.longitude, data.latitude));
                dirty = true;
		}
        }
	
        while let Some(event) = screen.event_pump.poll_event() 
	    {
            match event 
		{
		// move map left
		Event::KeyDown { keycode: Some(Keycode::Left), keymod, .. } => {
			if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD) {
			    // Shift + Left
			    state.position = Some(renderer.screen_to_gps(1000,0));
			    
			    println!("Shift+Left détecté");
			} else {
			    // Left seul
			    state.position = Some(renderer.screen_to_gps(100,0));
			    println!("Left seul");
			}
			dirty = true;
		    }

		// move map right
		Event::KeyDown { keycode: Some(Keycode::Right), keymod, .. } => {
			if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD) {
			    // Shift + Left
			    state.position = Some(renderer.screen_to_gps(-1000,0));
			    println!("Shift+Left détecté");
			} else {
			    // Left seul
			    state.position = Some(renderer.screen_to_gps(-100,0));
			    println!("Left seul");
			}
			dirty = true;
		    }

		// move map down
		Event::KeyDown { keycode: Some(Keycode::Down), keymod, .. } => {
			if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD) {
			    // Shift + Left
			    state.position = Some(renderer.screen_to_gps(0,-500));
			    println!("Shift+Left détecté");
			} else {
			    // Left seul
			    state.position = Some(renderer.screen_to_gps(0,-100));
			    println!("Left seul");
			}
			dirty = true;
		    }

		// move map up
		Event::KeyDown { keycode: Some(Keycode::Up), keymod, .. } => {
			if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD) {
			    // Shift + Left
			    state.position = Some(renderer.screen_to_gps(0,500));
			    println!("Shift+Left détecté");
			} else {
			    // Left seul
			    state.position = Some(renderer.screen_to_gps(0,100));
			    println!("Left seul");
			}
			dirty = true;
		    }

		
		// Molette de la souris : zoom avant/arrière
		Event::MouseWheel { y, direction, mouse_x: _, mouse_y: _, .. } => 
			{
		    let delta = match direction 
				{
				sdl2::mouse::MouseWheelDirection::Flipped => -y.signum(),
			_ 		=> y.signum(),
		    	};
		    //let dx = map.screen_width as i32 / 2 - mouse_x;
		    //let dy = map.screen_height as i32 / 2 - mouse_y;
		    //state.position = Some(renderer.screen_to_gps( dx as i32, dy as i32));  // centre la carte sur la position de la souris
		    
		    if delta > 0 
				{
				state.zoom = renderer.zoom_increase(&state.map); // retourne le nouveau pour information
				}
		    if delta < 0 
				{
				state.zoom = renderer.zoom_decrease();   // retourne le nouveau pour information
				}
			state.map_offset = (0,0);
		    map.cursor_wait.set();
		    is_zooming = true;
		    dirty = true;
		}

		// zoom in (+)
		Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::KpPlus), .. } => {
		    state.zoom = renderer.zoom_increase(&state.map);
		    is_zooming = true;
		    dirty = true;
			state.map_offset = (0,0);
			//println!("position : {:.1} / {:.2} ",state.position.unwrap().0,state.position.unwrap().1);
		    map.cursor_wait.set();
		    }

		 // zoom out (-)
		Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::KpMinus), .. } => {
		    state.zoom = renderer.zoom_decrease();
		    is_zooming = true;
		    dirty = true;
			state.map_offset = (0,0);
			println!("position : {:.1} / {:.2} ",state.position.unwrap().0,state.position.unwrap().1);
		    map.cursor_wait.set();
		    }

		 // Center Boat (F)
		Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::F), .. } => {
		    println!("follow Boat");
		    
		    if state.follow
			{
			let pt = renderer.gps_to_point((boat.longitude, boat.latitude));
			let dx = map.screen_width as i32 / 2 - pt.x + state.map_offset.0;
			let dy = map.screen_height as i32 / 2 - pt.y + state.map_offset.1;
			state.position = Some(renderer.screen_to_gps(dx, dy));
			}
		    state.follow = !state.follow;
		    state.map_offset = (0,0);
		    dirty = true;
		    }
		
		 // Previous Wp (P)
		Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::P), .. } => {
		    println!("Previous WP");
		    nav.decrease_id();
		    dirty = true;
		    }
		    
		 // Next Wp (N)
		Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::N), .. } => {
		    println!("Previous WP");
		    nav.increase_id(route.get_nb_wp());
		    dirty = true;
		    }
		    
		 // goto (G)
		Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::G), .. } => {
		    println!("Goto");
		    state.goto = !state.goto;
		    dirty = true;
		    }
		    		
		// Change mode(Tab)
		Event::KeyDown { keycode: Some(Keycode::Tab), .. } => {
			if state.edit_route
				{
				state.edit_route = false;
		    	state.edit_wp = true;
		    	state.mesure = false;
				}
			else if state.edit_wp
		    	{
		    	state.edit_route = false;
		    	state.edit_wp = false;
		    	state.mesure = true;
		    	}
		    else if state.mesure
		    	{
		    	state.edit_route = true;
		    	state.edit_wp = false;
		    	state.mesure = false;
		    	}
		    else
		    	{
		    	state.edit_route = true;
		    	state.edit_wp = false;
		    	state.mesure = false;
		    	}	
		    dirty = true;
		}
		// close topbar
		Event::KeyDown { keycode: Some(Keycode::Escape), .. } => topbar.close(),
		
		Event::MouseButtonDown { mouse_btn: MouseButton::Left, x, y, .. } =>
		    {
		    if topbar.mouse(x as u32, y as u32)
			{
			// clic sur la barre ou sur le topbar déroulé : le topbar s'en occupe, pas la carte
			match topbar.event(x as u32, y as u32).as_deref()
			    {
			    None => {}   // simple ouverture / fermeture d'un topbar

			    Some(topbar::EDIT_ROUTE) =>
					{
					state.edit_route 	= true;
					state.edit_wp  		= false;
					state.mesure  		= false;
					dirty = true;
					}
				Some(topbar::EDIT_WP) =>
					{
					state.edit_route 	= false;
					state.edit_wp  		= true;
					state.mesure  		= false;
					dirty = true;
					}
				Some(topbar::EDIT_MESURE) =>
					{
					state.edit_route 	= false;
					state.edit_wp  		= false;
					state.mesure  		= true;
					dirty = true;
					}
			    Some(topbar::CLEAR_ROUTE) =>
					{
					state.goto = false;
					route.clear();
					dirty = true;
					}
			    Some(topbar::FOLLOW) =>
					{
					state.follow = !state.follow;
					if !state.follow
						{
						let pt = renderer.gps_to_point((boat.longitude, boat.latitude));
						let dx = map.screen_width as i32 / 2 - pt.x + state.map_offset.0;
						let dy = map.screen_height as i32 / 2 - pt.y + state.map_offset.1;
						state.position = Some(renderer.screen_to_gps(dx, dy));
						}
					state.map_offset = (0,0);
					dirty = true;
					}
			    Some(topbar::PREVIOUS) =>
					{
					nav.decrease_id();
					dirty = true;
					}
			    Some(topbar::GOTO) =>
					{
					state.goto = !state.goto;
					dirty = true;
					}
			    Some(topbar::NEXT) =>
					{
					nav.increase_id(route.get_nb_wp());
					dirty = true;
					}
			    Some(topbar::QUITTE) =>
					{
					match track.save(Path::new(DEFAULT_TRACK_PATH)) 
						{
						Ok(()) => 	{
									println!("Trace sauvegardée");
									},
						Err(e) => eprintln!("Erreur sauvegarde : {e}"),
						}
					let _ = state.save(STATE_FILE);
					break 'main;
					}

			     Some(topbar::CLEAR_TRACK) =>
					{
					track.reset();
					}
			    Some(topbar::SAVE_TRACK) =>
					{
					println!("Save Track");
					let now = Local::now();
					let path = Path::new(DEFAULT_TRACK_DIR)
						.join(format!("Track-{}.gpx", now.format("%Y-%m-%d_%H-%M-%S")));
					match track.save(&path)
						{
						Ok(()) => 	{
									println!("Trace sauvegardée : {}", path.display());
									},
						Err(e) => eprintln!("Erreur sauvegarde : {e}"),
						}
					}
					Some(name) => 
						{
						if let Some(i) = charts_name.iter().position(|n| n == name) 
						{
						match renderer.change_map(&charts_path[i])
							{
							Ok(()) => 
							{
							state.map = charts_path[i].clone();
							state.map_offset= (0,0);               // remettre le déplacement à zéro
							}
							Err(e) => eprintln!("Impossible d'ouvrir {} : {e}", &charts_path[i]),
							}
						}
						}
					}
			press = None;
			}
		    else if topbar.is_open()
				{
				// clic en dehors d'un topbar ouvert : on le ferme et on avale le clic
				topbar.close();
				press = None;
				}
		    else // clic sur la carte géré par MouseMotion
			{
			press = Some(Press { start: Instant::now(), x, y, moved: false, long_fired: false });
			}
		    }

            Event::MouseMotion { x, y, xrel, yrel, .. } => 
				{
				topbar.motion(x as u32, y as u32);
				//if  topbar.mouse(y as u32) 
				if  topbar.mouse(x as u32,y as u32) 
					{ 
					map.cursor_arrow.set();
					}
				else
					{
					if state.edit_route
						{
						selected = route.find_point_at(&renderer, (x, y), 8);
						if selected.is_some() 
							{
							println!("selected : {:?}", selected);
							map.cursor_size.set();
							}
						else{
							map.cursor_cross.set();
							}
						if route.find_midpoint_at(&renderer, (x, y), 8).is_some()
							{
							map.cursor_arrow.set();
							}
						if let Some(p) = press.as_mut() 
							{
							if let Some(i) = selected // il y un waypoint sous la souris
								{
								if !p.moved
									{
									p.moved = true;
									}
								if p.moved  // deplacement du waypoint avec la souris
									{
									let dx = map.screen_width as i32 / 2 - x;
									let dy = map.screen_height as i32 / 2 - y;
									map.cursor_size.set();
									let (lon, lat) = renderer.screen_to_gps(dx , dy);
									route.modify(i, lon, lat);
									dirty = true;
									println!("Deplace un point {:?}",selected);
									}
								}
							else{    // delpacement de la carte avec la souris
								if !p.moved 
									{
									let (dx, dy) = (x - p.x, y - p.y);
									if dx * dx + dy * dy > MOVE_TOLERANCE * MOVE_TOLERANCE    // demarrage du drag
										{
										println!("demarrage du drag");
										p.moved = true;
										is_dragging = true;
										drag_start = (x, y);
										xdrag = 0;
										ydrag = 0;
										map.cursor_hand.set();
										}
									}
								if p.moved  // deplacement de la carte avec la souris
									{
									if is_dragging 
									{
									xdrag += xrel;
									ydrag += yrel;
									let _ = map.drag_map(&texture,xdrag,ydrag)?;
									}
									}
								}
							}
						}	
			    
					else if state.mesure	// on n'est pas en mode edit route
						{
						if let Some(p) = press.as_mut() 
							{
							if !p.moved 
								{
								let (dx, dy) = (x - p.x, y - p.y);
								if dx * dx + dy * dy > MOVE_TOLERANCE * MOVE_TOLERANCE    // demarrage du drag
									{
									println!("demarrage du drag");
									p.moved = true;
									is_dragging = true;
									drag_start = (x, y);
									xdrag = 0;
									ydrag = 0;
									map.cursor_hand.set();
									}
								}
							if p.moved  // deplacement de la carte avec la souris
								{
								if is_dragging 
								{
								xdrag += xrel;
								ydrag += yrel;
								let _ = map.drag_map(&texture,xdrag,ydrag)?;
								}
								}
							}
						}
					else if state.edit_wp
						{
						println!(	"Mode Edit Waypoint");
						}
					else
						{
					
						}
					}
				}
			Event::MouseButtonUp { mouse_btn: MouseButton::Left,x,y, .. } => 
				{
				map.cursor_cross.set();
                    if let Some(p) = press.take() // fin du deplacement de la carte
						{
                        if p.moved 
							{
							if is_dragging   // drag map
								{
								let dx = x - drag_start.0;
								let dy = y - drag_start.1;
								//println!("Drag terminé, déplacement total : ({}, {})", dx, dy);
								state.position = Some(renderer.screen_to_gps(dx, dy));
								dirty = true;
								is_dragging = false;
								state.map_offset.0 += dx;
								state.map_offset.1 += dy;
								}
								else{
								route.save();
								}
			    			} 
							else if !p.long_fired  // clic court
								{
											println!("Clic court ({:?})", p.start.elapsed());
								match state.edit_route
								{
								false =>
									{
									println!("Mesure clic");
									let dx = map.screen_width as i32 / 2 - x;
									let dy = map.screen_height as i32 / 2 - y;
									mesure.add_mark(&renderer,dx,dy);
									dirty = true;
									}
								true =>
								{
								selected = route.find_point_at(&renderer, (x, y), 8);
								println!("selected {:?}",selected);
								if selected == None
								{
								if let Some(k) = route.find_midpoint_at(&renderer, (x, y), 8) // nouveau point entre k et k+1
									{
									let (longitude, latitude) = route.midpoints()[k];
									route.insert(k, longitude, latitude);     
									println!("Ajoute un point entre deux wp");
									}
								else 	
									{                      // nouveau point en fin de route
									let dx = map.screen_width as i32 / 2 - x;
									let dy = map.screen_height as i32 / 2 - y;
									let (longitude,latitude) = renderer.screen_to_gps(dx, dy);
									route.add(longitude,latitude);
									println!("Ajoute un point à la fin");
									//println!("route {route:?}");
									}
					    
					    		}
							route.save();
							dirty = true;
							}
						}
			    	}
				}
		    }

		_ => {}
                }
	    }
	
	if let Some(p) = press.as_mut()  // clic long to remove waypoint
	    {
            if !p.moved && !p.long_fired && p.start.elapsed() >= LONG_CLICK 
		{
                p.long_fired = true;
                println!("Clic long en ({}, {})", p.x, p.y);
		
		match state.edit_route 
		    {
		    true =>
			{
			println!("Delete Point");
			if let Some(i) = route.find_point_at(&renderer, (p.x, p.y), 8) 
			    {
			    route.remove(i);
			    route.save();
			    dirty = true;
			    }
			}
		    _ => {}
		    }
		
		}
	    }
	    
        // Redessine la carte uniquement si quelque chose a change
        // (deplacement, zoom, suivi du bateau...), et pas pendant un drag
        // (le drag a son propre rendu via map.drag_map).
        if dirty && !is_dragging 
	    {
	    
/////////////////// Affichage de la carte /////////////////////////////////

	    let pixels = match state.follow // mode fixed or follow boat
		{
		true =>
		    {
		    renderer.get_raw_rgba( boat.longitude,  boat.latitude,  state.map_offset)
		    }
		false =>
		    {
		    let (lon, lat) = state.position.ok_or("position de la carte inconnue")?;
		    renderer.get_raw_rgba(lon, lat,  (0,0))		    
		    }
		};
             
            texture
                .update(None, &pixels, pitch)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let _ = map.show_map(&texture)?;
	    
///////////////////// Affichage du bateau  ////////////////////////////////////
	    
	    boat.show( &renderer, &mut map);

////////////////// Affichage de la route  soit en mode normal ou mode nav /////////////////////////////////////////////////////
	    
	    if state.goto
			{
			route.show_route_from_boat(&renderer, &mut map, state.edit_route,nav.get_id(), boat.position());
			}
	    else
			{
			route.show_route_since_id(&renderer, &mut map, state.edit_route, 0);
			}
		
//////////////// Affichage de la mesure////////////////////////////////////////////////////

	    if state.mesure
		 	{
	        mesure.show(&renderer, &mut map);
	    	}

	    if is_zooming
			{
			map.cursor_cross.set();
			is_zooming = false;
			}
		
	    dirty = false;
	    
	    
/////////////////// topbar //////////////////////////////////////////////////

	    topbar.show(&mut map.canvas);

/////////////////////buttombar bar soit en mode normal soit en mode nav /////////

	    let message_left = 
		if state.goto
		    {
		    let (dist_wp, bearing_wp, dist_total) = route.calc_route_mn_from_boat(nav.get_id(), boat.position());
		    format!("From Boat to Next Waypoint :  {:.1} Mn   {:.0}° / DTD : {:.1} Mn",dist_wp, bearing_wp, dist_total)
		    } 
		else 
		    {
		    route.show_route_len()
		    };
		
		let mut message_right =
		if state.edit_route
			{
			"Mode Edit Route".to_string()
			}
		else if state.edit_wp
			{
			"Mode Edit Waypoint".to_string()
			}
		else if state.mesure
			{
			"Mode Mesure".to_string()
			}
		else
			{
			"No Mode".to_string()
			};

		message_right += 
		if state.follow
			{
			" / Follow"
			}
		else
			{
			""
			};
	    buttombar.show(&mut map.canvas,&message_left, &message_right);
	
	    
/////////////////////// Track ///////////////////////////////////////////

	    track.add(boat.position());
	    track.show(&mut map.canvas, &renderer, Some(boat.position()));
	
//////////////////////// Mode Nuit  ///////////////////////////////////////////

	    let _ = map.night_layer(0)?;
	    
////////////////////// prechargement des tuiles voisines /////////////////////////////////////	

	    if renderer.prefetch_step(8)  
			{ 
			// println!("loading tilles");
			}      // 4 tuiles par frame
	    }

/////////////////////// Evite de saturer le CPU (pas de vsync ici a priori) : ~60 fps max.

        std::thread::sleep(Duration::from_millis(16));
    }

    Ok(())
}

/// Liste les fichiers .mbtiles d'un dossier, triés par nom.
pub fn list_maps(dir: &str) -> Vec<String> {
    let mut maps: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e.eq_ignore_ascii_case("mbtiles")))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    maps.sort();
    maps
}

 fn display_name(path: &str) -> &str {
        Path::new(path)
            .file_stem()          // "fidji" (sans dossier ni extension)
            .and_then(|s| s.to_str())
            .unwrap_or(path)
    }  
