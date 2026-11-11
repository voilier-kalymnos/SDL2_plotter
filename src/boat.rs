use crate::mbtiles_renderer::MBTilesRenderer;
use crate::screen::Map;
 
const MINIMUM_SPEED :f64 = 1.0;

#[derive(Clone, Copy, Debug)]
#[allow(dead_code)]
pub struct BoatState 
    {
    pub longitude           : f64,
    pub latitude            : f64,
    pub position_valid      : bool,
    pub speed_over_ground   : f64,
    pub course_over_ground  : f64,
    }



impl BoatState
    {
     pub fn default() -> Self 
        {
        BoatState {
            longitude: 0.0,
            latitude: 0.0,
            position_valid: false,
            speed_over_ground: 0.0,
            course_over_ground: 0.0,
        }
        }
        
    pub fn show(
            &self,
			renderer    : &MBTilesRenderer,
			map         : &mut Map)
        {
        if !self.position_valid {
            return;
        }
        //println!("show longitude : {} / latitude : {}",self.longitude,self.latitude);
        let pt = renderer.gps_to_point((self.longitude, self.latitude));
        let _ = map.show_boat(pt.x as i16, pt.y as i16);	
        if self.speed_over_ground > MINIMUM_SPEED
            {
            let _ = map.show_cog(self.course_over_ground,pt);    
            }
        }
        
    pub fn position(self) -> (f64, f64)
        {
        (self.longitude,self.latitude)
        }
    }
