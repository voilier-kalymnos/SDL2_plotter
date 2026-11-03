use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    connect_async, tungstenite::Error as WsError, tungstenite::Message,
    WebSocketStream, MaybeTlsStream,
};
use tokio::net::TcpStream;
use std::time::Duration;
use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use tokio::sync::watch;

use crate::boat::BoatState;
use crate::ais::AisTarget;
pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub const SIGNALK_URL :&str = "192.168.3.2";

const POSITION_PATH :&str = "navigation.position";
const COG_PATH      :&str = "navigation.courseOverGroundLastMinute";
const SOG_PATH      :&str = "navigation.speedOverGroundLastMinute";
const AIS_POSITION_PATH: &str = "navigation.position";
const AIS_NAME_PATH: &str = "name";
const AIS_COG_PATH: &str = "navigation.courseOverGroundTrue";
const AIS_SOG_PATH: &str = "navigation.speedOverGround";


const SUBSCRIBED_PATHS: [&str; 3] = 
    [
    POSITION_PATH,
    COG_PATH,
    SOG_PATH,
    ];
    
/// Les donnees SignalK collectees au fil de l'eau
#[derive(Debug, Default, Clone)]
pub struct SignalKData {
    pub latitude:   f64,
    pub longitude:  f64,
    pub cog:        f64,
    pub sog:        f64,
    }

/// Le client complet : connexion WebSocket + dernieres donnees connues
pub struct SignalKClient {
    write: SplitSink<WsStream, Message>,
    read: SplitStream<WsStream>,
    pub data: SignalKData,
    pub targets: HashMap<String, AisTarget>,
    self_context: Option<String>,
    url: String,
}

impl SignalKClient {
    /// Se connecte (avec retry automatique) et cree le client
    pub async fn connect() -> Self {
        let url = "ws://".to_owned() + SIGNALK_URL + "/signalk/v1/stream?subscribe=none";
        let ws_stream = wait_for_signalk_connection(&url).await;
        let (write, read) = ws_stream.split();

        SignalKClient {
            write,
            read,
            data: SignalKData::default(),
            targets: HashMap::new(),
            self_context: None,
            url: url.to_string(),
        }
    }

    /// Reconnecte (par ex. apres une coupure), en gardant les dernieres donnees
    pub async fn reconnect(&mut self) {
        let ws_stream = wait_for_signalk_connection(&self.url).await;
        let (write, read) = ws_stream.split();
        self.write = write;
        self.read = read;
    }

    /// Lit le prochain message et met a jour `self.data` si c'est du SignalK delta
    pub async fn next_update(&mut self) -> Option<String> {
        while let Some(msg) = self.read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    //let mess = "none";
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text.as_str())
                        {
                        match  self.decode_updates(json)
                            {
                            Ok(message) => return Some(message),
                            Err(_) => continue, 
                            }
                        }
                    
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => continue,
                Ok(Message::Close(_)) => return None,
                Err(e) => {
                    eprintln!("Erreur de lecture WebSocket: {}", e);
                    return None;
                }
                _ => continue,
            }
        }
        None
    }
    
    /// verifie que le premier message du sereveur est bien reçu 
    pub async fn check_connect(&mut self) -> bool {
        let msg = self.read.next().await ;
        match msg {
                Some(Ok(Message::Text(text))) => {

                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text.as_str())
                        {
                        //println!("json {}",json);
                        let status = &json["name"]; 
                                {
                                if status == "signalk-server" {
                                    self.self_context = json["self"].as_str().map(str::to_string);
                                    if self.self_context.is_none() {
                                        eprintln!("Le serveur Signal K n'a pas fourni le contexte du bateau (self)");
                                    }
                                    println!("connection ok");
                                    return true
                                }
                                else                            {  println!("connection failed"); return false }  
                                }
                        }
                    }
                Some(Ok(Message::Ping(_))) | Some(Ok(Message::Pong(_))) => return false,
                Some(Ok(Message::Close(_))) => return false,
                Some(Err(e)) => {
                    eprintln!("Erreur de lecture WebSocket: {}", e);
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
            subscribe: Vec<Delta>
            }

        #[derive(Serialize, Deserialize)]
        struct Delta {
            path: String,
            format : String,
            //period : u32,
            //policy: String,
            //#[serde(rename = "minPeriod")] // Assure la compatibilité avec le JSON de Signal K
            //min_period: u32,
        }
     
        println!("subscribe path à Signalk");
        let deltas: Vec<Delta> = SUBSCRIBED_PATHS
            .iter()
            .map(|path| Delta {
                path: path.to_string(),
                format: "delta".to_string(),
                //period: PERIOD_MS,
                //policy: "instant".to_string(), // "instant" pousse les données dès qu'elles changent
                //min_period: PERIOD_MS,  
            })
            .collect();
        
        let payload = Subscribe{
            context: "vessels.self".to_string(),
            subscribe: deltas
            };
        let json_string = serde_json::to_string(&payload)?;
        self.write.send(Message::Text(json_string.into())).await?; 

        let ais_paths = [
            AIS_POSITION_PATH,
            AIS_NAME_PATH,
            AIS_COG_PATH,
            AIS_SOG_PATH,
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
    fn decode_updates (&mut self, v: Value) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(updates) = v["updates"].as_array() {
            for update in updates {
                if let Some(values) = update["values"].as_array() {
                    let context = update["context"]
                        .as_str()
                        .or_else(|| v["context"].as_str())
                        .unwrap_or("vessels.self");
                    for value in values {
                        let val = &value["value"];
                        if let Some(path) = value["path"].as_str() {
                            if is_self_context(context, self.self_context.as_deref()) {
                                match path {
                                    POSITION_PATH => {
                                        self.data.latitude = val["latitude"].as_f64().unwrap_or(self.data.latitude);
                                        self.data.longitude = val["longitude"].as_f64().unwrap_or(self.data.longitude);
                                    }
                                    COG_PATH => self.data.cog = val.as_f64().unwrap_or(self.data.cog),
                                    SOG_PATH => self.data.sog = val.as_f64().unwrap_or(self.data.sog),
                                    _ => {}
                                }
                            } else {
                                update_ais_target(&mut self.targets, context, path, val);
                            }
                        }
                    }
                }
            }
            return Ok("updates".to_string());
        }
        println!("other");
        Ok("other".to_string())
        } 
}

