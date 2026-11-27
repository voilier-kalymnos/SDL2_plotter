use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    connect_async, tungstenite::Error as WsError, tungstenite::Message, MaybeTlsStream,
    WebSocketStream,
};

use crate::ais::AisTarget;
use crate::boat::{BoatState, FixGuard};
use crate::config::{SignalKConfig, SignalKPaths};
use crate::feed::LiveFeed;
/// A ping is sent after this long with no message.
const PING_INTERVAL: Duration = Duration::from_secs(10);
/// No message for this long (no pong to two pings): reconnect.
const SILENCE_TIMEOUT: Duration = Duration::from_secs(30);

pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// What one Signal K delta message changed, so the task only forwards the
/// data that actually moved.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DeltaSummary {
    /// Own vessel position, course or speed changed.
    pub self_changed: bool,
    /// Own vessel position was updated (a new track point).
    pub self_position: bool,
    /// At least one AIS target was created or updated.
    pub ais_changed: bool,
}

/// Le client complet : connexion WebSocket + dernieres donnees connues
pub struct SignalKClient {
    write: SplitSink<WsStream, Message>,
    read: SplitStream<WsStream>,
    pub data: BoatState,
    /// Keeps impossible jumps (a second GPS) off the chart.
    guard: FixGuard,
    pub targets: HashMap<String, AisTarget>,
    self_context: Option<String>,
    url: String,
    paths: SignalKPaths,
    /// With no message for this long, a ping is sent.
    ping_interval: Duration,
    /// With no message (pong included) for this long, the connection is
    /// taken as dead and the task reconnects.
    silence_timeout: Duration,
}

impl SignalKClient {
    /// Se connecte (avec retry automatique) et cree le client
    pub async fn connect(config: SignalKConfig) -> Self {
        let ws_stream = wait_for_signalk_connection(&config.url).await;
        let (write, read) = ws_stream.split();

        SignalKClient {
            write,
            read,
            data: BoatState::default(),
            guard: FixGuard::default(),
            targets: HashMap::new(),
            self_context: None,
            url: config.url,
            paths: config.paths,
            ping_interval: PING_INTERVAL,
            silence_timeout: SILENCE_TIMEOUT,
        }
    }

    /// Reconnecte (par ex. apres une coupure), en gardant les dernieres donnees
    pub async fn reconnect(&mut self) {
        let ws_stream = wait_for_signalk_connection(&self.url).await;
        let (write, read) = ws_stream.split();
        self.write = write;
        self.read = read;
        // The server may have changed: read its hello and `self` again.
        if !self.check_connect().await {
            crate::diagnostics::record(
                "WARN",
                "La vérification SignalK après reconnexion n'a pas abouti",
            );
        }
    }

