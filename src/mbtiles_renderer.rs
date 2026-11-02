use std::collections::HashMap;
use std::num::NonZeroUsize;

use image::{imageops, ImageBuffer, Rgba, RgbaImage};
use lru::LruCache;
use rusqlite::{Connection, OpenFlags};
use sdl2::rect::Point;

const RAYON_TERRE	:f64   = 6378137.0;
const TILE_SIZE     :u32   = 256;
const CACHE_SIZE    :usize = 512;
const PREFETCH_MARGIN: i64 = 2; // nombre de tuiles chargées autour de la vue

use crate::MAP_WORLD;

/// Cached tile: `None` means "tile fetched but could not be decoded / did not exist".
type CachedTile = Option<RgbaImage>;

pub struct MBTilesRenderer {
    #[allow(dead_code)]
    path: String,
    tile_size: u32,
    width: u32,
    height: u32,
    zoom: i32,
    min_zoom: i32,
    max_zoom: i32,
    metadata: HashMap<String, String>,
    #[allow(dead_code)]
    tile_format: String,
    conn: Connection,
    cache: LruCache<(i32, i64, i64), CachedTile>,
    pub resolution : f64,
    /// Centre (en coordonnées "pixel monde") utilisé lors du dernier rendu.
    /// Sert de référence pour convertir d'autres coordonnées GPS (waypoints,
    /// etc.) en coordonnées écran correspondant à la vue actuellement affichée.
    last_center: (f64, f64),
    available_zooms: Vec<i32>,
}

impl MBTilesRenderer {
    pub fn new(
        mbtiles_path: &str,
        zoom: i32,
        width: u32,
        height: u32,
    ) -> rusqlite::Result<Self> 
        {
        // Ouverture en lecture seule, equivalent de "file:...?mode=ro"
        let conn = Connection::open_with_flags(
            mbtiles_path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
        )?;

        let mut renderer = Self {
            path: mbtiles_path.to_string(),
            tile_size : TILE_SIZE,
            width,
            height,
            zoom, // valeur temporaire, ecrasee juste apres par _clamp_zoom
            min_zoom: 0,
            max_zoom: 18,
            metadata: HashMap::new(),
            tile_format: "png".to_string(),
            conn,
            cache: LruCache::new(NonZeroUsize::new(CACHE_SIZE.max(1)).unwrap()),
            resolution: 1.0,
            last_center: (0.0, 0.0),
            available_zooms: Vec::new(),
        };

        renderer.load_metadata()?;
        renderer.zoom = renderer.nearest_zoom( zoom)
                                .unwrap_or_else(|| renderer.clamp_zoom(zoom));
        println!("nearest zoom: {}",renderer.zoom);

        Ok(renderer)
        }

    /// Charge au plus `budget` tuiles manquantes. Renvoie true s'il reste du travail.
    pub fn prefetch_step(&mut self, budget: usize) -> bool {
        let n_tiles: i64 = 1i64 << self.zoom;
        let tile_size = self.tile_size as f64;
        let origin_x = self.last_center.0 - self.width as f64 / 2.0;
        let origin_y = self.last_center.1 - self.height as f64 / 2.0;

        let first_tx = (origin_x / tile_size).floor() as i64 - PREFETCH_MARGIN;
        let first_ty = (origin_y / tile_size).floor() as i64 - PREFETCH_MARGIN;
        let last_tx = ((origin_x + self.width as f64) / tile_size).floor() as i64 + PREFETCH_MARGIN;
        let last_ty = ((origin_y + self.height as f64) / tile_size).floor() as i64 + PREFETCH_MARGIN;

        let mut loaded = 0;
        for tx in first_tx..=last_tx 
            {
            for ty in first_ty..=last_ty 
                {
                if ty < 0 || ty >= n_tiles 
                    {
                    continue;
                    }
                let wx = tx.rem_euclid(n_tiles);
                if self.cache.contains(&(self.zoom, wx, ty)) 
                    {
                    continue;
                    }
                if loaded >= budget 
                    {
                    return true; // il en reste
                    }
                let _ = self.get_tile_image(self.zoom, wx, ty);
                loaded += 1;
                }
            }
            false
        }

