
// Install SDL2 driver
// sudo apt update && sudo apt install -y libsdl2-2.0-0
// sudo apt install -y libsdl2-ttf-2.0-0 libsdl2-image-2.0-0 libsdl2-mixer-2.0-0
// sudo apt install libdrm-dev libgbm-dev libegl1-mesa-dev
// sudo apt install libsdl2-gfx-1.0-0

mod mbtiles_renderer;
// pkill -9 plotter

mod screen;
mod signalk;
mod route;
mod mesure;
mod topbar;
mod buttombar;
mod nav;
mod gps;
use mbtiles_renderer::MBTilesRenderer;
use std::process;

mod boat;
use crate::boat::BoatState;

use sdl2::pixels::PixelFormatEnum;
use sdl2::event::Event;
use sdl2::mouse::MouseButton;
use sdl2::keyboard::{Keycode, Mod};
use tokio::sync::watch;
use std::time::{Duration, Instant};

const MAP_FILE	:&str	= "map/Fiji.mbtiles";
const ZOOM	:i32	= 11;

const LONG_CLICK: Duration = Duration::from_millis(500);
const MOVE_TOLERANCE: i32 = 5;

struct Press {
    start: Instant,
    x: i32,
    y: i32,
    moved: bool,
    long_fired: bool,
}


#[derive(Clone, Copy, PartialEq, Eq)]
#[derive(Debug)]
enum Mode {
    Route,
    Mesure,
}
 
