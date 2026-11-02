 const EARTH_RADIUS_NM: f64 = 3440.065;
 
pub fn distance_wp(pos1: Option<(f64,f64)>, pos2: Option<(f64,f64)>) -> f64
	{
	let (Some((lat1, lon1)), Some((lat2, lon2))) = (pos1, pos2) 
	else   { return 0.0; };
		
	let lat1_rad = lat1.to_radians();
	let lat2_rad = lat2.to_radians();
	let delta_lat = (lat2 - lat1).to_radians();
	let delta_lon = (lon2 - lon1).to_radians();

	// --- Distance : formule de Haversine ---
	let a = (delta_lat / 2.0).sin().powi(2)
		+ lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
	2.0 * EARTH_RADIUS_NM * a.sqrt().asin()
	}

pub fn distance_bearing_wp(pos1:(f64,f64), pos2:(f64,f64)) -> (f64, f64)
		{
		let (lon1, lat1) = pos1;
		let (lon2, lat2) = pos2;

		// Conversion en radians
		let lat1_rad = lat1.to_radians();
		let lat2_rad = lat2.to_radians();
		let delta_lat = (lat2 - lat1).to_radians();
		let delta_lon = (lon2 - lon1).to_radians();

		// --- Distance : formule de Haversine ---
		let a = (delta_lat / 2.0).sin().powi(2)
			+ lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
		let c = 2.0 * a.sqrt().asin();
		let distance = EARTH_RADIUS_NM * c;

		// --- Cap initial (bearing) ---
		let y = delta_lon.sin() * lat2_rad.cos();
		let x = lat1_rad.cos() * lat2_rad.sin()
			- lat1_rad.sin() * lat2_rad.cos() * delta_lon.cos();
		let bearing_rad = y.atan2(x);

		// Conversion en degrés et normalisation 0-360
		let bearing = (bearing_rad.to_degrees() + 360.0) % 360.0;
		(distance, bearing)
		}
