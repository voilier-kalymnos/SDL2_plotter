use serde::{Deserialize, Serialize};
use std::{fs, path::Path};



#[derive(Serialize, Deserialize, Debug)]
pub struct State {
    #[serde(default)]
    pub follow		: bool,
    pub goto		: bool,
    pub map			: String,
    pub position    : Option<(f64, f64)>,
    pub zoom		: i32,
    pub map_offset  : (i32,i32),
    pub nav_id      : usize,
}

impl State 
    {
    pub fn new<P: AsRef<Path>>(
        path: P,
        init_map: String,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        match State::load(path) {
            Ok(state) => Ok(state),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
            Ok(State {
                follow: false,
                goto: false,
                map: init_map,
                position: None,
                zoom: 8,
                map_offset: (0,0),
                nav_id: 0,
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn save<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
	    {
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)?;
        Ok(())
	    }

    pub fn load<P: AsRef<Path>>(
        path: P,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>>
	    {
        let json = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
	    }
    }

    /// Liste les fichiers .mbtiles d'un dossier, triés par nom.
pub fn list_maps(dir: &str) -> Vec<String> {
    let mut maps: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().map_or(false, |e| e.eq_ignore_ascii_case("mbtiles")))
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    maps.sort();
    maps
}

 #[allow(dead_code)]
 fn display_name(path: &str) -> &str {
        Path::new(path)
            .file_stem()          // "fidji" (sans dossier ni extension)
            .and_then(|s| s.to_str())
            .unwrap_or(path)
    }  
