use sdl2::rect::Rect;
use std::time::{Duration, Instant};

/// Animation that plays when text is copied (Ctrl+Shift+C)
/// Shows the selection area expanding and fading out
#[derive(Clone)]
pub struct CopyAnimation {
    /// Original selection rectangle (before expansion)
    pub original_rect: Rect,
    /// When the animation started
    pub start_time: Instant,
    /// Total duration of the animation
    pub duration: Duration,
    /// Maximum expansion in pixels
    pub max_expansion: f32,
}

impl CopyAnimation {
    /// Create a new copy animation
    pub fn new(rect: Rect) -> Self {
        Self {
            original_rect: rect,
            start_time: Instant::now(),
            duration: Duration::from_millis(300), // 300ms animation
            max_expansion: 30.0,                  // Grow by 30 pixels
        }
    }

    /// Get the current progress (0.0 to 1.0)
    pub fn progress(&self) -> f32 {
        let elapsed = self.start_time.elapsed();
        let progress = elapsed.as_secs_f32() / self.duration.as_secs_f32();
        progress.min(1.0)
    }

    /// Check if the animation is complete
    pub fn is_complete(&self) -> bool {
        self.progress() >= 1.0
    }

    /// Get the current expanded rectangle
    pub fn current_rect(&self) -> Rect {
        let progress = self.progress();
        let expansion = self.max_expansion * progress;

        Rect::new(
            self.original_rect.x() - expansion as i32,
            self.original_rect.y() - expansion as i32,
            (self.original_rect.width() as f32 + expansion * 2.0) as u32,
            (self.original_rect.height() as f32 + expansion * 2.0) as u32,
        )
    }

    /// Get the current opacity (255 to 0, nearly transparent at the end)
    pub fn current_opacity(&self) -> u8 {
        let progress = self.progress();
        // Start at full opacity, fade to nearly transparent (but not completely 0)
        let opacity = 255.0 * (1.0 - progress);
        opacity.max(10.0) as u8 // Minimum 10 for "nearly transparent"
    }

    /// Get the corner radius for rounded corners (increases with expansion)
    pub fn corner_radius(&self) -> i16 {
        let progress = self.progress();
        (8.0 + progress * 4.0) as i16 // Start at 8px, grow to 12px
    }
}