    /// Lit le prochain message et met a jour `self.data` si c'est du SignalK delta.
    /// `None` when the connection ended: closed, an error, or silent for
    /// `silence_timeout` (a ping goes out after each `ping_interval` with no
    /// message, so a live server always answers).
    pub async fn next_update(&mut self) -> Option<DeltaSummary> {
        let mut last_heard = Instant::now();
        loop {
            let msg = match tokio::time::timeout(self.ping_interval, self.read.next()).await {
                Ok(Some(msg)) => msg,
                Ok(None) => return None,
                Err(_) => {
                    if last_heard.elapsed() >= self.silence_timeout {
                        crate::diagnostics::record(
                            "WARN",
                            format!(
                                "SignalK silencieux depuis {}s; reconnexion",
                                self.silence_timeout.as_secs()
                            ),
                        );
                        return None;
                    }
                    if self
                        .write
                        .send(Message::Ping(Default::default()))
                        .await
                        .is_err()
                    {
                        return None;
                    }
                    continue;
                }
            };
            last_heard = Instant::now();
            match msg {
                Ok(Message::Text(text)) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text.as_str()) {
                        // A delta that makes the decoder panic is dropped,
                        // not the connection.
                        if let Some(summary) =
                            crate::diagnostics::survive(|| self.decode_updates(&json))
                        {
                            return Some(summary);
                        }
                    }
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => continue,
                Ok(Message::Close(_)) => return None,
                Err(e) => {
                    crate::diagnostics::record("WARN", format!("Erreur de lecture WebSocket: {e}"));
                    return None;
                }
                _ => continue,
            }
        }
    }

    /// verifie que le premier message du sereveur est bien reçu
    /// Waits at most `silence_timeout` for it.
    pub async fn check_connect(&mut self) -> bool {
        let Ok(msg) = tokio::time::timeout(self.silence_timeout, self.read.next()).await else {
            crate::diagnostics::record("WARN", "Pas de message d'accueil SignalK");
            return false;
        };
        match msg {
            Some(Ok(Message::Text(text))) => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(text.as_str()) {
                    let status = &json["name"];
                    {
                        if status == "signalk-server" {
                            self.self_context = json["self"].as_str().map(str::to_string);
                            if self.self_context.is_none() {
                                crate::diagnostics::record("WARN", "Le serveur Signal K n'a pas fourni le contexte du bateau (self)");
                            }
                            crate::diagnostics::record("INFO", "Identification SignalK réussie");
                            return true;
                        } else {
                            crate::diagnostics::record(
                                "WARN",
                                "Réponse initiale SignalK inattendue",
                            );
                            return false;
                        }
                    }
                }
            }
            Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => return false,
            Some(Ok(Message::Close(_))) => return false,
            Some(Err(e)) => {
                crate::diagnostics::record("WARN", format!("Erreur de lecture WebSocket: {e}"));
                return false;
            }
            _ => return false,
        }
        false
    }

    /// suscribe les paths du tableau SUBSRBES_PATH au sever signalk
    pub async fn subscribe(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        #[derive(Serialize, Deserialize)]
        struct Subscribe {
            context: String,
            subscribe: Vec<Delta>,
        }

        #[derive(Serialize, Deserialize)]
        struct Delta {
            path: String,
            format: String,
            //period : u32,
            //policy: String,
            //#[serde(rename = "minPeriod")] // Assure la compatibilité avec le JSON de Signal K
            //min_period: u32,
        }

        crate::diagnostics::record("INFO", "Souscription SignalK aux données bateau et AIS");
        let self_paths = [
            self.paths.position.as_str(),
            self.paths.course_over_ground.as_str(),
            self.paths.speed_over_ground.as_str(),
            self.paths.heading.as_str(),
            self.paths.magnetic_variation.as_str(),
            self.paths.datetime.as_str(),
        ];
        let deltas: Vec<Delta> = self_paths
            .iter()
            .map(|path| Delta {
                path: path.to_string(),
                format: "delta".to_string(),
                //period: PERIOD_MS,
                //policy: "instant".to_string(), // "instant" pousse les données dès qu'elles changent
                //min_period: PERIOD_MS,
            })
            .collect();

        let payload = Subscribe {
            context: "vessels.self".to_string(),
            subscribe: deltas,
        };
        let json_string = serde_json::to_string(&payload)?;
        self.write.send(Message::Text(json_string.into())).await?;

        let ais_paths = [
            self.paths.ais_position.as_str(),
            self.paths.ais_name.as_str(),
            self.paths.ais_course_over_ground.as_str(),
            self.paths.ais_speed_over_ground.as_str(),
            self.paths.ais_ship_type.as_str(),
            self.paths.ais_nav_state.as_str(),
        ];
        let ais_payload = Subscribe {
            context: "vessels.*".to_string(),
            subscribe: ais_paths
                .iter()
                .map(|path| Delta {
                    path: path.to_string(),
                    format: "delta".to_string(),
                })
                .collect(),
        };
        let json_string = serde_json::to_string(&ais_payload)?;
        self.write.send(Message::Text(json_string.into())).await?;
        Ok(())
    }

    /// interpretes les messages updates
    fn decode_updates(&mut self, v: &Value) -> DeltaSummary {
        apply_delta(
            &mut self.data,
            &mut self.guard,
            &mut self.targets,
            &self.paths,
            self.self_context.as_deref(),
            v,
            Instant::now(),
        )
    }
}

