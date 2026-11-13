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
	let mut sdl = screen::MoteurSdl::init().map_err(anyhow::Error::msg)?;
	let mut map = screen::Map::new(&sdl.ttf_context, &sdl.context).map_err(anyhow::Error::msg)?;

	let texture_creator = map.canvas.texture_creator();
	let mut texture = texture_creator
		.create_texture_streaming(PixelFormatEnum::RGBA32, map.screen_width, map.screen_height)
		.map_err(anyhow::Error::msg)?;
	let pitch = (map.screen_width * 4) as usize;

	let mut plotter =
		app::Plotter::new(&sdl.context, &sdl.ttf_context, map.screen_width, map.screen_height)?;
	plotter.main_loop(&mut sdl.event_pump, &mut map, &mut texture, pitch)
}
