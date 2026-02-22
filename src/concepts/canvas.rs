use crate::concepts::scene_map::SceneMap;
use crate::concepts::text_buffer::TextBuffer;

#[derive(Debug)]
pub struct StyledLine {
    pub text: String,
    pub is_scene_boundary: bool,
    pub is_streaming_target: bool,
    pub line_number: usize,
}

#[derive(Debug)]
pub struct StreamState {
    pub position: usize,
    pub active: bool,
}

pub struct Canvas {
    scroll_offset: usize,
    viewport_height: usize,
    viewport_width: usize,
    render_cache: Vec<StyledLine>,
    streaming: Option<StreamState>,
}

impl Canvas {
    pub fn new(viewport_height: usize, viewport_width: usize) -> Self {
        Self {
            scroll_offset: 0,
            viewport_height,
            viewport_width,
            render_cache: Vec::new(),
            streaming: None,
        }
    }

    /// Render the visible portion of the text buffer, marking scene boundaries
    pub fn render(&mut self, text_buffer: &TextBuffer, scene_map: &SceneMap) {
        self.render_cache.clear();
        let full_text = text_buffer.read_all();
        let scenes = scene_map.list_scenes();

        let stream_pos = self.streaming.as_ref().map(|s| s.position);

        for (line_num, line_text) in full_text.lines().enumerate() {
            // Check if this line is at a scene boundary
            let line_byte_start = full_text
                .lines()
                .take(line_num)
                .map(|l| l.len() + 1) // +1 for newline
                .sum::<usize>();

            let is_scene_boundary = scenes.iter().any(|s| {
                s.start == line_byte_start
            });

            let is_streaming_target = stream_pos.map_or(false, |pos| {
                pos >= line_byte_start && pos < line_byte_start + line_text.len() + 1
            });

            // Truncate to viewport width
            let display_text = if line_text.len() > self.viewport_width {
                &line_text[..self.viewport_width]
            } else {
                line_text
            };

            self.render_cache.push(StyledLine {
                text: display_text.to_string(),
                is_scene_boundary,
                is_streaming_target,
                line_number: line_num + 1,
            });
        }
    }

    /// Get the visible lines (applying scroll offset)
    pub fn visible_lines(&self) -> &[StyledLine] {
        let start = self.scroll_offset.min(self.render_cache.len());
        let end = (start + self.viewport_height).min(self.render_cache.len());
        &self.render_cache[start..end]
    }

    /// Get all rendered lines (for testing)
    pub fn all_lines(&self) -> &[StyledLine] {
        &self.render_cache
    }

    /// Start streaming at a byte position
    pub fn begin_stream(&mut self, position: usize) {
        self.streaming = Some(StreamState {
            position,
            active: true,
        });
    }

    /// Process a stream chunk. Returns (position, text) for the caller to insert
    /// into the TextBuffer (S3.2 sync — Canvas holds no text).
    pub fn stream_chunk(&mut self, text: &str) -> Option<(usize, String)> {
        if let Some(ref mut state) = self.streaming {
            if !state.active {
                return None;
            }
            let pos = state.position;
            state.position += text.len();
            Some((pos, text.to_string()))
        } else {
            None
        }
    }

    /// End the current stream
    pub fn end_stream(&mut self) {
        if let Some(ref mut state) = self.streaming {
            state.active = false;
        }
        self.streaming = None;
    }

    /// Scroll to make a specific scene visible
    pub fn scroll_to_scene(&mut self, scene_map: &SceneMap, text_buffer: &TextBuffer, scene_id: &str) {
        if let Some(scene) = scene_map.get_scene(scene_id) {
            let full_text = text_buffer.read_all();
            // Count lines before the scene start
            let line_num = full_text[..scene.start]
                .lines()
                .count();
            self.scroll_offset = line_num;
        }
    }

    /// Scroll by a delta (positive = down, negative = up)
    pub fn scroll(&mut self, delta: isize) {
        let new_offset = self.scroll_offset as isize + delta;
        self.scroll_offset = new_offset.max(0) as usize;
        // Clamp to valid range
        let max_offset = self.render_cache.len().saturating_sub(self.viewport_height);
        self.scroll_offset = self.scroll_offset.min(max_offset);
    }

    /// Get current scroll offset
    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    /// Check if streaming is active
    pub fn is_streaming(&self) -> bool {
        self.streaming.as_ref().map_or(false, |s| s.active)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::concepts::scene_map::ParseMode;
    use std::path::PathBuf;

    #[test]
    fn test_canvas_render_basic() {
        let text = "Line one\nLine two\nLine three\n";
        let text_buffer = TextBuffer::from_str(text, PathBuf::from("/tmp/test.md"));
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text);

        let mut canvas = Canvas::new(10, 80);
        canvas.render(&text_buffer, &scene_map);

        let lines = canvas.all_lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text, "Line one");
        assert_eq!(lines[0].line_number, 1);
        assert_eq!(lines[1].text, "Line two");
        assert_eq!(lines[2].text, "Line three");
    }

    #[test]
    fn test_canvas_scene_boundary_highlighting() {
        let text = "## Scene 1\n\nFirst scene with enough words to be a real scene.\n\n## Scene 2\n\nSecond scene with enough words to be a real scene.";
        let text_buffer = TextBuffer::from_str(text, PathBuf::from("/tmp/test.md"));
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text);

        let mut canvas = Canvas::new(20, 80);
        canvas.render(&text_buffer, &scene_map);

        let lines = canvas.all_lines();
        // At least one line should be marked as a scene boundary
        let boundary_count = lines.iter().filter(|l| l.is_scene_boundary).count();
        assert!(boundary_count >= 1, "Expected at least 1 scene boundary, got {boundary_count}");
    }

    #[test]
    fn test_canvas_stream_lifecycle() {
        let mut canvas = Canvas::new(10, 80);

        assert!(!canvas.is_streaming());

        canvas.begin_stream(100);
        assert!(canvas.is_streaming());

        let chunk1 = canvas.stream_chunk("Hello ");
        assert_eq!(chunk1, Some((100, "Hello ".to_string())));

        let chunk2 = canvas.stream_chunk("world");
        assert_eq!(chunk2, Some((106, "world".to_string())));

        canvas.end_stream();
        assert!(!canvas.is_streaming());

        let chunk3 = canvas.stream_chunk("after");
        assert_eq!(chunk3, None);
    }

    #[test]
    fn test_canvas_scroll_to_scene() {
        let text = "## Scene 1\n\nFirst scene with enough words to be a real scene here.\n\n## Scene 2\n\nSecond scene with enough words to be a real scene here.";
        let text_buffer = TextBuffer::from_str(text, PathBuf::from("/tmp/test.md"));
        let mut scene_map = SceneMap::new(ParseMode::Prose);
        scene_map.full_reindex(text);

        let mut canvas = Canvas::new(3, 80);
        canvas.render(&text_buffer, &scene_map);

        let scene2_id = scene_map.list_scenes()[1].id.clone();
        canvas.scroll_to_scene(&scene_map, &text_buffer, &scene2_id);

        // After scrolling to scene 2, scroll_offset should be > 0
        assert!(canvas.scroll_offset() > 0, "Expected scroll offset > 0 after scrolling to scene 2");
    }
}