/// Applies one Signal K delta message to the own-vessel data and the AIS
/// target map, and reports what changed.
fn apply_delta(
    data: &mut BoatState,
    guard: &mut FixGuard,
    targets: &mut HashMap<String, AisTarget>,
    paths: &SignalKPaths,
    self_context: Option<&str>,
    v: &Value,
    now: Instant,
) -> DeltaSummary {
    let mut summary = DeltaSummary::default();
    let Some(updates) = v["updates"].as_array() else {
        return summary;
    };
    for update in updates {
        let Some(values) = update["values"].as_array() else {
            continue;
        };
        let context = update["context"]
            .as_str()
            .or_else(|| v["context"].as_str())
            .unwrap_or("vessels.self");
        for value in values {
            let val = &value["value"];
            let Some(path) = value["path"].as_str() else {
                continue;
            };
            if is_self_context(context, self_context) {
                if path == paths.position {
                    if let (Some(latitude), Some(longitude)) =
                        (val["latitude"].as_f64(), val["longitude"].as_f64())
                    {
                        if latitude.is_finite()
                            && longitude.is_finite()
                            && (-90.0..=90.0).contains(&latitude)
                            && (-180.0..=180.0).contains(&longitude)
                            && guard.accept((longitude, latitude), now)
                        {
                            data.latitude = latitude;
                            data.longitude = longitude;
                            data.position_valid = true;
                            data.fixed_at = Some(now);
                            summary.self_position = true;
                            summary.self_changed = true;
                        }
                    }
                } else if path == paths.course_over_ground {
                    if let Some(cog) = val.as_f64().filter(|c| c.is_finite()) {
                        data.course_over_ground = cog.rem_euclid(std::f64::consts::TAU);
                        summary.self_changed = true;
                    }
                } else if path == paths.speed_over_ground {
                    if let Some(sog) = val.as_f64().filter(|s| s.is_finite() && *s >= 0.0) {
                        data.speed_over_ground = sog;
                        summary.self_changed = true;
                    }
                } else if path == paths.heading {
                    if let Some(heading) = val.as_f64().filter(|h| h.is_finite()) {
                        data.heading = Some(heading);
                        data.heading_at = Some(now);
                        summary.self_changed = true;
                    }
                } else if path == paths.magnetic_variation {
                    // At most half a turn, as an NMEA variation.
                    if let Some(variation) = val
                        .as_f64()
                        .map(f64::to_degrees)
                        .filter(|v| v.is_finite() && v.abs() <= 180.0)
                    {
                        data.variation = Some(variation);
                        data.variation_at = Some(now);
                        summary.self_changed = true;
                    }
                } else if path == paths.datetime {
                    if let Some(date) = val
                        .as_str()
                        .and_then(|text| chrono::DateTime::parse_from_rfc3339(text).ok())
                    {
                        data.gps_date = Some(date.with_timezone(&chrono::Utc).date_naive());
                        summary.self_changed = true;
                    }
                }
            } else if update_ais_target(targets, paths, context, path, val) {
                summary.ais_changed = true;
            }
        }
    }
    summary
}

fn is_self_context(context: &str, self_context: Option<&str>) -> bool {
    context == "vessels.self" || self_context == Some(context)
}

/// Updates the AIS target for `context` from one path/value pair.
/// Returns true when the target was created or changed.
fn update_ais_target(
    targets: &mut HashMap<String, AisTarget>,
    paths: &SignalKPaths,
    context: &str,
    path: &str,
    value: &Value,
) -> bool {
    if !context.starts_with("vessels.") || context == "vessels.self" {
        return false;
    }

    let target = targets
        .entry(context.to_string())
        .or_insert_with(|| AisTarget::new(context.to_string()));

    if path == paths.ais_position {
        if let (Some(latitude), Some(longitude)) =
            (value["latitude"].as_f64(), value["longitude"].as_f64())
        {
            if latitude.is_finite()
                && longitude.is_finite()
                && (-90.0..=90.0).contains(&latitude)
                && (-180.0..=180.0).contains(&longitude)
            {
                target.position = Some((longitude, latitude));
            }
        }
    } else if path == paths.ais_name {
        if let Some(name) = value.as_str().filter(|name| !name.trim().is_empty()) {
            target.name = Some(name.to_string());
        }
    } else if path == paths.ais_course_over_ground {
        if let Some(course) = value.as_f64().filter(|course| course.is_finite()) {
            target.course_over_ground = Some(course);
        }
    } else if path == paths.ais_speed_over_ground {
        if let Some(speed) = value
            .as_f64()
            .filter(|speed| speed.is_finite() && *speed >= 0.0)
        {
            target.speed_over_ground = Some(speed);
        }
    } else if path == paths.ais_ship_type {
        if let Some(kind) = value["id"].as_u64().filter(|kind| (1..=99).contains(kind)) {
            target.ship_type = Some(kind as u8);
        }
    } else if path == paths.ais_nav_state {
        target.nav_status = value.as_str().and_then(nav_status_code);
    } else {
        return false;
    }
    target.update_received();
    true
}

