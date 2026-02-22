use chrono::{DateTime, Utc};
use ropey::Rope;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use crate::error::{LairesError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEntry {
    pub range: ByteRange,
    pub kind: ChangeKind,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ByteRange {
    pub start: usize,
    pub end: usize,
}

impl ByteRange {
    pub fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }

    /// Check if two ranges overlap
    pub fn overlaps(&self, other: &ByteRange) -> bool {
        self.start < other.end && other.start < self.end
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ChangeKind {
    Insert,
    Delete,
    Replace,
}

pub struct TextBuffer {
    rope: Rope,
    file_path: PathBuf,
    dirty: bool,
    change_log: Vec<ChangeEntry>,
}

impl TextBuffer {
    /// Create a new empty TextBuffer
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            rope: Rope::new(),
            file_path,
            dirty: false,
            change_log: Vec::new(),
        }
    }

    /// Load text from the file on disk
    pub fn load(&mut self) -> Result<()> {
        let content = if self.file_path.extension().and_then(|e| e.to_str()) == Some("docx") {
            crate::concepts::docx::extract_text_from_docx(&self.file_path)?
        } else {
            std::fs::read_to_string(&self.file_path).map_err(|_| {
                LairesError::StoryFileNotFound(self.file_path.display().to_string())
            })?
        };
        self.rope = Rope::from_str(&content);
        self.dirty = false;
        self.change_log.clear();
        Ok(())
    }

    /// Create a TextBuffer from a file path, loading its contents
    pub fn from_file(file_path: PathBuf) -> Result<Self> {
        let mut buf = Self::new(file_path);
        buf.load()?;
        Ok(buf)
    }

    /// Create a TextBuffer from a string (for testing or in-memory use)
    pub fn from_str(text: &str, file_path: PathBuf) -> Self {
        Self {
            rope: Rope::from_str(text),
            file_path,
            dirty: false,
            change_log: Vec::new(),
        }
    }

    /// Insert text at a byte offset
    pub fn insert(&mut self, position: usize, text: &str) -> Result<()> {
        if position > self.rope.len_bytes() {
            return Err(LairesError::InvalidRange {
                start: position,
                end: position,
            });
        }

        let char_idx = self.rope.byte_to_char(position);
        self.rope.insert(char_idx, text);

        self.change_log.push(ChangeEntry {
            range: ByteRange::new(position, position + text.len()),
            kind: ChangeKind::Insert,
            timestamp: Utc::now(),
        });
        self.dirty = true;
        Ok(())
    }

    /// Delete text in a byte range
    pub fn delete(&mut self, range: ByteRange) -> Result<()> {
        if range.end > self.rope.len_bytes() {
            return Err(LairesError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }

        let char_start = self.rope.byte_to_char(range.start);
        let char_end = self.rope.byte_to_char(range.end);
        self.rope.remove(char_start..char_end);

        self.change_log.push(ChangeEntry {
            range,
            kind: ChangeKind::Delete,
            timestamp: Utc::now(),
        });
        self.dirty = true;
        Ok(())
    }

    /// Replace text in a byte range with new text
    pub fn replace(&mut self, range: ByteRange, text: &str) -> Result<()> {
        if range.end > self.rope.len_bytes() {
            return Err(LairesError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }

        let char_start = self.rope.byte_to_char(range.start);
        let char_end = self.rope.byte_to_char(range.end);
        self.rope.remove(char_start..char_end);
        self.rope.insert(char_start, text);

        self.change_log.push(ChangeEntry {
            range: ByteRange::new(range.start, range.start + text.len()),
            kind: ChangeKind::Replace,
            timestamp: Utc::now(),
        });
        self.dirty = true;
        Ok(())
    }

    /// Read text from a byte range
    pub fn read(&self, range: ByteRange) -> Result<String> {
        if range.end > self.rope.len_bytes() {
            return Err(LairesError::InvalidRange {
                start: range.start,
                end: range.end,
            });
        }

        let char_start = self.rope.byte_to_char(range.start);
        let char_end = self.rope.byte_to_char(range.end);
        Ok(self.rope.slice(char_start..char_end).to_string())
    }

    /// Read the entire rope content
    pub fn read_all(&self) -> String {
        self.rope.to_string()
    }

    /// Save the rope content to disk
    pub fn save(&mut self) -> Result<()> {
        let content = self.rope.to_string();
        std::fs::write(&self.file_path, &content)?;
        self.dirty = false;
        Ok(())
    }

    /// Return accumulated changes and reset the log
    pub fn checkpoint(&mut self) -> Vec<ChangeEntry> {
        std::mem::take(&mut self.change_log)
    }

    /// Get the total byte length of the text
    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.rope.len_bytes() == 0
    }

    /// Check if there are unsaved changes
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Get the file path
    pub fn file_path(&self) -> &Path {
        &self.file_path
    }

    /// Get word count (approximate)
    pub fn word_count(&self) -> usize {
        self.rope.to_string().split_whitespace().count()
    }

    /// Get line count
    pub fn line_count(&self) -> usize {
        self.rope.len_lines()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_buffer(text: &str) -> TextBuffer {
        TextBuffer::from_str(text, PathBuf::from("/tmp/test.md"))
    }

    #[test]
    fn test_insert() {
        let mut buf = test_buffer("hello world");
        buf.insert(5, " beautiful").unwrap();
        assert_eq!(buf.read_all(), "hello beautiful world");
        assert!(buf.is_dirty());
    }

    #[test]
    fn test_delete() {
        let mut buf = test_buffer("hello beautiful world");
        buf.delete(ByteRange::new(5, 15)).unwrap();
        assert_eq!(buf.read_all(), "hello world");
    }

    #[test]
    fn test_replace() {
        let mut buf = test_buffer("hello world");
        buf.replace(ByteRange::new(6, 11), "rust").unwrap();
        assert_eq!(buf.read_all(), "hello rust");
    }

    #[test]
    fn test_read_range() {
        let buf = test_buffer("hello world");
        assert_eq!(buf.read(ByteRange::new(0, 5)).unwrap(), "hello");
    }

    #[test]
    fn test_checkpoint_clears_log() {
        let mut buf = test_buffer("hello");
        buf.insert(5, " world").unwrap();
        let changes = buf.checkpoint();
        assert_eq!(changes.len(), 1);
        let changes2 = buf.checkpoint();
        assert!(changes2.is_empty());
    }

    #[test]
    fn test_invalid_range() {
        let buf = test_buffer("hello");
        assert!(buf.read(ByteRange::new(0, 100)).is_err());
    }

    #[test]
    fn test_load_docx_file() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/character_bible.docx");
        if !path.exists() {
            eprintln!("Skipping: test file not found at {}", path.display());
            return;
        }
        let buf = TextBuffer::from_file(path).unwrap();
        assert!(!buf.is_empty(), "docx buffer should not be empty");
        assert!(buf.word_count() > 100, "expected >100 words from docx");
    }
}
