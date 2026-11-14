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
mod app;
mod diagnostics;

use sdl2::pixels::PixelFormatEnum;

const MAP_WORLD: &str = "assets/OSM.mbtiles";

#[derive(Debug, PartialEq, Clone, Copy, Default)]
pub enum Mode {
	EditRoute,
	EditWaypoint,
	#[default]
	Measurement,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
	if let Err(error) = diagnostics::init(std::path::Path::new("plotter.log")) {
		eprintln!("Impossible d'ouvrir plotter.log : {error}");
	}
	let default_panic_hook = std::panic::take_hook();
	std::panic::set_hook(Box::new(move |panic| {
		crate::diagnostics::record("PANIC", panic);
		default_panic_hook(panic);
	}));
	diagnostics::record(
		"INFO",
		format!("Démarrage de plotter (pid={}, cwd={:?})", std::process::id(), std::env::current_dir()),
	);
	let result = run().await;
	if let Err(error) = &result {
		diagnostics::record("ERROR", format!("{error:#}"));
	}
	result
}

async fn run() -> anyhow::Result<()> {
	let mut sdl = screen::MoteurSdl::init().map_err(anyhow::Error::msg)?;
	diagnostics::record("INFO", "SDL initialisé");
	let mut map = screen::Map::new(&sdl.ttf_context, &sdl.context).map_err(anyhow::Error::msg)?;
	diagnostics::record("INFO", "Fenêtre SDL initialisée");

	let texture_creator = map.canvas.texture_creator();
	let mut texture = texture_creator
		.create_texture_streaming(PixelFormatEnum::RGBA32, map.screen_width, map.screen_height)
		.map_err(anyhow::Error::msg)?;
	let pitch = (map.screen_width * 4) as usize;

	let mut plotter =
		app::Plotter::new(&sdl.context, &sdl.ttf_context, map.screen_width, map.screen_height)?;
	diagnostics::record("INFO", "Application initialisée");
	plotter.main_loop(&mut sdl.event_pump, &mut map, &mut texture, pitch)
}