/// The AIS navigational status for a Signal K `navigation.state` text
/// (the specification's list); `None` for one with no AIS equivalent the
/// plotter draws.
fn nav_status_code(state: &str) -> Option<u8> {
    Some(match state {
        "motoring" => 0,
        "anchored" => 1,
        "not under command" => 2,
        state if state.starts_with("restricted manouverability") => 3,
        "constrained by draft" => 4,
        "moored" => 5,
        "aground" => 6,
        "fishing" | "fishing-hampered" | "trawling" | "trawling-shooting" | "trawling-hauling" => 7,
        "sailing" => 8,
        _ => return None,
    })
}

async fn wait_for_signalk_connection(url: &str) -> WsStream {
    let mut attempt: u32 = 0;

    loop {
        attempt += 1;
        match connect_async(url).await {
            Ok((ws_stream, response)) => {
                crate::diagnostics::record(
                    "INFO",
                    format!(
                        "Connexion SignalK établie après {attempt} tentative(s), status {}",
                        response.status()
                    ),
                );
                return ws_stream;
            }
            Err(WsError::Http(response)) if response.status().as_u16() == 429 => {
                let wait = response
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or_else(|| (2u64.pow(attempt.min(5))).min(60));

                crate::diagnostics::record(
                    "WARN",
                    format!("SignalK HTTP 429: attente {wait}s (tentative {attempt})"),
                );
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
            Err(error) => {
                crate::diagnostics::record(
                    "WARN",
                    format!("Connexion SignalK échouée (tentative {attempt}): {error}"),
                );
                let delay = Duration::from_secs((attempt.min(10)) as u64);
                tokio::time::sleep(delay).await;
            }
        }
    }
}

// Tache dediee a signalk : connexion, souscription, reception des deltas,
// et reconnexion automatique en cas de coupure. Tourne independamment de
// la boucle SDL, ne bloque donc jamais le rendu / les events clavier-souris.
pub async fn run(config: SignalKConfig, feed: LiveFeed) {
    crate::diagnostics::record(
        "INFO",
        format!("Tâche SignalK démarrée, serveur={}", config.url),
    );
    let mut signalk = SignalKClient::connect(config).await;
    if !signalk.check_connect().await {
        crate::diagnostics::record("WARN", "La vérification initiale SignalK n'a pas abouti");
    }

    loop {
        if let Err(error) = signalk.subscribe().await {
            crate::diagnostics::record("ERROR", format!("Échec de souscription SignalK: {error}"));
        }

        // boucle normale de reception des deltas signalk
        let mut last_eviction = Instant::now();
        while let Some(summary) = signalk.next_update().await {
            if summary.self_changed {
                let _ = feed.boat.send(signalk.data);
            }
            if summary.self_position {
                let _ = feed
                    .track
                    .send((signalk.data.longitude, signalk.data.latitude));
            }

            // Silent targets, and the oldest above the cap, are dropped
            // here, so the map stays bounded for the whole passage. Checked
            // at most once a second.
            let mut ais_changed = summary.ais_changed;
            let now = Instant::now();
            if now.duration_since(last_eviction) >= Duration::from_secs(1) {
                ais_changed |= crate::ais::bound(&mut signalk.targets, now);
                last_eviction = now;
            }
            if ais_changed {
                let _ = feed.ais.send(signalk.targets.values().cloned().collect());
            }
        }

        crate::diagnostics::record("WARN", "Connexion SignalK interrompue, reconnexion");
        signalk.reconnect().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decodes_ais_target_and_ignores_own_vessel() {
        let context = "vessels.urn:mrn:imo:mmsi:123456789";
        let mut targets = HashMap::new();
        let paths = SignalKPaths {
            ais_position: "custom.position".into(),
            ais_name: "custom.name".into(),
            ais_course_over_ground: "custom.course".into(),
            ais_speed_over_ground: "custom.speed".into(),
            ..SignalKPaths::default()
        };

        update_ais_target(
            &mut targets,
            &paths,
            context,
            &paths.ais_position,
            &json!({"latitude": -17.7, "longitude": 178.1}),
        );
        update_ais_target(
            &mut targets,
            &paths,
            context,
            &paths.ais_name,
            &json!("Example"),
        );
        update_ais_target(
            &mut targets,
            &paths,
            context,
            &paths.ais_course_over_ground,
            &json!(1.25),
        );
        update_ais_target(
            &mut targets,
            &paths,
            context,
            &paths.ais_speed_over_ground,
            &json!(4.5),
        );

        let target = targets.get(context).unwrap();
        assert_eq!(target.position, Some((178.1, -17.7)));
        assert_eq!(target.name.as_deref(), Some("Example"));
        assert_eq!(target.course_over_ground, Some(1.25));
        assert_eq!(target.speed_over_ground, Some(4.5));

        update_ais_target(
            &mut targets,
            &paths,
            context,
            &paths.ais_position,
            &json!({"latitude": 91.0, "longitude": 181.0}),
        );
        update_ais_target(
            &mut targets,
            &paths,
            "vessels.self",
            &paths.ais_position,
            &json!({"latitude": 0.0, "longitude": 0.0}),
        );

        assert_eq!(targets.len(), 1);
        assert_eq!(targets.get(context).unwrap().position, Some((178.1, -17.7)));
    }

    #[test]
    fn an_own_position_far_from_the_last_is_ignored() {
        let paths = SignalKPaths::default();
        let mut boat = BoatState::default();
        let mut targets = HashMap::new();
        let mut guard = FixGuard::default();
        let t0 = Instant::now();
        let position = |lat: f64| {
            json!({"context": "vessels.self", "updates": [{"values": [
                {"path": "navigation.position", "value": {"latitude": lat, "longitude": 26.98}}
            ]}]})
        };
        apply_delta(
            &mut boat,
            &mut guard,
            &mut targets,
            &paths,
            None,
            &position(36.95),
            t0,
        );
        let summary = apply_delta(
            &mut boat,
            &mut guard,
            &mut targets,
            &paths,
            None,
            &position(37.95),
            t0 + std::time::Duration::from_secs(1),
        );
        assert!(!summary.self_position);
        assert_eq!(boat.latitude, 36.95);
        assert_eq!(boat.fixed_at, Some(t0));
    }

    #[test]
    fn own_heading_is_read_in_radians_and_stamped() {
        let paths = SignalKPaths::default();
        let mut boat = BoatState::default();
        let mut targets = HashMap::new();
        let now = Instant::now();
        let heading = json!({"context": "vessels.self", "updates": [{"values": [
            {"path": "navigation.headingTrue", "value": 1.25}
        ]}]});
        let summary = apply_delta(
            &mut boat,
            &mut FixGuard::default(),
            &mut targets,
            &paths,
            None,
            &heading,
            now,
        );
        assert!(summary.self_changed);
        assert!(!summary.self_position);
        assert_eq!(boat.heading, Some(1.25));
        assert_eq!(boat.heading_at, Some(now));
        assert_eq!(boat.fixed_at, None, "a heading is not a fix");
    }

    #[test]
    fn own_variation_is_read_from_radians_into_degrees_east_positive() {
        let paths = SignalKPaths::default();
        let mut boat = BoatState::default();
        let now = Instant::now();
        // -0.0541 rad is 3.1 degrees west.
        let delta = json!({"context": "vessels.self", "updates": [{"values": [
            {"path": "navigation.magneticVariation", "value": -0.054_105_2},
            {"path": "navigation.datetime", "value": "2026-11-26T10:15:00.000Z"}
        ]}]});
        let summary = apply_delta(
            &mut boat,
            &mut FixGuard::default(),
            &mut HashMap::new(),
            &paths,
            None,
            &delta,
            now,
        );
        assert!(summary.self_changed);
        assert!((boat.variation.unwrap() - -3.1).abs() < 1e-4, "{boat:?}");
        assert_eq!(boat.variation_at, Some(now));
        assert_eq!(boat.gps_date, chrono::NaiveDate::from_ymd_opt(2026, 11, 26));
    }

    #[test]
    fn a_course_update_does_not_refresh_the_fix_time() {
        let paths = SignalKPaths::default();
        let mut boat = BoatState::default();
        let mut targets = HashMap::new();
        let t0 = Instant::now();
        let own = json!({"context": "vessels.self", "updates": [{"values": [
            {"path": "navigation.position", "value": {"latitude": 36.95, "longitude": 26.98}}
        ]}]});
        apply_delta(
            &mut boat,
            &mut FixGuard::default(),
            &mut targets,
            &paths,
            None,
            &own,
            t0,
        );
        assert_eq!(boat.fixed_at, Some(t0));
        let course = json!({"context": "vessels.self", "updates": [{"values": [
            {"path": "navigation.courseOverGroundLastMinute", "value": 1.5}
        ]}]});
        apply_delta(
            &mut boat,
            &mut FixGuard::default(),
            &mut targets,
            &paths,
            None,
            &course,
            t0 + std::time::Duration::from_secs(5),
        );
        assert_eq!(boat.fixed_at, Some(t0));
        assert_eq!(boat.course_over_ground, 1.5);
    }

    #[test]
    fn ais_ship_type_and_nav_state_are_read() {
        let context = "vessels.urn:mrn:imo:mmsi:123456789";
        let mut targets = HashMap::new();
        let paths = SignalKPaths::default();
        update_ais_target(
            &mut targets,
            &paths,
            context,
            &paths.ais_ship_type,
            &json!({"id": 30, "name": "Fishing"}),
        );
        update_ais_target(
            &mut targets,
            &paths,
            context,
            &paths.ais_nav_state,
            &json!("fishing"),
        );
        let t = targets.get(context).unwrap();
        assert_eq!((t.ship_type, t.nav_status), (Some(30), Some(7)));
        for (state, code) in [
            ("motoring", Some(0)),
            ("anchored", Some(1)),
            ("not under command", Some(2)),
            ("restricted manouverability towing < 200m", Some(3)),
            ("constrained by draft", Some(4)),
            ("moored", Some(5)),
            ("aground", Some(6)),
            ("trawling", Some(7)),
            ("sailing", Some(8)),
            ("pilotage", None),
        ] {
            assert_eq!(nav_status_code(state), code, "{state}");
        }
    }

    #[test]
    fn delta_summary_tells_own_position_apart_from_ais_updates() {
        let paths = SignalKPaths::default();
        let mut data = BoatState::default();
        let mut targets = HashMap::new();
        let now = Instant::now();

        let own = json!({"context": "vessels.self", "updates": [{"values": [
            {"path": "navigation.position", "value": {"latitude": 36.95, "longitude": 26.98}}
        ]}]});
        let summary = apply_delta(
            &mut data,
            &mut FixGuard::default(),
            &mut targets,
            &paths,
            None,
            &own,
            now,
        );
        assert!(summary.self_position);
        assert!(summary.self_changed);
        assert!(!summary.ais_changed);

        let course = json!({"context": "vessels.self", "updates": [{"values": [
            {"path": "navigation.courseOverGroundLastMinute", "value": 1.5}
        ]}]});
        let summary = apply_delta(
            &mut data,
            &mut FixGuard::default(),
            &mut targets,
            &paths,
            None,
            &course,
            now,
        );
        assert_eq!(
            summary,
            DeltaSummary {
                self_changed: true,
                self_position: false,
                ais_changed: false
            }
        );
        assert_eq!(data.course_over_ground, 1.5);
        assert_eq!((data.longitude, data.latitude), (26.98, 36.95));
        assert!(targets.is_empty());

        let other = json!({"context": "vessels.urn:mrn:imo:mmsi:123456789", "updates": [{"values": [
            {"path": "navigation.position", "value": {"latitude": 37.0, "longitude": 27.0}}
        ]}]});
        let summary = apply_delta(
            &mut data,
            &mut FixGuard::default(),
            &mut targets,
            &paths,
            None,
            &other,
            now,
        );
        assert!(!summary.self_position);
        assert!(summary.ais_changed);
        assert_eq!(targets.len(), 1);

        let unrelated = json!({"context": "vessels.self", "updates": [{"values": [
            {"path": "environment.depth.belowKeel", "value": 12.5}
        ]}]});
        let summary = apply_delta(
            &mut data,
            &mut FixGuard::default(),
            &mut targets,
            &paths,
            None,
            &unrelated,
            now,
        );
        assert!(!summary.self_position);
        assert!(!summary.ais_changed);
    }

    #[test]
    fn own_speed_must_be_finite_and_not_negative_and_course_is_normalised() {
        let paths = SignalKPaths::default();
        let mut boat = BoatState::default();
        let mut targets = HashMap::new();
        let now = Instant::now();
        let delta = |path: &str, value: f64| {
            json!({"context": "vessels.self", "updates": [{"values": [
                {"path": path, "value": value}
            ]}]})
        };
        let sog = "navigation.speedOverGroundLastMinute";
        let cog = "navigation.courseOverGroundLastMinute";
        apply_delta(
            &mut boat,
            &mut FixGuard::default(),
            &mut targets,
            &paths,
            None,
            &delta(sog, 2.5),
            now,
        );
        let summary = apply_delta(
            &mut boat,
            &mut FixGuard::default(),
            &mut targets,
            &paths,
            None,
            &delta(sog, -1.0),
            now,
        );
        assert!(!summary.self_changed);
        assert_eq!(boat.speed_over_ground, 2.5, "a negative speed is ignored");
        apply_delta(
            &mut boat,
            &mut FixGuard::default(),
            &mut targets,
            &paths,
            None,
            &delta(cog, 7.0),
            now,
        );
        assert!((boat.course_over_ground - (7.0 - std::f64::consts::TAU)).abs() < 1e-12);
        apply_delta(
            &mut boat,
            &mut FixGuard::default(),
            &mut targets,
            &paths,
            None,
            &delta(cog, -0.5),
            now,
        );
        assert!((boat.course_over_ground - (std::f64::consts::TAU - 0.5)).abs() < 1e-12);
    }

    /// A local Signal K server for one connection: sends `hello` with
    /// `self_context`, then runs `then` on the socket.
    async fn serve_once<F, Fut>(listener: &tokio::net::TcpListener, self_context: &str, then: F)
    where
        F: FnOnce(WebSocketStream<TcpStream>) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        let (tcp, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
        let hello = json!({"name": "signalk-server", "self": self_context}).to_string();
        ws.send(Message::Text(hello.into())).await.unwrap();
        then(ws).await;
    }

    fn local_config(address: std::net::SocketAddr) -> SignalKConfig {
        SignalKConfig {
            url: format!("ws://{address}"),
            paths: SignalKPaths::default(),
        }
    }

    #[tokio::test]
    async fn a_connection_that_goes_silent_ends_so_the_task_reconnects() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        // The server never reads, so it never answers a ping.
        let server = tokio::spawn(async move {
            serve_once(&listener, "vessels.urn:a", |ws| async move {
                tokio::time::sleep(Duration::from_secs(10)).await;
                drop(ws);
            })
            .await;
        });
        let mut client = SignalKClient::connect(local_config(address)).await;
        client.ping_interval = Duration::from_millis(100);
        client.silence_timeout = Duration::from_millis(400);
        assert!(client.check_connect().await);
        let started = Instant::now();
        assert_eq!(client.next_update().await, None);
        assert!(started.elapsed() < Duration::from_secs(3));
        server.abort();
    }

    #[tokio::test]
    async fn a_quiet_connection_that_answers_pings_stays_open() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        // Reading makes the server answer pings, but it sends no data.
        let server = tokio::spawn(async move {
            serve_once(&listener, "vessels.urn:a", |mut ws| async move {
                while ws.next().await.is_some() {}
            })
            .await;
        });
        let mut client = SignalKClient::connect(local_config(address)).await;
        client.ping_interval = Duration::from_millis(100);
        client.silence_timeout = Duration::from_millis(400);
        assert!(client.check_connect().await);
        let waited = tokio::time::timeout(Duration::from_millis(1200), client.next_update()).await;
        assert!(waited.is_err(), "still connected: {waited:?}");
        server.abort();
    }

    #[tokio::test]
    async fn the_hello_is_read_again_after_a_reconnect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            serve_once(&listener, "vessels.urn:first", |ws| async move { drop(ws) }).await;
            serve_once(&listener, "vessels.urn:second", |mut ws| async move {
                while ws.next().await.is_some() {}
            })
            .await;
        });
        let mut client = SignalKClient::connect(local_config(address)).await;
        assert!(client.check_connect().await);
        assert_eq!(client.self_context.as_deref(), Some("vessels.urn:first"));
        assert_eq!(client.next_update().await, None, "the first server closed");
        client.reconnect().await;
        assert_eq!(client.self_context.as_deref(), Some("vessels.urn:second"));
        server.abort();
    }

    #[test]
    fn recognizes_server_reported_self_context() {
        let self_context = Some("vessels.urn:mrn:imo:mmsi:123456789");
        assert!(is_self_context(
            "vessels.urn:mrn:imo:mmsi:123456789",
            self_context
        ));
        assert!(is_self_context("vessels.self", self_context));
        assert!(!is_self_context(
            "vessels.urn:mrn:imo:mmsi:987654321",
            self_context
        ));
    }
}
