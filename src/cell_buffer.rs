/// A single cell in the terminal grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    /// The character displayed in this cell.
    pub ch: char,
    /// Foreground color as RGB tuple, None = default.
    pub fg: Option<(u8, u8, u8)>,
    /// Background color as RGB tuple, None = default.
    pub bg: Option<(u8, u8, u8)>,
    /// Whether this cell is bold.
    pub bold: bool,
    /// Whether this cell is italic.
    pub italic: bool,
    /// Whether this cell is underlined.
    pub underline: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            fg: None,
            bg: None,
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

/// An off-screen rendering buffer of `Cell` values.
///
/// All UI rendering writes to a `CellBuffer` first, then the buffer
/// is diffed against the previous frame and flushed to the terminal.
/// This enables deterministic snapshot testing without a real terminal.
#[derive(Debug, Clone)]
pub struct CellBuffer {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
}

impl CellBuffer {
    /// Create a new buffer filled with default (blank) cells.
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![Cell::default(); width * height],
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Resize the buffer, preserving content that fits.
    pub fn resize(&mut self, width: usize, height: usize) {
        let mut new_cells = vec![Cell::default(); width * height];
        let copy_w = self.width.min(width);
        let copy_h = self.height.min(height);
        for y in 0..copy_h {
            for x in 0..copy_w {
                new_cells[y * width + x] = self.cells[y * self.width + x].clone();
            }
        }
        self.width = width;
        self.height = height;
        self.cells = new_cells;
    }

    /// Get a reference to the cell at (x, y).
    pub fn get(&self, x: usize, y: usize) -> &Cell {
        assert!(x < self.width && y < self.height, "out of bounds: ({x}, {y})");
        &self.cells[y * self.width + x]
    }

    /// Set the cell at (x, y).
    pub fn set(&mut self, x: usize, y: usize, cell: Cell) {
        if x < self.width && y < self.height {
            self.cells[y * self.width + x] = cell;
        }
    }

    /// Write a plain string at (x, y) with the given colors.
    pub fn put_str(
        &mut self,
        x: usize,
        y: usize,
        s: &str,
        fg: Option<(u8, u8, u8)>,
        bg: Option<(u8, u8, u8)>,
    ) {
        let mut col = x;
        for ch in s.chars() {
            if col >= self.width {
                break;
            }
            if y < self.height {
                self.cells[y * self.width + col] = Cell {
                    ch,
                    fg,
                    bg,
                    ..Cell::default()
                };
            }
            let w = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
            col += w;
        }
    }

    /// Fill an entire row with a character and colors.
    pub fn fill_row(&mut self, y: usize, ch: char, fg: Option<(u8, u8, u8)>, bg: Option<(u8, u8, u8)>) {
        if y >= self.height {
            return;
        }
        for x in 0..self.width {
            self.cells[y * self.width + x] = Cell {
                ch,
                fg,
                bg,
                ..Cell::default()
            };
        }
    }

    /// Fill a rectangular area with a character and colors.
    pub fn fill_rect(
        &mut self,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        ch: char,
        fg: Option<(u8, u8, u8)>,
        bg: Option<(u8, u8, u8)>,
    ) {
        for row in y..(y + h).min(self.height) {
            for col in x..(x + w).min(self.width) {
                self.cells[row * self.width + col] = Cell {
                    ch,
                    fg,
                    bg,
                    ..Cell::default()
                };
            }
        }
    }

    /// Clear the entire buffer to default cells.
    pub fn clear(&mut self) {
        self.cells.fill(Cell::default());
    }

    /// Compute the positions that differ between `self` and `other`.
    pub fn diff(&self, other: &CellBuffer) -> Vec<(usize, usize)> {
        assert_eq!(self.width, other.width);
        assert_eq!(self.height, other.height);
        let mut changes = Vec::new();
        for y in 0..self.height {
            for x in 0..self.width {
                let idx = y * self.width + x;
                if self.cells[idx] != other.cells[idx] {
                    changes.push((x, y));
                }
            }
        }
        changes
    }

