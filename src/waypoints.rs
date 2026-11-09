use sdl2::pixels::Color;
use sdl2::render::WindowCanvas;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

use crate::mbtiles_renderer::MBTilesRenderer;

const WAYPOINTS_FILE: &str = "waypoints";

#[derive(Serialize, Deserialize)]
struct Waypoint {
    lon: f64,
    lat: f64,
}

pub struct Waypoints {
    waypoints: Vec<Waypoint>,
}

impl Waypoints {
    pub fn new() -> io::Result<Self> {
        let mut waypoints = Self {
            waypoints: Vec::new(),
        };
        match waypoints.load() {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        Ok(waypoints)
    }

    pub fn add(&mut self, pos: (f64, f64)) {
        
        let (longitude, latitude) = pos;
        if !longitude.is_finite()
			|| !latitude.is_finite()
			|| !(-180.0..=180.0).contains(&longitude)
			|| !(-90.0..=90.0).contains(&latitude)
			{
			return;
			}
        self.waypoints.push(Waypoint {
            lon: longitude,
            lat: latitude,
        });
    }

    pub fn modify(&mut self, i:usize, pos: (f64, f64)) {
        let (longitude, latitude) = pos;
        if i < self.waypoints.len() {
            self.waypoints[i] = Waypoint { lon: longitude, lat: latitude };
        }
    }

    pub fn remove(&mut self, i: usize) {
        self.waypoints.remove(i);
    }

    pub fn show(&self, canvas: &mut WindowCanvas, renderer: &MBTilesRenderer) {
        for wp in &self.waypoints {
            let point = renderer.gps_to_point((wp.lon, wp.lat));
            canvas.set_draw_color(Color::RGB(255, 0, 0));
            let _ = canvas.fill_rect(sdl2::rect::Rect::new(point.x - 3, point.y - 3, 6, 6));
        }
    }

    pub fn reset(&mut self) {
        self.waypoints.clear();
        self.waypoints.shrink_to_fit();
    }

    pub fn load(&mut self) -> io::Result<()> {
        self.load_from(Path::new(WAYPOINTS_FILE))
    }

    fn load_from(&mut self, path: &Path) -> io::Result<()> {
        let json = fs::read_to_string(path)?;
        let waypoints = serde_json::from_str(&json)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        self.waypoints = waypoints;
        Ok(())
    }

    pub fn save(&self) -> io::Result<()> {
        self.save_to(Path::new(WAYPOINTS_FILE))
    }

    fn save_to(&self, path: &Path) -> io::Result<()> {
        let json = serde_json::to_string_pretty(&self.waypoints)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        fs::write(path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_and_load_waypoints() {
        let path =
            std::env::temp_dir().join(format!("plotter-waypoints-{}.json", std::process::id()));
        let mut original = Waypoints {
            waypoints: Vec::new(),
        };
        original.add((178.1, -17.7));

        original.save_to(&path).unwrap();
        let mut loaded = Waypoints {
            waypoints: Vec::new(),
        };
        loaded.load_from(&path).unwrap();

        assert_eq!(loaded.waypoints.len(), 1);
        assert_eq!(loaded.waypoints[0].lon, 178.1);
        assert_eq!(loaded.waypoints[0].lat, -17.7);

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_file_is_a_valid_empty_startup() {
        let path = std::env::temp_dir().join(format!(
            "plotter-waypoints-missing-{}.json",
            std::process::id()
        ));
        let mut waypoints = Waypoints {
            waypoints: Vec::new(),
        };

        assert_eq!(
            waypoints.load_from(&path).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }
}
