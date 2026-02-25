use sequoia_shared::colors::{hsl_to_rgb, interpolate_hsl, rgb_to_hsl};

/// A color transition animation for territory ownership changes.
#[derive(Debug, Clone)]
pub struct ColorTransition {
    pub from_hsl: (f64, f64, f64),
    pub to_hsl: (f64, f64, f64),
    pub start_time: f64,
    pub duration: f64, // milliseconds
}

impl ColorTransition {
    pub fn new(from: (u8, u8, u8), to: (u8, u8, u8), start_time: f64, duration: f64) -> Self {
        Self {
            from_hsl: rgb_to_hsl(from.0, from.1, from.2),
            to_hsl: rgb_to_hsl(to.0, to.1, to.2),
            start_time,
            duration,
        }
    }

    /// Returns the current interpolated color as (r, g, b), or None if animation is complete.
    pub fn current_color(&self, now: f64) -> Option<(u8, u8, u8)> {
        let elapsed = now - self.start_time;
        if elapsed >= self.duration {
            return None;
        }

        let t = cubic_ease_out(elapsed / self.duration);
        let hsl = interpolate_hsl(self.from_hsl, self.to_hsl, t);
        Some(hsl_to_rgb(hsl.0, hsl.1, hsl.2))
    }

    /// Returns the flash intensity (0.0..1.0) for the initial flash overlay.
    /// Flash duration scales with transition duration: 25% of duration, capped at 200ms.
    pub fn flash_intensity(&self, now: f64) -> f64 {
        let flash_duration = (self.duration * 0.25).min(200.0);
        let elapsed = now - self.start_time;
        if elapsed >= flash_duration {
            return 0.0;
        }
        let t = elapsed / flash_duration;
        (1.0 - t) * 0.6
    }
}

/// Cubic ease-out: decelerating to zero velocity.
fn cubic_ease_out(t: f64) -> f64 {
    let t = t - 1.0;
    t * t * t + 1.0
}