fn is_self_context(context: &str, self_context: Option<&str>) -> bool {
    context == "vessels.self" || self_context == Some(context)
}

fn update_ais_target(
    targets: &mut HashMap<String, AisTarget>,
    context: &str,
    path: &str,
    value: &Value,
) {
    if !context.starts_with("vessels.") || context == "vessels.self" {
        return;
    }

    let target = targets
        .entry(context.to_string())
        .or_insert_with(|| AisTarget::new(context.to_string()));

    match path {
        AIS_POSITION_PATH => {
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
        }
        AIS_NAME_PATH => {
            if let Some(name) = value.as_str().filter(|name| !name.trim().is_empty()) {
                target.name = Some(name.to_string());
            }
        }
        AIS_COG_PATH => {
            if let Some(course) = value.as_f64().filter(|course| course.is_finite()) {
                target.course_over_ground = Some(course);
            }
        }
        AIS_SOG_PATH => {
            if let Some(speed) = value.as_f64().filter(|speed| speed.is_finite() && *speed >= 0.0) {
                target.speed_over_ground = Some(speed);
            }
        }
        _ => return,
    }
    target.update_received();
}
    
async fn wait_for_signalk_connection(url: &str) -> WsStream {
    let mut attempt: u32 = 0;

    loop {
        attempt += 1;
        match connect_async(url).await {
            Ok((ws_stream, response)) => {
                println!(
                    "Connecté à SignalK après {} tentative(s) (status: {})",
                    attempt,
                    response.status()
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

                eprintln!("429 recu, attente de {}s (tentative {})", wait, attempt);
                tokio::time::sleep(Duration::from_secs(wait)).await;
            }
            Err(_e) => {
                //~ eprintln!("chec de connexion (tentative {}): {}", attempt, e);
                let delay = Duration::from_secs((attempt.min(10)) as u64);
                tokio::time::sleep(delay).await;
            }
        }
    }

}

// Tache dediee a signalk : connexion, souscription, reception des deltas,
// et reconnexion automatique en cas de coupure. Tourne independamment de
// la boucle SDL, ne bloque donc jamais le rendu / les events clavier-souris.
pub async fn signalk_task(
    state_tx: watch::Sender<BoatState>,
    ais_tx: watch::Sender<Vec<AisTarget>>,
    status_tx: watch::Sender<String>,
) {
    let _ = status_tx.send(format!("Connecting to Signalk {}...", SIGNALK_URL));
    let mut signalk = SignalKClient::connect().await;
    let _ = signalk.check_connect().await;

    loop {
        let _ = status_tx.send("Signalk Subscribe...".to_string());
        let _ = signalk.subscribe().await;

        let _ = status_tx.send("Load Map...".to_string());
        // boucle normale de reception des deltas signalk
        while let Some(_mess) = signalk.next_update().await {
            let state = BoatState {
                longitude: signalk.data.longitude,
                latitude: signalk.data.latitude,
                course_over_ground: signalk.data.cog,
                speed_over_ground: signalk.data.sog,
                };
            let _ = state_tx.send(state);
                let _ = ais_tx.send(signalk.targets.values().cloned().collect());
        }

        let _ = status_tx.send("Try to reconnect...".to_string());
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

        update_ais_target(
            &mut targets,
            context,
            AIS_POSITION_PATH,
            &json!({"latitude": -17.7, "longitude": 178.1}),
        );
        update_ais_target(&mut targets, context, AIS_NAME_PATH, &json!("Example"));
        update_ais_target(&mut targets, context, AIS_COG_PATH, &json!(1.25));
        update_ais_target(&mut targets, context, AIS_SOG_PATH, &json!(4.5));

        let target = targets.get(context).unwrap();
        assert_eq!(target.position, Some((178.1, -17.7)));
        assert_eq!(target.name.as_deref(), Some("Example"));
        assert_eq!(target.course_over_ground, Some(1.25));
        assert_eq!(target.speed_over_ground, Some(4.5));

        update_ais_target(
            &mut targets,
            context,
            AIS_POSITION_PATH,
            &json!({"latitude": 91.0, "longitude": 181.0}),
        );
        update_ais_target(
            &mut targets,
            "vessels.self",
            AIS_POSITION_PATH,
            &json!({"latitude": 0.0, "longitude": 0.0}),
        );

        assert_eq!(targets.len(), 1);
        assert_eq!(targets.get(context).unwrap().position, Some((178.1, -17.7)));
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
