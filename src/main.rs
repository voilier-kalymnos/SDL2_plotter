// Install SDL2 driver
// sudo apt update && sudo apt install -y libsdl2-2.0-0
// sudo apt install -y libsdl2-ttf-2.0-0 libsdl2-image-2.0-0 libsdl2-mixer-2.0-0
// sudo apt install libdrm-dev libgbm-dev libegl1-mesa-dev
// pkill -9 plotter
// sudo apt install libsdl2-gfx-1.0-0

mod action;
mod ais;
mod alarm;
mod app;
mod boat;
mod buttombar;
mod catalogue;
mod collision;
mod config;
mod diagnostics;
mod feed;
mod geo_data;
mod gps;
mod gpx;
mod magnetic;
mod mbtiles_renderer;
mod mesure;
mod nmea0183;
mod overlay;
mod route;
mod screen;
mod signalk;
mod state;
mod stream;
mod topbar;
mod track;
mod waypoints;

const DEFAULT_PLOTTER: &str = "default/plotter.toml";

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
        crate::diagnostics::record_panic(&panic.to_string());
        default_panic_hook(panic);
    }));
    diagnostics::record(
        "INFO",
        format!(
            "Démarrage de plotter (pid={}, cwd={:?})",
            std::process::id(),
            std::env::current_dir()
        ),
    );
    let result = run().await;
    if let Err(error) = &result {
        diagnostics::record("ERROR", format!("{error:#}"));
    }
    result
}

async fn run() -> anyhow::Result<()> {
    let config = config::AppConfig::load_or_create(std::path::Path::new(DEFAULT_PLOTTER))?;
    let mut sdl = screen::MoteurSdl::init().map_err(anyhow::Error::msg)?;
    diagnostics::record("INFO", "SDL initialisé");
    let mut screen = screen::Screen::new(
        &sdl.ttf_context,
        &sdl.context,
        &config.window,
        &config.fonts,
        &config.appearance,
    )
    .map_err(anyhow::Error::msg)?;
    diagnostics::record("INFO", "Fenêtre SDL initialisée");

    let texture_creator = screen.canvas.texture_creator();
    let mut texture =
        app::chart_texture(&texture_creator, screen.screen_width, screen.screen_height)
            .map_err(anyhow::Error::msg)?;
    let streamer = stream::Streamer::start_or_disable(&config.streaming);
    let alarm_sound = match sdl.context.audio() {
        Ok(audio) => match alarm::AlarmSound::new(&audio) {
            Ok(sound) => Some(sound),
            Err(error) => {
                diagnostics::record("WARN", format!("Alarme sonore indisponible: {error}"));
                None
            }
        },
        Err(error) => {
            diagnostics::record(
                "WARN",
                format!("Sous-système audio SDL indisponible: {error}"),
            );
            None
        }
    };
    let mut plotter = app::Plotter::new(
        &sdl.context,
        &sdl.ttf_context,
        screen.screen_width,
        screen.screen_height,
        &config,
        streamer,
        alarm_sound,
        app::Files::default(),
    )?;
    let today = magnetic::decimal_year(chrono::Utc::now().date_naive());
    if let Some(warning) = magnetic::start_warning(today) {
        diagnostics::record("WARN", warning);
    }
    diagnostics::record("INFO", "Application initialisée");
    plotter.main_loop(
        &mut sdl.event_pump,
        &mut screen,
        &texture_creator,
        &mut texture,
    )
}
