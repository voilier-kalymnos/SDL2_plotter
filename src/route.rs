mod gpx;
 
use crate::mbtiles_renderer::MBTilesRenderer;
use crate::screen::Map;
use crate::gps;
use crate::state::Mode;
use sdl2::rect::Point;
 
 #[derive(Debug)]
 pub struct Route
	{
	track: Vec<(f64, f64)>,
	distance: Vec<f64>,
	restante: Vec<f64>,
	longueur :f64,
	}
	
impl Route
	{
	pub fn new() -> Result<Self, String> 
		{
		let track: Vec<(f64, f64)> = match gpx::load_gpx("route.gpx") 
			{
			Ok(r) => 
				{
				println!("route chargée : {} points", r.len());
				r
				}
			Err(e) => 
				{
				eprintln!("pas de route chargée ({e}), départ avec une route vide");
				Vec::new()
				}
			};

		let mut route = Route 
			{
			track,
			distance: Vec::new(),
			restante: Vec::new(),
			longueur: 0.0,
			};
		route.calc_route();
		Ok(route)
		}
	pub fn show_route_since_id(&mut self,renderer: &MBTilesRenderer,map: &mut Map,mode: Mode,id: usize)
		{
		let rte = &self.track[id..];
		if rte.len() >= 1 {
		    let pts: Vec<Point> = rte
						.iter()
						.map(|&pos| renderer.gps_to_point(pos))
						.collect();
		    map.show_route(&pts);
		    }
	     if mode == Mode::Edit_Route
			{
			 if rte.len() >= 2 
				{
				let mid_points = self.midpoints();
				let mid_pts: Vec<Point> = mid_points[id..]
						.iter()
						.map(|&pos| renderer.gps_to_point(pos))
						.collect();
				map.draw_mid_handles(&mid_pts);
				}
			}
		}
	pub fn show_route_from_boat(&mut self,renderer: &MBTilesRenderer,map: &mut Map, mode: Mode,id: usize,boat: (f64,f64))
		{
		map.show_route(&[ 
						renderer.gps_to_point(boat),
						renderer.gps_to_point(self.track[id])
						]);
		self.show_route_since_id(renderer, map,mode, id);
		}
	
	pub fn show_route_len(&self) -> String
		{
		format!("route : {:.1} Mn", self.longueur)
		}
			
	pub fn save(&mut self) 
		{
		match gpx::save_gpx("route.gpx", "Plotter", &self.track) {
						Ok(())  => println!("route sauvée : {} points", self.track.len()),
						Err(e)  => eprintln!("erreur de sauvegarde : {e}"),
						}
		}
		
	pub fn clear(&mut self)
		{
		self.track.clear();
		self.longueur = 0.0;
		self.save();
		}
		
	pub fn remove(&mut self, i:usize) 
		{
		self.track.remove(i);
		self.calc_route();
		}
		
	pub fn modify(&mut self, i:usize, longitude:f64, latitude:f64) 
		{
		self.track[i] = (longitude, latitude);
		self.calc_route();
		}
		
	pub fn add(&mut self, longitude:f64, latitude:f64) 
		{
		self.track.push((longitude,latitude));
		self.calc_route();
		}
		
	pub fn insert (&mut self, i:usize, longitude:f64, latitude:f64) 
		{
		self.track.insert(i+1,(longitude,latitude));
		self.calc_route();
		}
		
	pub fn calc_route(&mut self) 
		{
		self.distance.clear();

		for w in self.track.windows(2) 
			{
			let (lon1, lat1) = w[0];
			let (lon2, lat2) = w[1];
			let d = gps::distance_wp(Some((lat1, lon1)), Some((lat2, lon2)));
			self.distance.push(d);
			}

		self.longueur = self.distance.iter().sum();
		
		self.restante = vec![0.0; self.track.len()];
		let mut reste = 0.0;
		for i in (0..self.distance.len()).rev() 
			{
			reste += self.distance[i];
			self.restante[i] = reste;
			}
		}
		
	pub fn calc_route_mn_from_boat(&mut self, id :usize, boat :(f64,f64)) -> (f64,f64,f64)
		{
		let (dist_wp, bearing_wp) = gps::distance_bearing_wp(boat, self.track[id]);
		let dist_total = dist_wp + self.restante[id];
		(dist_wp, bearing_wp, dist_total)
		}
		
	pub fn find_point_at(
		&mut self,
		renderer: &MBTilesRenderer,
		mouse: (i32, i32),
		radius: i32,
		) -> Option<usize> {
			let r2 = (radius as i64) * (radius as i64);
			self.track
				.iter()
				.enumerate()
				.map(|(i, &(lon, lat))| {
					let pt = renderer.gps_to_point((lon, lat));
					let dx = pt.x as i64 - mouse.0 as i64;
					let dy = pt.y as i64 - mouse.1 as i64;
					(i, dx * dx + dy * dy)
				})
				.filter(|&(_, d2)| d2 <= r2)
				.min_by_key(|&(_, d2)| d2)
				.map(|(i, _)| i)
				}
	
	pub fn find_midpoint_at(
		&mut self,
		renderer: &MBTilesRenderer,
		mouse: (i32, i32),
		radius: i32,
		) -> Option<usize> {
			let r2 = (radius as i64) * (radius as i64);
			self.midpoints()
				.iter()
				.enumerate()
				.map(|(i, &(lon, lat))| {
					let pt = renderer.gps_to_point((lon, lat));
					let dx = pt.x as i64 - mouse.0 as i64;
					let dy = pt.y as i64 - mouse.1 as i64;
					(i, dx * dx + dy * dy)
				})
				.filter(|&(_, d2)| d2 <= r2)
				.min_by_key(|&(_, d2)| d2)
				.map(|(i, _)| i)
				}
				
	pub fn midpoints(&mut self) -> Vec<(f64, f64)> {
		self.track
			.windows(2)
			.map(|w| ((w[0].0 + w[1].0) / 2.0, (w[0].1 + w[1].1) / 2.0))
			.collect()
		}
		/*
	pub fn get_wp(&self, id: usize) -> Option<(f64, f64)> 
		{
		let last = self.track.len().checked_sub(1)?; // None si le Vec est vide
		Some(self.track[id.min(last)])
		}
	*/
	pub fn get_nb_wp(&self) -> usize
		{
		self.track.len()
		}
	}