    fn nearest_zoom(&self, target: i32) -> Option<i32> 
        {
        self.available_zooms
            .iter()
            .copied()
            .min_by_key(|&z| (z - target).abs())
        }
        
    pub fn zoom_increase(&mut self, map : &str) -> i32
        {
        if let Some(i) = self.available_zooms.iter().position(|&z| z == self.zoom) 
            {
            if let Some(&next) = self.available_zooms.get(i + 1) 
                {
                self.zoom = next;
                }
            else
                {
                println!("zoom max");
                let _ = self.change_map(map);
                }
            }
        println!("zoom : {}",self.zoom);
        self.zoom
        }
    
    pub fn zoom_decrease(&mut self) -> i32
        {
        if let Some(i) = self.available_zooms.iter().position(|&z| z == self.zoom) 
            {
            if let Some(&prev) = i.checked_sub(1).and_then(|j| self.available_zooms.get(j)) 
                {
                self.zoom = prev;
                }
            else{
                println!("zomm mini");
                let _ = self.change_map(MAP_WORLD);
                }
            }
        println!("zomm : {}",self.zoom);
        self.zoom
        }
        
    pub fn change_map(&mut self, mbtiles_path: &str) -> rusqlite::Result<()> {
        // Position GPS actuellement affichée au centre de l'écran
        let (lon, lat) = self.pixel_to_lonlat(self.last_center.0, self.last_center.1);

        // On construit le nouveau renderer d'abord : si ça échoue, self n'est pas touché.
        // Le cache est neuf (donc vide), les métadonnées et les zooms sont rechargés,
        // et le zoom courant est ramené dans [min_zoom, max_zoom] de la nouvelle carte.
        let mut new_renderer = Self::new(mbtiles_path, self.zoom, self.width, self.height)?;

        // On garde un last_center cohérent avec le (possible) nouveau zoom
        new_renderer.last_center = new_renderer.project(lon, lat);

        *self = new_renderer;
        Ok(())
    }
    
/// Centre calculé à partir des tuiles réellement présentes au zoom min.
/// Gère les cartes qui chevauchent l'antiméridien (Pacifique).
    pub fn tiles_center(&self) -> Option<(f64, f64)> {
        let z = self.min_zoom;
        let n = (1i64 << z) as f64;
        let ni = 1i64 << z;

        // Colonnes occupées (triées)
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT tile_column FROM tiles WHERE zoom_level=?1 ORDER BY tile_column")
            .ok()?;
        let cols: Vec<i64> = stmt
            .query_map([z], |r| r.get::<_, i64>(0))
            .ok()?
            .flatten()
            .collect();
        if cols.is_empty() {
            return None;
        }

        // Plus grand trou circulaire entre deux colonnes consécutives
        let mut best_gap = -1i64;
        let mut start = cols[0]; // première colonne après le trou
        for i in 0..cols.len() {
            let cur = cols[i];
            let next = if i + 1 < cols.len() { cols[i + 1] } else { cols[0] + ni };
            let gap = next - cur - 1;
            if gap > best_gap {
                best_gap = gap;
                start = next % ni;
            }
        }

        // Étendue couverte (en tuiles) = tout sauf le trou
        let span = (ni - best_gap) as f64;
        let center_col = (start as f64 + span / 2.0) % n;
        let mut lon = center_col / n * 360.0 - 180.0;
        if lon >= 180.0 {
            lon -= 360.0;
        }

        // Latitude : lignes min/max (TMS -> XYZ)
        let (rmin, rmax): (i64, i64) = self
            .conn
            .query_row(
                "SELECT MIN(tile_row), MAX(tile_row) FROM tiles WHERE zoom_level=?1",
                [z],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()?;
        let y_top = (ni - 1 - rmax) as f64;       // bord nord
        let y_bottom = (ni - 1 - rmin) as f64 + 1.0; // bord sud
        let y_mid = (y_top + y_bottom) / 2.0;

        let merc = std::f64::consts::PI * (1.0 - 2.0 * y_mid / n);
        let lat = merc.sinh().atan().to_degrees();
        Some((lon, lat))
    } 


    pub fn get_raw_rgba(&mut self, lon: f64, lat: f64, offset: (i32,  i32)) ->  Vec<u8> {
        //self.zoom = self.clamp_zoom(zoom);
        let (mut center_x, mut center_y) = self.lonlat_to_pixel(lon, lat);
        center_x -= offset.0 as f64;
        center_y -= offset.1 as f64;
        self.last_center = (center_x, center_y); // mémorisé pour gps_to_screen()
        let image = self.render_view(center_x, center_y);
        return image.into_raw();
    }

    pub fn gps_to_point(&self, pos: (f64,f64)) -> Point
        {
        let (world_x, world_y) = self.project(pos.0, pos.1);
        let (x,y) = self.world_to_screen(world_x, world_y, self.last_center.0, self.last_center.1);
        Point::new(x as i32, y as i32)
        }
    
    pub fn screen_to_gps(&self, dx:i32, dy:i32) -> (f64,f64) {
        let (x,y) = (self.last_center.0,self.last_center.1);
        let (new_lon, new_lat) = self.pixel_to_lonlat( x - dx as f64, y - dy as f64); 
        (new_lon,new_lat)
        }

    fn world_to_screen(&self, world_x: f64, world_y: f64, center_x: f64, center_y: f64) -> (i64, i64) {
        let origin_x = center_x - self.width as f64 / 2.0;
        let origin_y = center_y - self.height as f64 / 2.0;
        (
            (world_x - origin_x).round() as i64,
            (world_y - origin_y).round() as i64,
        )
    }

    fn lonlat_to_pixel(&mut self, lon: f64, lat: f64) -> (f64, f64) {
        let (x, y) = self.project(lon, lat);

        let n = (1i64 << self.zoom) as f64;
        let tile_size = self.tile_size as f64;
        let lat_rad = lat.to_radians();
        self.resolution = (lat_rad.cos() * 2.0 * std::f64::consts::PI * RAYON_TERRE)
                            / (tile_size * n);

        (x, y)
    }

    fn pixel_to_lonlat(&self, x: f64, y: f64) -> (f64, f64) {
        let scale = TILE_SIZE as f64 * 2f64.powf(self.zoom as f64);

        let lon = x / scale * 360.0 - 180.0;

        let n = std::f64::consts::PI - 2.0 * std::f64::consts::PI * y / scale;
        let lat = n.sinh().atan().to_degrees();

        (lon, lat)
    }

    fn load_metadata(&mut self) -> rusqlite::Result<()> {
        // Lecture de la table metadata (peut ne pas exister -> on ignore l'erreur)
        let mut meta = HashMap::new();
        if let Ok(mut stmt) = self.conn.prepare("SELECT name, value FROM metadata") {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            }) {
                for r in rows.flatten() {
                    meta.insert(r.0, r.1);
                }
            }
        }

