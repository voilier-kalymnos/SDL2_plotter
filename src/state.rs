use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Serialize, Deserialize, Debug)]
pub struct State {
    #[serde(default)]
    pub follow: bool,
    pub goto: bool,
    pub map: String,
    /// The chart layer the navigator chose; automatic switching stays in it.
    #[serde(default)]
    pub layer: Option<String>,
    /// The chart file the navigator locked against automatic switching.
    #[serde(default)]
    pub lock: Option<String>,
    /// The last chart layer chosen, where "Imagery off" returns.
    #[serde(default)]
    pub chart_layer: Option<String>,
    /// The last imagery layer chosen, where "Imagery on" goes.
    #[serde(default)]
    pub imagery_layer: Option<String>,
    pub position: Option<(f64, f64)>,
    pub zoom: i32,
    pub map_offset: (i32, i32),
    pub nav_id: usize,
    pub display_waypoint: bool,
    pub display_route: bool,
    pub display_track: bool,
    pub display_ais: bool,
    #[serde(default)]
    pub course_up: bool,
    /// Wins over `course_up`; the two are never both set by an action.
    #[serde(default)]
    pub heading_up: bool,
    /// Own-boat bearings show magnetic (M) instead of true (T).
    #[serde(default)]
    pub magnetic: bool,
}

impl State {
    /// Loads the saved state. A missing file gives the defaults. A file that
    /// cannot be parsed is moved to `<name>.corrupt` for inspection and the
    /// defaults are used, so a damaged state file never stops startup.
    pub fn new<P: AsRef<Path>>(
        path: P,
        init_map: String,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let path = path.as_ref();
        match State::load(path) {
            Ok(state) => Ok(state),
            Err(error)
                if error
                    .downcast_ref::<std::io::Error>()
                    .is_some_and(|error| error.kind() == std::io::ErrorKind::NotFound) =>
            {
                Ok(State::defaults(init_map))
            }
            Err(error) if error.is::<serde_json::Error>() => {
                let aside = crate::geo_data::set_aside(path)?;
                crate::diagnostics::record(
                    "ERROR",
                    format!(
                        "État illisible ({error}); {} déplacé vers {} et valeurs par défaut utilisées",
                        path.display(),
                        aside.display()
                    ),
                );
                Ok(State::defaults(init_map))
            }
            Err(error) => Err(error),
        }
    }

    fn defaults(init_map: String) -> Self {
        State {
            follow: false,
            goto: false,
            map: init_map,
            layer: None,
            lock: None,
            chart_layer: None,
            imagery_layer: None,
            position: None,
            zoom: 8,
            map_offset: (0, 0),
            nav_id: 0,
            display_waypoint: true,
            display_route: true,
            display_track: true,
            display_ais: true,
            course_up: false,
            heading_up: false,
            magnetic: false,
        }
    }

    pub fn save<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let json = serde_json::to_string_pretty(self)?;
        crate::geo_data::atomic_write(path.as_ref(), json.as_bytes())?;
        Ok(())
    }

    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let json = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }
}

#[cfg(test)]
mod tests {
    use super::State;

    #[test]
    fn older_state_files_default_to_north_up() {
        let state: State = serde_json::from_str(
            r#"{
                "follow": false,
                "goto": false,
                "map": "chart.mbtiles",
                "position": null,
                "zoom": 8,
                "map_offset": [0, 0],
                "nav_id": 0,
                "display_waypoint": true,
                "display_route": true,
                "display_track": true,
                "display_ais": true
            }"#,
        )
        .unwrap();

        assert!(!state.course_up);
    }

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "plotter-state-{tag}-{}-{unique}",
            std::process::id()
        ))
    }

    #[test]
    fn save_creates_missing_parent_directories_and_round_trips() {
        let dir = temp_dir("save");
        let path = dir.join("default").join("state.json");
        let mut state = State::new(&path, "chart.mbtiles".to_string()).unwrap();
        state.zoom = 11;
        state.position = Some((26.98, 36.95));

        state.save(&path).unwrap();
        state.zoom = 12;
        state.save(&path).unwrap();

        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.zoom, 12);
        assert_eq!(loaded.position, Some((26.98, 36.95)));
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, vec!["state.json".to_string()]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn corrupt_state_file_is_set_aside_and_defaults_are_used() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state.json");
        std::fs::write(&path, "{ not json").unwrap();

        let state = State::new(&path, "chart.mbtiles".to_string()).unwrap();

        assert_eq!(state.zoom, 8);
        assert_eq!(state.map, "chart.mbtiles");
        assert!(
            !path.exists(),
            "the corrupt file should have been moved aside"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("state.json.corrupt")).unwrap(),
            "{ not json"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
