use crate::mbtiles_renderer::MBTilesRenderer;
use crate::screen::Map;

pub struct Mesure
	{
	mark1: Option<(f64, f64)>, 
    mark2: Option<(f64, f64)>,
    distance: f64,
    bearing: f64,
	}
	
impl Mesure
	{
	pub fn new()-> Result<Self, String>
		{

		Ok(Mesure{
				mark1: None, 
				mark2: None,
				distance: 0.0,
				bearing: 0.0 
				})
		}

	pub fn add_mark(&mut self, renderer: &MBTilesRenderer, dx: i32 , dy: i32)
		{
		match (self.mark1, self.mark2) {
					// Étape 1 : rien de posé -> premier point
					(None, _) => {
					    self.mark1 = Some(renderer.screen_to_gps(dx, dy));
					    self.mark2 = None;
					    (self.distance, self.bearing) = (0.0, 0.0);
						}
					// Étape 2 : premier point posé -> deuxième point
					(Some((lon1, lat1)), None) => {
					    let (lon2, lat2) = renderer.screen_to_gps(dx, dy);
					    self.mark2 = Some((lon2, lat2));
					    self.distance_and_bearing(lat1, lon1, lat2, lon2);
					    println!("{:.1} Mn  {:.0}°", self.distance, self.bearing);
						}
					// Étape 3 : les deux posés -> on efface
					(Some(_), Some(_)) => {
					    self.mark1 = None;
					    self.mark2 = None;
					    (self.distance, self.bearing) = (0.0, 0.0);
						}
				   }
		}
		
	pub fn show(
			&mut self,
			renderer: &MBTilesRenderer,
			map: &mut Map) 
		{
		let s1 = self.get_pt1(&renderer);
	    let s2 = self.get_pt2(&renderer);
	    // les points
	    for (x, y) in [s1, s2].into_iter().flatten() 
			{
			let _ = map.show_mark(x as i16, y as i16);
			}
	    // le trait, seulement si les deux existent
	    if let (Some((x1, y1)), Some((x2, y2))) = (s1, s2) 
			{
			let _ = map.show_line(x1 as i16, y1 as i16, x2 as i16, y2 as i16);
			}
		let text = format!("{:.1} Mn {:.0}°", self.distance, self.bearing);
			if let Some((x,y)) = s2
			{
			let _ = map.show_text(&text,x as i32,y as i32);
			}
		}
		
	fn get_pt1(&self, renderer: &MBTilesRenderer) -> Option<(i32, i32)>
		{
		let pt = renderer.gps_to_point(self.mark1?);
		Some((pt.x, pt.y))
		}

	fn get_pt2(&self, renderer: &MBTilesRenderer) -> Option<(i32, i32)>
		{
		let pt = renderer.gps_to_point(self.mark2?);
		Some((pt.x, pt.y))
		}
					
		
	fn distance_and_bearing(&mut self,lat1: f64, lon1: f64, lat2: f64, lon2: f64)  {
		// Rayon moyen de la Terre en milles nautiques
		const EARTH_RADIUS_NM: f64 = 3440.065;
        
		// Conversion en radians
		let lat1_rad = lat1.to_radians();
		let lat2_rad = lat2.to_radians();
		let delta_lat = (lat2 - lat1).to_radians();
		let delta_lon = (lon2 - lon1).to_radians();

		// --- Distance : formule de Haversine ---
		let a = (delta_lat / 2.0).sin().powi(2)
			+ lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
		let c = 2.0 * a.sqrt().asin();
		self.distance = EARTH_RADIUS_NM * c;

		// --- Cap initial (bearing) ---
		let y = delta_lon.sin() * lat2_rad.cos();
		let x = lat1_rad.cos() * lat2_rad.sin()
			- lat1_rad.sin() * lat2_rad.cos() * delta_lon.cos();
		let bearing_rad = y.atan2(x);

		// Conversion en degrés et normalisation 0-360
		self.bearing = (bearing_rad.to_degrees() + 360.0) % 360.0;
		}
	}
