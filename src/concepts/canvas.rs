pub struct Canvas {
    scroll_offset: usize,
}

impl Canvas {
    pub fn new(_viewport_height: usize, _viewport_width: usize) -> Self {
        Self { scroll_offset: 0 }
    }

    /// Scroll by a delta (positive = down, negative = up).
    pub fn scroll(&mut self, delta: isize) {
        let new_offset = self.scroll_offset as isize + delta;
        self.scroll_offset = new_offset.max(0) as usize;
    }

    /// Get current scroll offset.
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }
}

#[cfg(test)]
mod tests {
    use super::Canvas;

    #[test]
    fn canvas_scroll_never_goes_negative() {
        let mut canvas = Canvas::new(10, 80);
        canvas.scroll(-10);
        assert_eq!(canvas.scroll_offset(), 0);
    }

    #[test]
    fn canvas_scroll_accumulates_positive_delta() {
        let mut canvas = Canvas::new(10, 80);
        canvas.scroll(3);
        canvas.scroll(2);
        assert_eq!(canvas.scroll_offset(), 5);
    }
}
