use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
#[derive(Default)]
pub struct AppConfig {
    pub input: InputConfig,
    pub signalk: SignalKConfig,
    pub nmea0183: Nmea0183Config,
    pub charts: ChartsConfig,
    pub window: WindowConfig,
    pub fonts: FontConfig,
    pub appearance: AppearanceConfig,
    pub rendering: RenderingConfig,
    pub streaming: StreamingConfig,
    pub track: TrackConfig,
    pub collision: CollisionConfig,
}

/// Where live boat data comes from. One source at a time, chosen at start.
/// Read from a word in the toml; a word the app does not know is kept as
/// `Unknown`, so a typo never stops the start (see `input_error`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(from = "String", into = "String")]
pub enum InputSource {
    #[default]
    Signalk,
    Nmea0183,
    /// Accepted, not supported yet: the app runs with no live data.
    Nmea2k,
    Unknown(String),
}

impl From<String> for InputSource {
    fn from(word: String) -> Self {
        match word.as_str() {
            "signalk" => Self::Signalk,
            "nmea0183" => Self::Nmea0183,
            "nmea2k" => Self::Nmea2k,
            _ => Self::Unknown(word),
        }
    }
}

impl From<InputSource> for String {
    fn from(source: InputSource) -> Self {
        match source {
            InputSource::Signalk => "signalk".into(),
            InputSource::Nmea0183 => "nmea0183".into(),
            InputSource::Nmea2k => "nmea2k".into(),
            InputSource::Unknown(word) => word,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct InputConfig {
    pub source: InputSource,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Transport {
    /// The plotter connects to `address`.
    #[default]
    Tcp,
    /// The plotter binds `address` and listens.
    Udp,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct Nmea0183Config {
    pub transport: Transport,
    pub address: String,
    /// Read position sentences (RMC, GGA, GLL, VTG) from this talker only
    /// (for example "GN"), for a
    /// multiplexer that mixes two GPS receivers. Empty: every talker.
    pub talker: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct SignalKConfig {
    pub url: String,
    pub paths: SignalKPaths,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct SignalKPaths {
    pub position: String,
    pub course_over_ground: String,
    pub speed_over_ground: String,
    pub heading: String,
    /// Radians, east positive.
    pub magnetic_variation: String,
    /// ISO 8601 UTC date and time from the GPS, for the magnetic model.
    pub datetime: String,
    pub ais_position: String,
    pub ais_name: String,
    pub ais_course_over_ground: String,
    pub ais_speed_over_ground: String,
    /// `{id, name}`; `id` is the AIS ship type.
    pub ais_ship_type: String,
    /// Text such as "anchored" or "fishing".
    pub ais_nav_state: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct ChartsConfig {
    pub directory: String,
    pub default: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct WindowConfig {
    pub title: String,
    pub fullscreen: bool,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct FontConfig {
    pub file: String,
    pub window_size: u16,
    pub ui_size: u16,
    /// AIS target names on the chart.
    pub label_size: u16,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct AppearanceConfig {
    /// The round marker's radius, when no direction is known.
    pub boat_size: i16,
    /// The heading line's length.
    pub boat_circle_radius: i16,
    /// The hull: centre to bow, in pixels.
    pub boat_hull_size: i16,
    pub mark_size: i16,
    pub route_point_size: i16,
    pub route_width: u8,
    pub cog_length: f64,
    pub cog_width: u8,
    pub ais_course_length: i16,
    pub ais_course_width: u8,
    pub ais_size: i16,
    pub ais_outline_size: i16,
    pub boat_color: [u8; 4],
    /// The boat marker while there is no fix.
    pub boat_no_fix_color: [u8; 4],
    pub measure_color: [u8; 4],
    pub route_color: [u8; 4],
    pub track_color: [u8; 4],
    pub waypoint_color: [u8; 4],
    /// The chart manager's "Where" outline.
    pub coverage_color: [u8; 4],
    pub cog_color: [u8; 4],
    /// AIS target colours by ship type (see `overlay::Category`).
    pub ais_cargo_color: [u8; 4],
    pub ais_tanker_color: [u8; 4],
    pub ais_passenger_color: [u8; 4],
    pub ais_high_speed_color: [u8; 4],
    pub ais_service_color: [u8; 4],
    pub ais_fishing_color: [u8; 4],
    pub ais_leisure_color: [u8; 4],
    pub ais_other_color: [u8; 4],
    pub ais_outline_color: [u8; 4],
    pub map_background_color: [u8; 4],
    pub line_color: [u8; 4],
    pub night_overlay_color: [u8; 4],
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct RenderingConfig {
    pub tile_cache_size: usize,
    pub prefetch_margin: i64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct TrackConfig {
    pub checkpoint_interval_seconds: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct CollisionConfig {
    pub enabled: bool,
    pub cpa_nm: f64,
    pub tcpa_minutes: f64,
    pub proximity_nm: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct StreamingConfig {
    pub enabled: bool,
    pub bind_address: String,
    pub port: u16,
    pub fps: u32,
    pub jpeg_quality: u8,
}

impl AppConfig {
    pub fn load_or_create(path: &Path) -> anyhow::Result<Self> {
        let config = match fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents).with_context(|| {
                format!("Impossible de lire la configuration {}", path.display())
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let config = Self::default();
                let contents = toml::to_string_pretty(&config)?;
                fs::write(path, contents).with_context(|| {
                    format!("Impossible de créer la configuration {}", path.display())
                })?;
                config
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Impossible de lire la configuration {}", path.display())
                });
            }
        };
        config.validate()?;
        Ok(config)
    }

    /// Why the live-data source cannot start, if it cannot: a bad Signal K
    /// URL or path, a bad NMEA 0183 address or talker, an unknown or
    /// unsupported `[input] source`. The app then logs it as an ERROR and
    /// runs with no live data; it is never a reason to stop.
    pub fn input_error(&self) -> Option<String> {
        match &self.input.source {
            InputSource::Signalk => {
                match url::Url::parse(&self.signalk.url) {
                    Ok(url) if matches!(url.scheme(), "ws" | "wss") => {}
                    _ => {
                        return Some(format!(
                            "signalk.url doit être une URL WebSocket ws:// ou wss:// valide: {:?}",
                            self.signalk.url
                        ))
                    }
                }
                let paths = [
                    &self.signalk.paths.position,
                    &self.signalk.paths.course_over_ground,
                    &self.signalk.paths.speed_over_ground,
                    &self.signalk.paths.heading,
                    &self.signalk.paths.magnetic_variation,
                    &self.signalk.paths.datetime,
                    &self.signalk.paths.ais_position,
                    &self.signalk.paths.ais_name,
                    &self.signalk.paths.ais_course_over_ground,
                    &self.signalk.paths.ais_speed_over_ground,
                    &self.signalk.paths.ais_ship_type,
                    &self.signalk.paths.ais_nav_state,
                ];
                if paths.iter().any(|path| path.trim().is_empty()) {
                    return Some("Les chemins Signal K ne peuvent pas être vides".into());
                }
                None
            }
            InputSource::Nmea0183 => {
                if !is_host_and_port(&self.nmea0183.address) {
                    return Some(format!(
                        "nmea0183.address doit être hôte:port (par ex. 127.0.0.1:10110): {:?}",
                        self.nmea0183.address
                    ));
                }
                let talker = &self.nmea0183.talker;
                if !talker.is_empty()
                    && !(talker.len() == 2 && talker.bytes().all(|b| b.is_ascii_uppercase()))
                {
                    return Some(format!(
                        "nmea0183.talker doit être vide ou deux lettres majuscules (par ex. GN): {talker:?}"
                    ));
                }
                None
            }
            InputSource::Nmea2k => Some("Entrée NMEA 2000 pas encore prise en charge".into()),
            InputSource::Unknown(word) => Some(format!(
                "input.source inconnue: {word:?} (signalk, nmea0183 ou nmea2k)"
            )),
        }
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.charts.directory.is_empty() || self.charts.default.is_empty() {
            bail!("charts.directory et charts.default ne peuvent pas être vides");
        }
        if self.window.width == 0 || self.window.height == 0 {
            bail!("window.width et window.height doivent être supérieurs à zéro");
        }
        if self.fonts.file.is_empty()
            || self.fonts.window_size == 0
            || self.fonts.ui_size == 0
            || self.fonts.label_size == 0
        {
            bail!("fonts.file, fonts.window_size, fonts.ui_size et fonts.label_size doivent être renseignés");
        }
        let appearance_sizes = [
            self.appearance.boat_size,
            self.appearance.boat_circle_radius,
            self.appearance.boat_hull_size,
            self.appearance.mark_size,
            self.appearance.route_point_size,
            self.appearance.ais_course_length,
            self.appearance.ais_size,
            self.appearance.ais_outline_size,
        ];
        if appearance_sizes.iter().any(|size| *size <= 0)
            || self.appearance.route_width == 0
            || self.appearance.cog_width == 0
            || self.appearance.ais_course_width == 0
            || !self.appearance.cog_length.is_finite()
            || self.appearance.cog_length <= 0.0
            || self.appearance.cog_length > i16::MAX as f64
        {
            bail!(
                "Les tailles de appearance doivent être positives et cog_length ne pas dépasser {}",
                i16::MAX
            );
        }
        if self.rendering.tile_cache_size == 0
            || !(0..=16).contains(&self.rendering.prefetch_margin)
        {
            bail!("rendering.tile_cache_size doit être positif et prefetch_margin compris entre 0 et 16");
        }
        if self.track.checkpoint_interval_seconds == 0 {
            bail!("track.checkpoint_interval_seconds doit être supérieur à zéro");
        }
        if self.collision.enabled
            && (!self.collision.cpa_nm.is_finite()
                || self.collision.cpa_nm <= 0.0
                || !self.collision.tcpa_minutes.is_finite()
                || self.collision.tcpa_minutes <= 0.0
                || !self.collision.proximity_nm.is_finite()
                || self.collision.proximity_nm <= 0.0)
        {
            bail!(
                "collision.cpa_nm, collision.tcpa_minutes et collision.proximity_nm doivent être positifs"
            );
        }
        if self.streaming.enabled
            && (self.streaming.port == 0
                || self.streaming.bind_address.trim().is_empty()
                || !(1..=30).contains(&self.streaming.fps)
                || self.streaming.jpeg_quality == 0)
        {
            bail!("streaming.port, streaming.bind_address, streaming.fps et streaming.jpeg_quality doivent être valides");
        }
        Ok(())
    }
}

/// `host:port` or `ip:port`, with a port from 1 to 65535. A host name is
/// looked up when the TCP client connects or the UDP socket binds.
fn is_host_and_port(address: &str) -> bool {
    if address.parse::<std::net::SocketAddr>().is_ok() {
        return true;
    }
    address.rsplit_once(':').is_some_and(|(host, port)| {
        !host.is_empty()
            && !host.contains([':', ' ', '[', ']'])
            && port.parse::<u16>().is_ok_and(|port| port > 0)
    })
}

impl Default for Nmea0183Config {
    fn default() -> Self {
        Self {
            transport: Transport::Tcp,
            address: "127.0.0.1:10110".into(),
            talker: String::new(),
        }
    }
}

impl Default for SignalKConfig {
    fn default() -> Self {
        Self {
            url: "ws://192.168.3.2/signalk/v1/stream?subscribe=none".into(),
            paths: SignalKPaths::default(),
        }
    }
}

impl Default for SignalKPaths {
    fn default() -> Self {
        Self {
            position: "navigation.position".into(),
            course_over_ground: "navigation.courseOverGroundLastMinute".into(),
            speed_over_ground: "navigation.speedOverGroundLastMinute".into(),
            heading: "navigation.headingTrue".into(),
            magnetic_variation: "navigation.magneticVariation".into(),
            datetime: "navigation.datetime".into(),
            ais_position: "navigation.position".into(),
            ais_name: "name".into(),
            ais_course_over_ground: "navigation.courseOverGroundTrue".into(),
            ais_speed_over_ground: "navigation.speedOverGround".into(),
            ais_ship_type: "design.aisShipType".into(),
            ais_nav_state: "navigation.state".into(),
        }
    }
}

impl Default for ChartsConfig {
    fn default() -> Self {
        Self {
            directory: "map".into(),
            default: "assets/OSM.mbtiles".into(),
        }
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "KALYMNOS".into(),
            fullscreen: false,
            width: 1600,
            height: 800,
        }
    }
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            file: "assets/Montserrat-Regular.ttf".into(),
            window_size: 28,
            ui_size: 16,
            label_size: 14,
        }
    }
}

impl Default for AppearanceConfig {
    fn default() -> Self {
        Self {
            boat_size: 9,
            boat_circle_radius: 50,
            boat_hull_size: 27,
            mark_size: 6,
            route_point_size: 6,
            route_width: 3,
            cog_length: 250.0,
            cog_width: 4,
            ais_course_length: 28,
            ais_course_width: 2,
            ais_size: 7,
            ais_outline_size: 9,
            boat_color: [255, 0, 0, 255],
            boat_no_fix_color: [128, 128, 128, 255],
            measure_color: [0, 0, 255, 255],
            route_color: [255, 0, 0, 255],
            track_color: [255, 0, 0, 255],
            waypoint_color: [220, 30, 30, 255],
            coverage_color: [255, 140, 0, 255],
            cog_color: [0, 0, 255, 255],
            ais_cargo_color: [0, 125, 0, 255],
            ais_tanker_color: [150, 0, 150, 255],
            ais_passenger_color: [0, 70, 200, 255],
            ais_high_speed_color: [185, 150, 0, 255],
            ais_service_color: [0, 130, 140, 255],
            ais_fishing_color: [215, 85, 0, 255],
            ais_leisure_color: [105, 45, 175, 255],
            ais_other_color: [95, 95, 95, 255],
            ais_outline_color: [0, 0, 0, 255],
            map_background_color: [250, 250, 250, 255],
            line_color: [0, 0, 255, 255],
            night_overlay_color: [255, 0, 0, 255],
        }
    }
}

impl Default for RenderingConfig {
    fn default() -> Self {
        Self {
            tile_cache_size: 512,
            prefetch_margin: 2,
        }
    }
}

impl Default for TrackConfig {
    fn default() -> Self {
        Self {
            checkpoint_interval_seconds: 15,
        }
    }
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind_address: "0.0.0.0".into(),
            port: 8080,
            fps: 5,
            jpeg_quality: 70,
        }
    }
}

impl Default for CollisionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cpa_nm: 0.5,
            tcpa_minutes: 15.0,
            proximity_nm: 2.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;
    use super::{InputSource, Transport};

    #[test]
    fn ais_labels_are_half_the_chart_text_size_by_default() {
        let config = AppConfig::default();
        assert_eq!(config.fonts.label_size * 2, config.fonts.window_size);
        let mut bad = AppConfig::default();
        bad.fonts.label_size = 0;
        assert!(bad.validate().is_err());
    }

    #[test]
    fn a_config_without_an_input_table_reads_signal_k() {
        let config: AppConfig = toml::from_str("").unwrap();
        assert_eq!(config.input.source, InputSource::Signalk);
        assert_eq!(config.nmea0183.transport, Transport::Tcp);
        assert_eq!(config.nmea0183.address, "127.0.0.1:10110");
        assert_eq!(config.signalk.paths.heading, "navigation.headingTrue");
        assert_eq!(config.appearance.boat_color, [255, 0, 0, 255]);
        assert_eq!(config.appearance.boat_no_fix_color, [128, 128, 128, 255]);
    }

    #[test]
    fn nmea0183_input_reads_its_own_table() {
        let config: AppConfig = toml::from_str(
            "[input]
source = \"nmea0183\"
[nmea0183]
transport = \"udp\"
address = \"0.0.0.0:10110\"
",
        )
        .unwrap();
        assert_eq!(config.input.source, InputSource::Nmea0183);
        assert_eq!(config.nmea0183.transport, Transport::Udp);
        assert_eq!(config.nmea0183.address, "0.0.0.0:10110");
        config.validate().unwrap();
    }

    #[test]
    fn a_bad_nmea0183_address_is_an_input_error_not_a_start_error() {
        let mut config = AppConfig::default();
        config.nmea0183.address = "not an address".into();
        assert_eq!(config.input_error(), None, "signalk chosen");
        config.input.source = InputSource::Nmea0183;
        config.validate().unwrap();
        assert!(config.input_error().unwrap().contains("nmea0183.address"));
        for bad in ["", "10110", "host:", ":10110", "host:99999"] {
            config.nmea0183.address = bad.into();
            assert!(config.input_error().is_some(), "{bad:?}");
        }
    }

    #[test]
    fn a_hostname_is_a_valid_nmea0183_address() {
        let mut config = AppConfig::default();
        config.input.source = InputSource::Nmea0183;
        for good in ["gateway.local:10110", "127.0.0.1:10110", "[::1]:10110"] {
            config.nmea0183.address = good.into();
            assert_eq!(config.input_error(), None, "{good}");
        }
    }

    #[test]
    fn the_signal_k_url_is_checked_only_when_signal_k_is_chosen() {
        let mut config = AppConfig::default();
        config.signalk.url = "http://wrong".into();
        config.validate().unwrap();
        assert!(config.input_error().unwrap().contains("signalk.url"));
        config.input.source = InputSource::Nmea0183;
        assert_eq!(config.input_error(), None);
    }

    #[test]
    fn an_empty_variation_or_date_path_is_an_input_error() {
        let mut config = AppConfig::default();
        assert_eq!(config.input_error(), None);
        config.signalk.paths.magnetic_variation = " ".into();
        assert!(config.input_error().is_some());
        config.signalk.paths.magnetic_variation = "navigation.magneticVariation".into();
        config.signalk.paths.datetime = String::new();
        assert!(config.input_error().is_some());
    }

    #[test]
    fn the_nmea0183_talker_is_empty_or_two_capital_letters() {
        let mut config = AppConfig::default();
        config.input.source = InputSource::Nmea0183;
        assert_eq!(config.nmea0183.talker, "");
        assert_eq!(config.input_error(), None);
        config.nmea0183.talker = "GN".into();
        assert_eq!(config.input_error(), None);
        for bad in ["gn", "G", "GPS", "G1"] {
            config.nmea0183.talker = bad.into();
            config.validate().unwrap();
            assert!(config.input_error().is_some(), "{bad}");
        }
    }

    #[test]
    fn an_unknown_source_word_is_an_input_error_not_a_start_error() {
        let config: AppConfig = toml::from_str("[input]\nsource = \"nmea\"\n").unwrap();
        assert_eq!(config.input.source, InputSource::Unknown("nmea".into()));
        config.validate().unwrap();
        assert!(config.input_error().unwrap().contains("nmea"));
        let back = toml::to_string(&config).unwrap();
        assert!(
            back.contains("source = \"nmea\""),
            "kept as written: {back}"
        );
    }

    #[test]
    fn nmea2k_is_not_supported_yet() {
        let config: AppConfig = toml::from_str("[input]\nsource = \"nmea2k\"\n").unwrap();
        assert_eq!(config.input.source, InputSource::Nmea2k);
        config.validate().unwrap();
        assert!(config.input_error().unwrap().contains("NMEA 2000"));
    }

    #[test]
    fn omitted_fields_use_defaults() {
        let config: AppConfig = toml::from_str("[window]\nwidth = 1024").unwrap();
        assert_eq!(config.window.width, 1024);
        assert_eq!(config.window.height, 800);
        assert_eq!(config.rendering.tile_cache_size, 512);
        config.validate().unwrap();
    }

    #[test]
    fn default_configuration_round_trips() {
        let serialized = toml::to_string(&AppConfig::default()).unwrap();
        let config: AppConfig = toml::from_str(&serialized).unwrap();
        config.validate().unwrap();
    }

    #[test]
    fn invalid_websocket_url_is_an_input_error() {
        let mut config = AppConfig::default();
        config.signalk.url = "http://localhost".into();
        assert!(config.input_error().is_some());
        config.signalk.url = "ws://192.168.3.2/signalk/v1/stream".into();
        config.signalk.paths.position = " ".into();
        assert!(config.input_error().unwrap().contains("chemins"));
    }

    #[test]
    fn invalid_appearance_sizes_are_rejected() {
        let mut config = AppConfig::default();
        config.appearance.boat_size = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn missing_configuration_is_created_with_defaults() {
        let path =
            std::env::temp_dir().join(format!("plotter-config-test-{}.toml", std::process::id()));
        let config = AppConfig::load_or_create(&path).unwrap();
        assert_eq!(config.window.width, 1600);
        assert!(path.exists());
        std::fs::remove_file(path).unwrap();
    }
}
