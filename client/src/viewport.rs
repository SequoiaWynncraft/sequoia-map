/// Viewport manages the pan/zoom transformation from world coordinates to screen coordinates.
/// Uses CSS matrix3d transforms for GPU-accelerated rendering.
#[derive(Debug, Clone)]
pub struct Viewport {
    pub offset_x: f64,
    pub offset_y: f64,
    pub scale: f64,
}

const MIN_SCALE: f64 = 0.05;
const MAX_SCALE: f64 = 8.0;
const ZOOM_SENSITIVITY: f64 = 0.001;

impl Default for Viewport {
    fn default() -> Self {
        Self {
            offset_x: 0.0,
            offset_y: 0.0,
            scale: 0.3,
        }
    }
}

impl Viewport {
    /// Convert world coordinates to screen coordinates.
    pub fn world_to_screen(&self, wx: f64, wy: f64) -> (f64, f64) {
        (
            wx * self.scale + self.offset_x,
            wy * self.scale + self.offset_y,
        )
    }

    /// Convert screen coordinates to world coordinates.
    pub fn screen_to_world(&self, sx: f64, sy: f64) -> (f64, f64) {
        (
            (sx - self.offset_x) / self.scale,
            (sy - self.offset_y) / self.scale,
        )
    }

    /// Zoom toward a focus point (screen coordinates).
    pub fn zoom_at(&mut self, delta: f64, screen_x: f64, screen_y: f64) {
        let factor = (-delta * ZOOM_SENSITIVITY).exp();
        let new_scale = (self.scale * factor).clamp(MIN_SCALE, MAX_SCALE);
        let ratio = new_scale / self.scale;

        // Adjust offset so the point under the cursor stays fixed
        self.offset_x = screen_x - (screen_x - self.offset_x) * ratio;
        self.offset_y = screen_y - (screen_y - self.offset_y) * ratio;
        self.scale = new_scale;
    }

    /// Pan by screen-space delta.
    pub fn pan(&mut self, dx: f64, dy: f64) {
        self.offset_x += dx;
        self.offset_y += dy;
    }

    /// Fit the viewport to show the given world-coordinate bounds with padding.
    pub fn fit_bounds(
        &mut self,
        min_x: f64,
        min_y: f64,
        max_x: f64,
        max_y: f64,
        canvas_w: f64,
        canvas_h: f64,
    ) {
        let world_w = max_x - min_x;
        let world_h = max_y - min_y;

        if world_w <= 0.0 || world_h <= 0.0 || canvas_w <= 0.0 || canvas_h <= 0.0 {
            return;
        }

        let padding = 0.05;
        let scale_x = canvas_w / (world_w * (1.0 + padding * 2.0));
        let scale_y = canvas_h / (world_h * (1.0 + padding * 2.0));
        self.scale = scale_x.min(scale_y).clamp(MIN_SCALE, MAX_SCALE);

        let center_x = (min_x + max_x) / 2.0;
        let center_y = (min_y + max_y) / 2.0;
        self.offset_x = canvas_w / 2.0 - center_x * self.scale;
        self.offset_y = canvas_h / 2.0 - center_y * self.scale;
    }
}
