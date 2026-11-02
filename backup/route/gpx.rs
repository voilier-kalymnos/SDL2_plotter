use std::fs::File;
use std::fs;
use std::io::{BufWriter, Write};

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        }


pub fn save_gpx(path: &str, name: &str, route: &[(f64, f64)]) -> std::io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);

    writeln!(w, r#"<?xml version="1.0" encoding="UTF-8"?>"#)?;
    writeln!(
        w,
        r#"<gpx version="1.1" creator="ma_carte" xmlns="http://www.topografix.com/GPX/1/1">"#
    )?;
    writeln!(w, "  <trk>")?;
    writeln!(w, "    <name>{}</name>", xml_escape(name))?;
    writeln!(w, "    <trkseg>")?;

    for &(lon, lat) in route {
        // GPX attend lat puis lon (attention : l'inverse de votre tuple)
        writeln!(w, r#"      <trkpt lat="{:.6}" lon="{:.6}"/>"#, lat, lon)?;
    }

    writeln!(w, "    </trkseg>")?;
    writeln!(w, "  </trk>")?;
    writeln!(w, "</gpx>")?;
    w.flush()
}

pub fn load_gpx(path: &str) -> std::io::Result<Vec<(f64, f64)>> {
    let text = fs::read_to_string(path)?;
    let mut route = Vec::new();

    for chunk in text.split('<').skip(1) {
        let tag = chunk.split('>').next().unwrap_or("");
        if tag.starts_with("trkpt") || tag.starts_with("rtept") {
            if let (Some(lat), Some(lon)) = (parse_attr(tag, "lat"), parse_attr(tag, "lon")) {
                route.push((lon, lat)); // retour en (lon, lat), comme votre route
            }
        }
    }
    Ok(route)
}

fn parse_attr(tag: &str, name: &str) -> Option<f64> {
    let key = format!("{name}=\"");
    let start = tag.find(&key)? + key.len();
    let end = tag[start..].find('"')? + start;
    tag[start..end].parse().ok()
    }
