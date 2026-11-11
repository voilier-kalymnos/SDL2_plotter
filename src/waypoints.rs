use sdl2::gfx::primitives::DrawRenderer;
use serde::{Deserialize, Serialize};
use std::{fs, io, path::Path};

use crate::mbtiles_renderer::MBTilesRenderer;
use crate::screen::Map;

const WAYPOINTS_FILE: &str = "waypoints";

#[derive(Serialize, Deserialize)]
struct Waypoint {
    lon: f64,
    lat: f64,
    #[serde(default)]
    name: String,
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

    pub fn add(&mut self, pos: (f64, f64), name: String) {
        
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
            name,
        });
    }

    pub fn modify(&mut self, i:usize, pos: (f64, f64)) {
        let (longitude, latitude) = pos;
        if let Some(waypoint) = self.waypoints.get_mut(i) {
            waypoint.lon = longitude;
            waypoint.lat = latitude;
        }
    }

	pub fn find_point_at(
		&self,
		renderer: &MBTilesRenderer,
		mouse: (i32, i32),
		radius: i32,
		) -> Option<usize> {
			let r2 = (radius as i64) * (radius as i64);
			self.waypoints
				.iter()
				.enumerate()
				.map(|(i, waypoint)| {
					let pt = renderer.gps_to_point((waypoint.lon, waypoint.lat));
					let dx = pt.x as i64 - mouse.0 as i64;
					let dy = pt.y as i64 - mouse.1 as i64;
					(i, dx * dx + dy * dy)
				})
				.filter(|&(_, d2)| d2 <= r2)
				.min_by_key(|&(_, d2)| d2)
				.map(|(i, _)| i)
				}

    pub fn remove(&mut self, i: usize) {
        self.waypoints.remove(i);
    }

    pub fn show(&self, map: &mut Map<'_>, renderer: &MBTilesRenderer) -> Result<(), String> {
        for (i, wp) in self.waypoints.iter().enumerate() {
            let point = renderer.gps_to_point((wp.lon, wp.lat));
            let x = point.x as i16;
            let y = point.y as i16;
            let color = sdl2::pixels::Color::RGB(220, 30, 30);
            map.canvas.filled_trigon(x - 5, y - 9, x + 5, y - 9, x, y, color)?;
            map.canvas.filled_circle(x, y - 11, 6, color)?;
            map.canvas
                .filled_circle(x, y - 11, 2, sdl2::pixels::Color::WHITE)?;
            let label = if wp.name.is_empty() {
                format!("Waypoint {}", i + 1)
            } else {
                wp.name.clone()
            };
            map.show_text(&label, point.x + 10, point.y - 12)?;
        }
        Ok(())
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
        original.add((178.1, -17.7), "Mouillage".to_string());

        original.save_to(&path).unwrap();
        let mut loaded = Waypoints {
            waypoints: Vec::new(),
        };
        loaded.load_from(&path).unwrap();

        assert_eq!(loaded.waypoints.len(), 1);
        assert_eq!(loaded.waypoints[0].lon, 178.1);
        assert_eq!(loaded.waypoints[0].lat, -17.7);
        assert_eq!(loaded.waypoints[0].name, "Mouillage");

        fs::remove_file(path).unwrap();
    }

    #[test]
    fn loads_waypoints_without_names() {
        let path =
            std::env::temp_dir().join(format!("plotter-waypoints-legacy-{}.json", std::process::id()));
        fs::write(&path, r#"[{"lon":178.1,"lat":-17.7}]"#).unwrap();
        let mut loaded = Waypoints {
            waypoints: Vec::new(),
        };

        loaded.load_from(&path).unwrap();

        assert_eq!(loaded.waypoints.len(), 1);
        assert_eq!(loaded.waypoints[0].name, "");
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