#[tokio::main]
async fn main() ->Result<(), Box<dyn std::error::Error>> {
    let mut routing = route::Route::new()?;
    let mut mesuring = mesure::Mesure::new()?;
    let mut boating  = boat::BoatState::default();
    
    let mut screen = screen::MoteurSdl::init()?;
    let mut map = screen::Map::new(&screen.ttf_context, &screen.context)?;
    let mut mode = Mode::Mesure;
    
    let mut menu = topbar::Topbar::new(&screen.ttf_context, map.screen_width)
		.expect("failed to build menu");
    let mut buttom = buttombar::Buttombar::new(&screen.ttf_context, map.screen_width,  map.screen_height)
		.expect("failed to build buttom bar");

    let mut nav = nav::Nav::new();
    
    let _ = map.info("Initialise Mbtiles...");
    let mut renderer = match MBTilesRenderer::new(
            MAP_FILE,
            ZOOM,
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
    let (mut map_longitude, mut map_latitude) = renderer.tiles_center().ok_or("no tiles center!!!")?;
    println!("center map longitude : {} / latitude : {}",map_longitude,map_latitude);
    
    let texture_creator =  map.canvas.texture_creator();
    let mut texture =
        texture_creator.create_texture_streaming(PixelFormatEnum::RGBA32, map.screen_width, map.screen_height)?;
    let pitch = (map.screen_width * 4) as usize; // 4 octets par pixel (RGBA8)
    
    // gestion des niveaux de zoom
    let zooms = renderer.available_zooms().to_vec();
    for x in &zooms  { println!("zoom : {}",x)} 
    let mut zoom_index = 0 as i32;
    let mut zoom = zooms[zoom_index as usize];
    let dernier_index_valide = zooms.len().saturating_sub(1) as i32;
    
    let mut selected: Option<usize>;

    // Channels de communication avec la tache signalk (donnees pures, pas de SDL).
    let (state_tx, mut state_rx)   = watch::channel(BoatState::default());
    let (status_tx, mut status_rx) = watch::channel(String::new());
    tokio::spawn(signalk::signalk_task(state_tx, status_tx));

    let mut follow_boat = false;
    let mut key_shift 	= false;
    let mut is_zooming  = false;


    // gestion du mode drag
    let mut is_dragging = false;
    let mut drag_start = (0i32, 0i32);
    let (mut xdrag, mut ydrag) = (0,0);
    
    let mut dirty = true; // force le premier rendu
    let mut press: Option<Press> = None;

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
            let state = *state_rx.borrow_and_update();
	    boating.longitude = state.longitude;
	    boating.latitude  = state.latitude;
	    boating.course_over_ground = state.course_over_ground;
	    boating.speed_over_ground = state.speed_over_ground;
	    //println!("boatstate longitude : {} / latitude : {} / cos : {} / sog : {}",boating.longitude,boating.latitude,boating.course_over_ground,boating.speed_over_ground,);
	    dirty = true;
	    
            if !is_dragging 
		{

		boating.longitude 	   = state.longitude;
		boating.latitude  	   = state.latitude;
		boating.show(&renderer, &mut map);
		}
            if follow_boat 
		{
                map_longitude = state.longitude;
                map_latitude  = state.latitude;
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
			    (map_longitude,map_latitude) = renderer.screen_to_gps(1000,0);
			    
			    println!("Shift+Left détecté");
			} else {
			    // Left seul
			    (map_longitude,map_latitude) = renderer.screen_to_gps(100,0);
			    println!("Left seul");
			}
			dirty = true;
		    }

		// move map right
		Event::KeyDown { keycode: Some(Keycode::Right), keymod, .. } => {
			if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD) {
			    // Shift + Left
			    (map_longitude,map_latitude) = renderer.screen_to_gps(-1000,0);
			    println!("Shift+Left détecté");
			} else {
			    // Left seul
			    (map_longitude,map_latitude) = renderer.screen_to_gps(-100,0);
			    println!("Left seul");
			}
			dirty = true;
		    }

		// move map down
		Event::KeyDown { keycode: Some(Keycode::Down), keymod, .. } => {
			if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD) {
			    // Shift + Left
			    (map_longitude,map_latitude) = renderer.screen_to_gps(0,-500);
			    println!("Shift+Left détecté");
			} else {
			    // Left seul
			    (map_longitude,map_latitude) = renderer.screen_to_gps(0,-100);
			    println!("Left seul");
			}
			dirty = true;
		    }

		// move map up
		Event::KeyDown { keycode: Some(Keycode::Up), keymod, .. } => {
			if keymod.contains(Mod::LSHIFTMOD) || keymod.contains(Mod::RSHIFTMOD) {
			    // Shift + Left
			    (map_longitude,map_latitude) = renderer.screen_to_gps(0,500);
			    println!("Shift+Left détecté");
			} else {
			    // Left seul
			    (map_longitude,map_latitude) = renderer.screen_to_gps(0,100);
			    println!("Left seul");
			}
			dirty = true;
		    }

		// touche Shift mode Edit On/Off
		Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::LShift), .. } => {
		    println!("Shift on");
		    key_shift = true;
		    }
		Event::KeyUp { keycode: Some(sdl2::keyboard::Keycode::LShift), .. } => {
		    println!("Shift off");
		    key_shift = false;
		    }

                 Event::Quit { .. } => 
			{
			break 'main;
			}
		
		// Molette de la souris : zoom avant/arrière
		Event::MouseWheel { y, direction, mouse_x, mouse_y, .. } => {
		    let delta = match direction {
			sdl2::mouse::MouseWheelDirection::Flipped => -y.signum(),
			_ => y.signum(),
		    };
		    let dx = map.screen_width as i32 / 2 - mouse_x;
		    let dy = map.screen_height as i32 / 2 - mouse_y;
		    (map_longitude, map_latitude) = renderer.screen_to_gps( dx as i32, dy as i32);
		    zoom_index = (zoom_index + delta).clamp(0, dernier_index_valide);
		    zoom = zooms[zoom_index as usize];
		    println!("zoom : {}",zoom);
		    map.cursor_wait.set();
		    is_zooming = true;
		    dirty = true;
		}

		// zoom in (+)
		Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::KpPlus), .. } => {
		    zoom_index = (zoom_index + 1).clamp(0, dernier_index_valide);
		    zoom = zooms[zoom_index as usize];
		    println!("zoom : {}",zoom);
		    is_zooming = true;
		    dirty = true;
		    map.cursor_wait.set();
		    }

		 // zoom out (-)
		Event::KeyDown { keycode: Some(sdl2::keyboard::Keycode::KpMinus), .. } => {
		    zoom_index = (zoom_index - 1).clamp(0, dernier_index_valide);
		    zoom = zooms[zoom_index as usize];
		    println!("zoom : {}",zoom);
		    is_zooming = true;
		    dirty = true;
		    map.cursor_wait.set();
		    }
		    
		Event::MouseButtonDown { mouse_btn: MouseButton::Left, x, y, .. } => 
		    {
                    press = Some(Press {
                        start: Instant::now(),
                        x,
                        y,
                        moved: false,
                        long_fired: false,
                        //trace: Vec::new(),
			});
		    }

                Event::MouseMotion { x, y, xrel, yrel, .. } => 
		    {
		    if  menu.mouse(y as u32) 
			{ 
			map.cursor_arrow.set();
			}
		    else
			{
			if mode == Mode::Route
				{
				selected = routing.find_point_at(&renderer, (x, y), 8);
				if selected.is_some() 
				    {
				    println!("selected : {:?}", selected);
				    map.cursor_size.set();
				    }
				else{
				    map.cursor_cross.set();
				    }

				if key_shift  // shift enfoncé donc drag sue la carte
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
				else{	// shift non enfoncé donc deplacement Wp
				    if let Some(p) = press.as_mut() 
					{
					if !p.moved
					    {
					    p.moved = true;
					    }
					if p.moved  // deplacement du waypoint avec la souris
					    {
					    let dx = map.screen_width as i32 / 2 - x;
					    let dy = map.screen_height as i32 / 2 - y;
					    if let Some(i) = selected 
						{
						map.cursor_size.set();
						let (lon, lat) = renderer.screen_to_gps(dx , dy);
						routing.modify(i, lon, lat);
						dirty = true;
						println!("Deplace un point {:?}",selected);
						}
					    }
					}
				    }
				}
			    
			else	// on n'est pas en mode edit route
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
				(map_longitude,map_latitude) = renderer.screen_to_gps(dx,dy);
				dirty = true;
				is_dragging = false;
				}
			    else{
				 routing.save();
				}
			    } 
			else if !p.long_fired  // clic court
			    {
                            println!("Clic court ({:?})", p.start.elapsed());
			    match menu.event(x as u32,y as u32)
				{
				None =>  // la souris n'est pas dans la barre de menu
				    {
				    match mode 
					{
					Mode::Mesure =>
					    {
					    println!("Mesure clic");
					    let dx = map.screen_width as i32 / 2 - x;
					    let dy = map.screen_height as i32 / 2 - y;
					    mesuring.add_mark(&renderer,dx,dy);
					    dirty = true;
					    }
					Mode::Route =>
					    {
					    selected = routing.find_point_at(&renderer, (x, y), 8);
					    println!("selected {:?}",selected);
					    if selected == None
						{
						if let Some(k) = routing.find_midpoint_at(&renderer, (x, y), 8) // nouveau point entre k et k+1
							{
							let (longitude, latitude) = routing.midpoints()[k];
							routing.insert(k, longitude, latitude);     
							println!("Ajoute un point entre deux wp");
							}
						else 	{                      // nouveau point en fin de route
							let dx = map.screen_width as i32 / 2 - x;
							let dy = map.screen_height as i32 / 2 - y;
							let (longitude,latitude) = renderer.screen_to_gps(dx, dy);
							routing.add(longitude,latitude);
							println!("Ajoute un point à la fin");
							//println!("route {route:?}");
							}
						    
						    }
					    routing.save();
					    dirty = true;
					    }
					}
				    }
			    Some((topbar::SETUP, _state)) =>{
				println!("Setup");
				}
			    Some((topbar::EDIT_ROUTE, true)) =>{
				println!("Edit Route");
				mode = Mode::Route;
				dirty = true;
				}
			    Some((topbar::EDIT_ROUTE, false)) =>{
				println!("Edit Route");
				mode = Mode::Mesure;
				dirty = true;
				}
			    Some((topbar::CLEAR_ROUTE, _state)) =>{
				println!("Clear Route");
				routing.clear();
				dirty  = true;
				}
			    Some((topbar::FOLLOW, state)) =>{
				follow_boat = state;
				println!("Follow {}",state);
				dirty = true;
				}
			    Some((topbar::PREVIOUS, _state)) =>{
				nav.decrease_id();
				dirty = true;
				}
			    Some((topbar::NAV, state)) =>{
				nav.state(state);
				dirty = true;
				}
			    Some((topbar::NEXT, _state)) =>{
				nav.increase_id(routing.get_nb_wp());
				dirty = true;
				}
			    Some((topbar::QUITTE, _state)) =>{
				println!("Quitte");
				break 'main;
				}
			    _ =>{
				println!("else");
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
		
		match mode 
		    {
		    Mode::Route =>
			{
			println!("Delete Point");
			if let Some(i) = routing.find_point_at(&renderer, (p.x, p.y), 8) 
			    {
			    routing.remove(i);
			    routing.save();
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

	    let pixels = match follow_boat // mode fixed or follow boat
		{
		true =>
		    {
		    renderer.get_raw_rgba( boating.longitude,  boating.latitude, zoom)
		    }
		false =>
		    {
		    renderer.get_raw_rgba(map_longitude, map_latitude, zoom)
		    }
		};
             
            texture
                .update(None, &pixels, pitch)
                .map_err(|e| anyhow::anyhow!(e.to_string()))?;
            let _ = map.show_map(&texture)?;
	    
///////////////////// Affichage du bateau  ////////////////////////////////////
	    
	    boating.show( &renderer, &mut map);

////////////////// Affichage de la route  soit en mode normal ou mode nav /////////////////////////////////////////////////////
	    
	    if nav.get_state() 
		{
		routing.show_route_from_boat(&renderer, &mut map, mode,nav.get_id(), boating.position());
		}
	    else
		{
		routing.show_route_since_id(&renderer, &mut map, mode, 0);
		}
		
//////////////// Affichage de la mesure////////////////////////////////////////////////////

	     mesuring.show(&renderer, &mut map);
	     
/////////////////// topbar //////////////////////////////////////////////////

	    menu.show(&mut map.canvas);
	    
/////////////////////buttom bar soit en mode normal soit en mode nav /////////
	    let message = 
		if nav.get_state() 
		    {
		    let (dist_wp, bearing_wp, dist_total) = routing.calc_route_mn_from_boat(nav.get_id(), boating.position());
		    format!("Next Waypoint :  {:.1} Mn   {:.0}° / DTD : {:.1} Mn",dist_wp, bearing_wp, dist_total)
		    } 
		else 
		    {
		    routing.show_route_len()
		    };
	    buttom.show(&mut map.canvas,&message);
			
//////////////////////// Mode Nuit  ///////////////////////////////////////////

	    let _ = map.night_layer(0)?;

	    if is_zooming
		{
		map.cursor_cross.set();
		is_zooming = false;
		}
		
	    dirty = false;
	    }

        // Evite de saturer le CPU (pas de vsync ici a priori) : ~60 fps max.
        std::thread::sleep(Duration::from_millis(16));
    }

    Ok(())
}
