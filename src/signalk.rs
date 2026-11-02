use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::{
    connect_async, tungstenite::Error as WsError, tungstenite::Message,
    WebSocketStream, MaybeTlsStream,
};
use tokio::net::TcpStream;
use std::time::Duration;
use serde::{Serialize, Deserialize};
use serde_json::Value;
use tokio::sync::watch;

use crate::boat::BoatState;
pub type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub const SIGNALK_URL :&str = "192.168.3.2";

const POSITION_PATH :&str = "navigation.position";
const COG_PATH      :&str = "navigation.courseOverGroundLastMinute";
const SOG_PATH      :&str = "navigation.speedOverGroundLastMinute";


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
                                if status == "signalk-server"   {  println!("connection ok");     return true }
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
        Ok(())   
        }
        
    /// interpretes les messages updates
    fn decode_updates (&mut self, v: Value) -> Result<String, Box<dyn std::error::Error>> {
        if let Some(updates) = v["updates"].as_array() 
            {
            for update in updates {
                if let Some(values) = update["values"].as_array() {
                    for value in values {
                        let val = &value["value"];
                        match value["path"].as_str() {
                            Some(POSITION_PATH)  => { 
                                                    self.data.latitude   = val["latitude"].as_f64().unwrap_or(0.0); 
                                                    self.data.longitude  = val["longitude"].as_f64().unwrap_or(0.0);     
                                                    return Ok("position".to_string());
                                                    }
                            Some(COG_PATH)  =>      { 
                                                    self.data.cog   = val.as_f64().unwrap_or(0.0); 
                                                    return Ok("cog".to_string());
                                                    }
                            Some(SOG_PATH)  =>      { 
                                                    self.data.sog   = val.as_f64().unwrap_or(0.0); 
                                                    return Ok("sog".to_string());
                                                    }
                            
                            Some(other_path) => println!("Unknown path: {}", other_path), 
                            None => println!("Path is missing or not a string"),      
                            }
                        }
                    println!("updates");
                    return Ok("updates".to_string());
                    }
                }
            }
        println!("other");
        Ok("other".to_string())
        } 
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
pub async fn signalk_task(state_tx: watch::Sender<BoatState>, status_tx: watch::Sender<String>) {
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
        }

        let _ = status_tx.send("Try to reconnect...".to_string());
        signalk.reconnect().await;
    }
}