        self.tile_format = meta.get("format").cloned().unwrap_or_else(|| "png".to_string());

        let parsed_min = meta.get("minzoom").and_then(|v| v.parse::<i32>().ok());
        let parsed_max = meta.get("maxzoom").and_then(|v| v.parse::<i32>().ok());

        match (parsed_min, parsed_max) {
            (Some(min), Some(max)) => {
                self.min_zoom = min;
                self.max_zoom = max;
            }
            _ => {
                let (min_row, max_row): (Option<i64>, Option<i64>) = self.conn.query_row(
                    "SELECT MIN(zoom_level), MAX(zoom_level) FROM tiles",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )?;
                self.min_zoom = min_row.unwrap_or(0) as i32;
                self.max_zoom = max_row.unwrap_or(18) as i32;
            }
        }

        self.metadata = meta;
        let mut zooms: Vec<i32> = Vec::new();
            if let Ok(mut stmt) = self
                .conn
                .prepare("SELECT DISTINCT zoom_level FROM tiles ORDER BY zoom_level")
            {
                if let Ok(rows) = stmt.query_map([], |row| row.get::<_, i32>(0)) {
                    zooms = rows.flatten().collect();
                }
            }
            self.available_zooms = zooms;
        Ok(())
    }

    fn fetch_tile_bytes(&self, z: i32, x: i64, y: i64) -> rusqlite::Result<Option<Vec<u8>>> {
        let n = 1i64 << z; // 2^z
        let tms_y = n - 1 - y; // MBTiles utilise le schema TMS

        let mut stmt = self.conn.prepare(
            "SELECT tile_data FROM tiles WHERE zoom_level=?1 AND tile_column=?2 AND tile_row=?3",
        )?;
        let result = stmt
            .query_row((z, x, tms_y), |row| row.get::<_, Vec<u8>>(0))
            .ok();
        Ok(result)
    }

    fn get_tile_image(&mut self, z: i32, x: i64, y: i64) -> CachedTile {
        let key = (z, x, y);
        if let Some(cached) = self.cache.get(&key) {
            return cached.clone();
        }

        let data = self.fetch_tile_bytes(z, x, y).unwrap_or(None);
        let img = data.and_then(|bytes| {
            image::load_from_memory(&bytes)
                .ok()
                .map(|dyn_img| dyn_img.to_rgba8())
        });

        self.cache.put(key, img.clone());
        img
    }

    fn clamp_zoom(&self, zoom: i32) -> i32 {
        zoom.clamp(self.min_zoom, self.max_zoom)
    }

    fn render_view(&mut self, center_x: f64, center_y: f64) -> RgbaImage {
        let n_tiles: i64 = 1i64 << self.zoom;
        let tile_size = self.tile_size as f64;

        let origin_x = center_x - self.width as f64 / 2.0;
        let origin_y = center_y - self.height as f64 / 2.0;

        let mut canvas: RgbaImage =
            ImageBuffer::from_pixel(self.width, self.height, Rgba([245, 245, 245, 255]));

        let first_tx = (origin_x / tile_size).floor() as i64;
        let first_ty = (origin_y / tile_size).floor() as i64;
        let last_tx = ((origin_x + self.width as f64) / tile_size).floor() as i64;
        let last_ty = ((origin_y + self.height as f64) / tile_size).floor() as i64;

        for tx in first_tx..=last_tx {
            for ty in first_ty..=last_ty {
                if ty < 0 || ty >= n_tiles {
                    continue;
                }

                let wrapped_x = tx.rem_euclid(n_tiles);

                if let Some(tile_img) = self.get_tile_image(self.zoom, wrapped_x, ty) {
                    let px = (tx as f64 * tile_size - origin_x).round() as i64;
                    let py = (ty as f64 * tile_size - origin_y).round() as i64;
                    // overlay applique la fusion alpha, equivalent de
                    // canvas.paste(tile_img, (px, py), tile_img) en PIL
                    imageops::overlay(&mut canvas, &tile_img, px, py);
                }
            }
        }

        canvas
    }

    fn project(&self, lon: f64, lat: f64) -> (f64, f64) 
        {
        let (x, y) = project_norm(lon, lat);
        let scale = self.tile_size as f64 * (1i64 << self.zoom) as f64;
        (x * scale, y * scale)
        }
    
    pub fn norm_to_point(&self, nx: f64, ny: f64) -> Point 
        {
        let scale = self.tile_size as f64 * (1i64 << self.zoom) as f64;
        let origin_x = self.last_center.0 - self.width as f64 / 2.0;
        let origin_y = self.last_center.1 - self.height as f64 / 2.0;
        Point::new(
            (nx * scale - origin_x).round() as i32,
            (ny * scale - origin_y).round() as i32,
            )
        }
}

pub fn project_norm(lon: f64, lat: f64) -> (f64, f64) 
    {
    let lat_rad = lat.to_radians();
    let x = (lon + 180.0) / 360.0;
    let y = (1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / std::f64::consts::PI) / 2.0;
    (x, y)
    }
