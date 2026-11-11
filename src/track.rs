use sdl2::pixels::Color;
use sdl2::rect::Point;
use sdl2::render::WindowCanvas;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;
use chrono::Local;

use crate::mbtiles_renderer::{project_norm, MBTilesRenderer};

const MIN_TRACK_DISTANCE_METERS: f64 = 20.0;
const SIMPLIFICATION_TOLERANCE_METERS: f64 = 20.0;
const MAX_SIMPLIFIED_SEGMENT_METERS: f64 = 1_500.0;
const EARTH_RADIUS_METERS: f64 = 6_371_000.0;

#[derive(Debug)]
struct TrackPoint {
    lon: f64,
    lat: f64,
    nx: f64,
    ny: f64,
}

pub struct Track {
    points: Vec<TrackPoint>,
	}

impl Track 
	{
    pub fn new(path: &Path) -> io::Result<Self>
		{
        let mut track = Self
			{
			points: Vec::new(), 
			};

		//let path = Path::new(file);
		match track.load(path)
			{
			Ok(()) => println!("Trace chargée : {} / {} points", path.display(), track.points.len()),
			Err(e) if e.kind() == io::ErrorKind::NotFound =>
				println!("Aucune trace par défaut trouvée : {}", path.display()),
			Err(e) if e.kind() == io::ErrorKind::InvalidData =>
				eprintln!(
					"Trace GPX invalide ({}), démarrage avec une trace vierge : {e}",
					path.display()
				),
			Err(e) => return Err(e),
			}

		Ok(track)
		}

    
    pub fn reset(&mut self) 
		{
		self.points.clear();
		self.points.shrink_to_fit();
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
		Ok(())
		}
		
	fn push_point(&mut self, pos: (f64, f64)) 
		{
		let (nx, ny) = project_norm(pos.0, pos.1);
		self.points.push(TrackPoint { lon: pos.0, lat: pos.1, nx, ny });
		}
    /// pos = (lon, lat)
	pub fn add(&mut self, pos: (f64, f64)) 
		{
		if !pos.0.is_finite()
			|| !pos.1.is_finite()
			|| !(-180.0..=180.0).contains(&pos.0)
			|| !(-90.0..=90.0).contains(&pos.1)
			{
			return;
			}
		if let Some(last) = self.points.last() {
			if distance_meters((last.lon, last.lat), pos) < MIN_TRACK_DISTANCE_METERS {
				return;
			}
		}
		self.push_point(pos);
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

		if let Some((lon, lat)) = boat
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
        for index in simplified_indices(&self.points)
			{
			let p = &self.points[index];
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

fn distance_meters(a: (f64, f64), b: (f64, f64)) -> f64 {
	let (lat1, lat2) = (a.1.to_radians(), b.1.to_radians());
	let delta_lat = lat2 - lat1;
	let delta_lon = (b.0 - a.0).to_radians();
	let haversine = (delta_lat / 2.0).sin().powi(2)
		+ lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
	2.0 * EARTH_RADIUS_METERS * haversine.sqrt().asin()
}

fn simplified_indices(points: &[TrackPoint]) -> Vec<usize> {
	if points.len() <= 2 {
		return (0..points.len()).collect();
	}

	let mut retained = vec![false; points.len()];
	retained[0] = true;
	retained[points.len() - 1] = true;
	let mut segments = vec![(0, points.len() - 1)];

	while let Some((start, end)) = segments.pop() {
		if end <= start + 1 {
			continue;
		}

		let a = (points[start].lon, points[start].lat);
		let c = (points[end].lon, points[end].lat);
		let mut farthest = None;
		let mut max_deviation = 0.0;

		for index in start + 1..end {
			let point = (points[index].lon, points[index].lat);
			let deviation = distance_to_segment_meters(a, point, c);
			if deviation > max_deviation {
				max_deviation = deviation;
				farthest = Some(index);
			}
		}

		let split = if max_deviation > SIMPLIFICATION_TOLERANCE_METERS {
			farthest
		} else if distance_meters(a, c) > MAX_SIMPLIFIED_SEGMENT_METERS {
			Some(midpoint_by_distance(points, start, end))
		} else {
			None
		};

		if let Some(index) = split {
			retained[index] = true;
			segments.push((start, index));
			segments.push((index, end));
		}
	}

	retained
		.into_iter()
		.enumerate()
		.filter_map(|(index, keep)| keep.then_some(index))
		.collect()
}

fn midpoint_by_distance(points: &[TrackPoint], start: usize, end: usize) -> usize {
	let distances: Vec<f64> = (start..end)
		.map(|index| {
			distance_meters(
				(points[index].lon, points[index].lat),
				(points[index + 1].lon, points[index + 1].lat),
			)
		})
		.collect();
	let half_distance = distances.iter().sum::<f64>() / 2.0;
	let mut accumulated = 0.0;

	for (offset, distance) in distances.into_iter().enumerate() {
		accumulated += distance;
		if accumulated >= half_distance {
			return start + offset + 1;
		}
	}

	start + (end - start) / 2
}

fn distance_to_segment_meters(
	start: (f64, f64),
	point: (f64, f64),
	end: (f64, f64),
) -> f64 {
	let lat_origin = start.1.to_radians();
	let local = |position: (f64, f64)| {
		let delta_lon = (position.0 - start.0 + 180.0).rem_euclid(360.0) - 180.0;
		(
			delta_lon.to_radians() * lat_origin.cos() * EARTH_RADIUS_METERS,
			(position.1 - start.1).to_radians() * EARTH_RADIUS_METERS,
		)
	};

	let (px, py) = local(point);
	let (ex, ey) = local(end);
	let length_squared = ex * ex + ey * ey;
	if length_squared == 0.0 {
		return px.hypot(py);
	}

	let projection = ((px * ex + py * ey) / length_squared).clamp(0.0, 1.0);
	(px - projection * ex).hypot(py - projection * ey)
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

        let track = Track::new(&path).unwrap();
        std::fs::remove_file(path).unwrap();

        assert_eq!(track.points.len(), 2);
        assert_eq!((track.points[0].lon, track.points[0].lat), (-122.3, 47.5));
        assert_eq!((track.points[1].lon, track.points[1].lat), (-122.2, 47.6));
    }

    #[test]
    fn starts_with_empty_track_when_gpx_has_no_points() {
        let unique_id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "desktop_plotter_track_empty_{}_{}.gpx",
            std::process::id(),
            unique_id
        ));
        std::fs::write(&path, "<gpx><trk><trkseg></trkseg></trk></gpx>").unwrap();

        let track = Track::new(&path).unwrap();
        std::fs::remove_file(path).unwrap();

        assert!(track.points.is_empty());
    }

    #[test]
    fn filters_positions_by_distance_from_last_recorded_point() {
        let mut track = Track::new(Path::new("missing-track.gpx")).unwrap();
        let start = (-122.3, 47.5);
        let under_20m = (-122.3, 47.5001);
        let over_20m = (-122.3, 47.5003);

        track.add(start);
        track.add(under_20m);
        assert_eq!(track.points.len(), 1);

        track.add(over_20m);
        assert_eq!(track.points.len(), 2);
        assert_eq!(
            (track.points[1].lon, track.points[1].lat),
            over_20m
        );
    }

	#[test]
	fn simplifies_straight_track_but_preserves_turns() {
		let straight = [
			(0.0, 0.0),
			(0.001, 0.0),
			(0.002, 0.0),
			(0.003, 0.0),
		]
		.into_iter()
		.map(|(lon, lat)| {
			let (nx, ny) = project_norm(lon, lat);
			TrackPoint { lon, lat, nx, ny }
		})
		.collect::<Vec<_>>();
		assert_eq!(simplified_indices(&straight), vec![0, 3]);

		let turning = [
			(0.0, 0.0),
			(0.001, 0.0),
			(0.001, 0.001),
			(0.002, 0.001),
			(0.002, 0.002),
		]
			.into_iter()
			.map(|(lon, lat)| {
				let (nx, ny) = project_norm(lon, lat);
				TrackPoint { lon, lat, nx, ny }
			})
			.collect::<Vec<_>>();
		let retained = simplified_indices(&turning);
		assert!(retained.contains(&1));
		assert!(retained.contains(&2));
		assert!(retained.contains(&3));
		assert_eq!(retained.first(), Some(&0));
		assert_eq!(retained.last(), Some(&4));
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

        let mut track = Track::new(Path::new("missing-track.gpx")).unwrap();
        track.add((-122.3, 47.5));
        track.save(&path).unwrap();

        assert!(path.is_file());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
