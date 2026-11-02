
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};


#[derive(Serialize, Deserialize, Debug)]
pub struct State {
    pub edit_route	: bool,
    pub edit_wp		: bool,
    pub mesure		: bool,
    pub follow		: bool,
    pub goto		: bool,
    pub map			: String,
    pub position    : Option<(f64, f64)>,
    pub zoom		: i32,
    pub map_offset  : (i32,i32),
}

impl State 
    {
    pub fn save<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> 
	    {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
	    }

    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> 
	    {
        let json = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
	    }
    }
