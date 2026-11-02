use sdl2::pixels::Color;
use sdl2::rect::Point;
use sdl2::render::WindowCanvas;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use chrono::Local;

use crate::mbtiles_renderer::{project_norm, MBTilesRenderer};

const EARTH_R: f64 = 6_371_000.0;


#[derive(Debug)]
struct TrackPoint {
    lon: f64,
    lat: f64,
    nx: f64,
    ny: f64,
}

pub struct Track {
    points: Vec<TrackPoint>,
    min_distance: f64, // mètres
    pending: Option<(f64, f64)>, 
    tolerance: f64, 
    max_gap: f64,
	}

impl Track 
	{
    pub fn new(min_distance: f64, tolerance: f64, max_gap: f64, path: &Path) -> io::Result<Self>
		{
        let mut track = Self
			{
			points: Vec::new(), 
			min_distance,
			tolerance,
			max_gap,
			pending: None,
			};

			//let path = Path::new(file);
			match track.load(path)
				{
				Ok(()) => println!("Trace chargée : {} / {} points", path.display(), track.points.len()),
				Err(e) if e.kind() == io::ErrorKind::NotFound =>
					println!("Aucune trace par défaut trouvée : {}", path.display()),
				Err(e) => return Err(e),
				}

			Ok(track)
			}

    
    pub fn reset(&mut self) 
		{
		self.points.clear();
		self.points.shrink_to_fit();
		self.pending = None;
		}

	pub fn load(&mut self, path: &Path) -> io::Result<()>
		{
		let text = std::fs::read_to_string(path)?;
		let mut points = Vec::new();

		for chunk in text.split('<').skip(1)
			{
			let tag = chunk.split('>').next().unwrap_or("").trim();
			if !is_track_point_tag(tag)
				{
				continue;
				}

			let lat = parse_gpx_attr(tag, "lat")
				.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "latitude GPX manquante ou invalide"))?;
			let lon = parse_gpx_attr(tag, "lon")
				.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "longitude GPX manquante ou invalide"))?;
			if !lat.is_finite() || !(-90.0..=90.0).contains(&lat)
				{
				return Err(io::Error::new(io::ErrorKind::InvalidData, "latitude GPX hors limites"));
				}
			if !lon.is_finite() || !(-180.0..=180.0).contains(&lon)
				{
				return Err(io::Error::new(io::ErrorKind::InvalidData, "longitude GPX hors limites"));
				}

			let (nx, ny) = project_norm(lon, lat);
			points.push(TrackPoint { lon, lat, nx, ny });
			}

		if points.is_empty()
			{
			return Err(io::Error::new(io::ErrorKind::InvalidData, "aucun point de trace GPX"));
			}

		self.points = points;
		self.pending = None;
		Ok(())
		}

	pub fn flush(&mut self) 
		{
		if let Some(p) = self.pending.take() 
			{
			self.push_point(p);
			}
		}
		
	fn push_point(&mut self, pos: (f64, f64)) 
		{
		let (nx, ny) = project_norm(pos.0, pos.1);
		self.points.push(TrackPoint { lon: pos.0, lat: pos.1, nx, ny });
		}
    /// pos = (lon, lat)
	pub fn add(&mut self, pos: (f64, f64)) 
		{
		// Premier point : on le garde toujours
		let Some(last) = self.points.last() else 
			{
			self.push_point(pos);
			self.pending = None;
			return;
			};
		let last_pos = (last.lon, last.lat);

		// Anti-bruit GPS (petit seuil, ex. 20 m)
		if haversine(last_pos, pos) < self.min_distance 
			{
			return;
			}
		if let Some(p) = self.pending 
				{
				let dev = deviation(last_pos, p, pos);
				// On garde p si changement de cap, ou si le segment devient trop long
				if dev > self.tolerance || haversine(last_pos, p) > self.max_gap 
					{
					self.push_point(p);
					}
				}
			self.pending = Some(pos);
		}
		
    pub fn show(&self, canvas: &mut WindowCanvas, map: &MBTilesRenderer, boat: Option<(f64, f64)>) 
		{
		if self.points.is_empty() 
			{
			return;
			}

		let mut pts: Vec<Point> = self.points
			.iter()
			.map(|p| map.norm_to_point(p.nx, p.ny))
			.collect();

		// Dernier segment : du dernier point gardé jusqu'à la position en attente
		if let Some((lon, lat)) = boat.or(self.pending) 
			{
			let (nx, ny) = project_norm(lon, lat);
			pts.push(map.norm_to_point(nx, ny));
			}

		if pts.len() < 2 
			{
			return;
			}

		canvas.set_draw_color(Color::RGB(255, 0, 0));
		let _ = canvas.draw_lines(pts.as_slice());
		}
	
	// Sauvegarde la trace dans le fichier indiqué, en créant son dossier parent au besoin.
    pub fn save(&mut self, path: &Path) -> io::Result<()>
		{
		if let Some(parent) = path.parent().filter(|parent| !parent.as_os_str().is_empty())
			{
			std::fs::create_dir_all(parent)?;
			}
        self.flush(); // inclut le dernier point en attente

        if self.points.is_empty() 
			{
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "trace vide"));
			}

        let now = Local::now();

        let mut w = BufWriter::new(File::create(&path)?);
        writeln!(w, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
        writeln!(w, r#"<gpx version="1.1" creator="boat-tracker" xmlns="http://www.topografix.com/GPX/1/1">"#)?;
        writeln!(w, "  <trk>")?;
        writeln!(w, "    <name>Track {}</name>", now.format("%Y-%m-%d %H:%M:%S"))?;
        writeln!(w, "    <trkseg>")?;
        for p in &self.points 
			{
            writeln!(w, r#"      <trkpt lat="{:.6}" lon="{:.6}"/>"#, p.lat, p.lon)?;
			}
        writeln!(w, "    </trkseg>")?;
        writeln!(w, "  </trk>")?;
        writeln!(w, "</gpx>")?;
        w.flush()?;

        Ok(())
        }
	}

fn is_track_point_tag(tag: &str) -> bool
	{
	let Some(rest) = tag.strip_prefix("trkpt") else
        {
        return false;
        };
	rest.is_empty() || rest.starts_with('/') || rest.chars().next().is_some_and(char::is_whitespace)
	}

fn parse_gpx_attr(tag: &str, name: &str) -> Option<f64>
	{
	let key = format!("{name}=");
	let start = tag.find(&key)? + key.len();
	let value = tag[start..].trim_start();
	let quote = value.chars().next()?;
	if quote != '"' && quote != '\''
        {
        return None;
        }
	let value = &value[quote.len_utf8()..];
	let end = value.find(quote)?;
	value[..end].parse().ok()
	}
	
/// a et b = (lon, lat) en degrés ; résultat en mètres
fn haversine(a: (f64, f64), b: (f64, f64)) -> f64 
	{
    const R: f64 = 6_371_000.0;
    let (lat1, lat2) = (a.1.to_radians(), b.1.to_radians());
    let dlat = lat2 - lat1;
    let dlon = (b.0 - a.0).to_radians();
    let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    2.0 * R * h.sqrt().asin()
	}


/// Projection locale équirectangulaire en mètres, centrée sur `origin`
fn to_local(origin: (f64, f64), p: (f64, f64)) -> (f64, f64) {
    let lat0 = origin.1.to_radians();
    let x = (p.0 - origin.0).to_radians() * lat0.cos() * EARTH_R;
    let y = (p.1 - origin.1).to_radians() * EARTH_R;
    (x, y)
}

/// Distance (m) du point b au segment a-c
fn deviation(a: (f64, f64), b: (f64, f64), c: (f64, f64)) -> f64 {
    let (bx, by) = to_local(a, b);
    let (cx, cy) = to_local(a, c);
    let len2 = cx * cx + cy * cy;
    if len2 < 1e-9 {
        return (bx * bx + by * by).sqrt();
    }
    let t = ((bx * cx + by * cy) / len2).clamp(0.0, 1.0);
    let (dx, dy) = (bx - t * cx, by - t * cy);
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn loads_track_points_from_gpx() {
        let unique_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "desktop_plotter_track_load_{}_{}.gpx",
            std::process::id(),
            unique_id
        ));
        std::fs::write(
            &path,
            r#"<gpx><trk><trkseg><trkpt lat="47.5" lon="-122.3"/><trkpt lon="-122.2" lat="47.6"/></trkseg></trk></gpx>"#,
        )
        .unwrap();

        let track = Track::new(20.0, 20.0, 1500.0, &path).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(track.points.len(), 2);
        assert_eq!((track.points[0].lon, track.points[0].lat), (-122.3, 47.5));
        assert_eq!((track.points[1].lon, track.points[1].lat), (-122.2, 47.6));
        assert!(track.pending.is_none());
    }

    #[test]
    fn save_creates_parent_directories() {
        let unique_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "desktop_plotter_track_save_{}_{}",
            std::process::id(),
            unique_id
        ));
        let path = dir.join("track").join("default.gpx");

        let mut track = Track::new(20.0, 20.0, 1500.0, Path::new("missing-track.gpx")).unwrap();
        track.start();
        track.add((-122.3, 47.5));
        track.save(&path).unwrap();

        assert!(path.is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
