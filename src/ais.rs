use std::time::{Duration, Instant};

use crate::mbtiles_renderer::MBTilesRenderer;
use crate::screen::Map;

const TARGET_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Debug)]
pub struct AisTarget {
    pub context: String,
    pub name: Option<String>,
    pub position: Option<(f64, f64)>,
    pub course_over_ground: Option<f64>,
    pub speed_over_ground: Option<f64>,
    last_update: Instant,
}

impl AisTarget {
    pub(crate) fn new(context: String) -> Self {
        Self {
            context,
            name: None,
            position: None,
            course_over_ground: None,
            speed_over_ground: None,
            last_update: Instant::now(),
        }
    }

    pub fn is_fresh(&self, now: Instant) -> bool {
        now.duration_since(self.last_update) < TARGET_TIMEOUT
    }

    pub(crate) fn update_received(&mut self) {
        self.last_update = Instant::now();
    }

    fn label(&self) -> String {
        self.name.clone().unwrap_or_else(|| {
            self.context
                .rsplit(':')
                .next()
                .unwrap_or(&self.context)
                .to_string()
        })
    }
}

pub fn show_targets(
    targets: &[AisTarget],
    renderer: &MBTilesRenderer,
    map: &mut Map,
) -> Result<(), String> {
    let width = map.screen_width as i32;
    let height = map.screen_height as i32;

    for target in targets {
        let Some(position) = target.position else {
            continue;
        };
        let point = renderer.gps_to_point(position);
        if point.x < -32 || point.x > width + 32 || point.y < -32 || point.y > height + 32 {
            continue;
        }

        map.show_ais_target(point.x, point.y, target.course_over_ground)?;
        map.show_text(&target.label(), point.x + 10, point.y - 10)?;
    }

    Ok(())
}