    /// Generate a plain text snapshot for testing.
    /// Each row is a line; only the `ch` field is included.
    pub fn snapshot(&self) -> String {
        let mut out = String::with_capacity(self.height * (self.width + 1));
        for y in 0..self.height {
            for x in 0..self.width {
                out.push(self.cells[y * self.width + x].ch);
            }
            if y < self.height - 1 {
                out.push('\n');
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_creates_correct_size() {
        let buf = CellBuffer::new(80, 24);
        assert_eq!(buf.width(), 80);
        assert_eq!(buf.height(), 24);
    }

    #[test]
    fn set_get_roundtrip() {
        let mut buf = CellBuffer::new(10, 10);
        let cell = Cell {
            ch: 'A',
            fg: Some((255, 0, 0)),
            bg: None,
            bold: true,
            ..Cell::default()
        };
        buf.set(3, 5, cell.clone());
        assert_eq!(buf.get(3, 5), &cell);
    }

    #[test]
    fn put_str_writes_characters() {
        let mut buf = CellBuffer::new(20, 1);
        buf.put_str(0, 0, "hello", None, None);
        assert_eq!(buf.get(0, 0).ch, 'h');
        assert_eq!(buf.get(1, 0).ch, 'e');
        assert_eq!(buf.get(4, 0).ch, 'o');
        assert_eq!(buf.get(5, 0).ch, ' '); // untouched
    }

    #[test]
    fn put_str_respects_bounds() {
        let mut buf = CellBuffer::new(3, 1);
        buf.put_str(0, 0, "hello", None, None);
        assert_eq!(buf.get(2, 0).ch, 'l');
        // Should not panic — extra chars are clipped
    }

    #[test]
    fn fill_row_fills_entire_row() {
        let mut buf = CellBuffer::new(5, 3);
        buf.fill_row(1, '#', None, None);
        for x in 0..5 {
            assert_eq!(buf.get(x, 1).ch, '#');
        }
        assert_eq!(buf.get(0, 0).ch, ' '); // other rows untouched
    }

    #[test]
    fn fill_rect_fills_area() {
        let mut buf = CellBuffer::new(10, 10);
        buf.fill_rect(2, 2, 3, 3, '*', None, None);
        assert_eq!(buf.get(2, 2).ch, '*');
        assert_eq!(buf.get(4, 4).ch, '*');
        assert_eq!(buf.get(1, 2).ch, ' '); // outside rect
        assert_eq!(buf.get(5, 2).ch, ' '); // outside rect
    }

    #[test]
    fn clear_resets_all_cells() {
        let mut buf = CellBuffer::new(5, 5);
        buf.put_str(0, 0, "test", None, None);
        buf.clear();
        for y in 0..5 {
            for x in 0..5 {
                assert_eq!(buf.get(x, y).ch, ' ');
            }
        }
    }

    #[test]
    fn diff_detects_changes() {
        let buf1 = CellBuffer::new(5, 5);
        let mut buf2 = CellBuffer::new(5, 5);
        buf2.put_str(1, 1, "X", None, None);
        let changes = buf1.diff(&buf2);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0], (1, 1));
    }

    #[test]
    fn diff_empty_when_identical() {
        let buf1 = CellBuffer::new(5, 5);
        let buf2 = buf1.clone();
        assert!(buf1.diff(&buf2).is_empty());
    }

    #[test]
    fn resize_preserves_content() {
        let mut buf = CellBuffer::new(5, 5);
        buf.put_str(0, 0, "hi", None, None);
        buf.resize(10, 10);
        assert_eq!(buf.get(0, 0).ch, 'h');
        assert_eq!(buf.get(1, 0).ch, 'i');
        assert_eq!(buf.width(), 10);
        assert_eq!(buf.height(), 10);
    }

    #[test]
    fn resize_shrinks_correctly() {
        let mut buf = CellBuffer::new(10, 10);
        buf.put_str(0, 0, "hello", None, None);
        buf.resize(3, 3);
        assert_eq!(buf.get(0, 0).ch, 'h');
        assert_eq!(buf.get(2, 0).ch, 'l');
        assert_eq!(buf.width(), 3);
    }

    #[test]
    fn snapshot_matches_expected_layout() {
        let mut buf = CellBuffer::new(5, 2);
        buf.put_str(0, 0, "hello", None, None);
        buf.put_str(0, 1, "world", None, None);
        assert_eq!(buf.snapshot(), "hello\nworld");
    }
}
