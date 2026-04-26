use std::collections::VecDeque;

/// Graph symbol mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphSymbol {
    Braille,
    Block,
    Tty,
}

/// Braille graph characters — 25 entries indexed by [prev_level * 5 + curr_level].
pub const BRAILLE_UP: [&str; 25] = [
    " ", "⢀", "⢠", "⢰", "⢸", "⡀", "⣀", "⣠", "⣰", "⣸", "⡄", "⣄", "⣤", "⣴", "⣼", "⡆",
    "⣆", "⣦", "⣶", "⣾", "⡇", "⣇", "⣧", "⣷", "⣿",
];
pub const BRAILLE_DOWN: [&str; 25] = [
    " ", "⠈", "⠘", "⠸", "⢸", "⠁", "⠉", "⠙", "⠹", "⢹", "⠃", "⠋", "⠛", "⠻", "⢻", "⠇",
    "⠏", "⠟", "⠿", "⢿", "⡇", "⡏", "⡟", "⡿", "⣿",
];
pub const BLOCK_UP: [&str; 25] = [
    " ", "▗", "▗", "▐", "▐", "▖", "▄", "▄", "▟", "▟", "▖", "▄", "▄", "▟", "▟", "▌", "▙", "▙",
    "█", "█", "▌", "▙", "▙", "█", "█",
];
pub const BLOCK_DOWN: [&str; 25] = [
    " ", "▝", "▝", "▐", "▐", "▘", "▀", "▀", "▜", "▜", "▘", "▀", "▀", "▜", "▜", "▌", "▛", "▛",
    "█", "█", "▌", "▛", "▛", "█", "█",
];
pub const TTY_UP: [&str; 25] = [
    " ", "░", "░", "▒", "▒", "░", "░", "▒", "▒", "█", "░", "░", "▒", "▒", "█", "▒", "▒", "▒",
    "█", "█", "█", "█", "█", "█", "█",
];

/// A multi-row terminal graph that renders data as braille/block/tty characters.
///
/// Matches btop's Graph architecture: double-buffered, multi-row with vertical
/// slicing so each row covers a portion of the 0-100% range.
#[derive(Debug, Clone)]
pub struct Graph {
    pub width: usize,
    pub height: usize,
    symbol: GraphSymbol,
    invert: bool,
    no_zero: bool,
    pub max_value: i64,
    offset: i64,
    last: i64,
    /// Double-buffered graph content: each buffer is Vec<String> of `height` rows.
    graphs: [Vec<String>; 2],
    /// Which buffer is current (indexes into `graphs`).
    current: bool,
    /// Name of the color gradient to use (e.g. "cpu", "download").
    color_gradient: String,
}

impl Graph {
    pub fn new(
        width: usize,
        height: usize,
        symbol: GraphSymbol,
        invert: bool,
        no_zero: bool,
        max_value: i64,
        offset: i64,
    ) -> Self {
        let h = height.max(1);
        Self {
            width,
            height: h,
            symbol,
            invert,
            no_zero,
            max_value: if max_value == 0 { 100 } else { max_value },
            offset,
            last: 0,
            graphs: [
                vec![String::new(); h],
                vec![String::new(); h],
            ],
            current: true,
            color_gradient: String::new(),
        }
    }

    /// Set the color gradient name for rendering.
    pub fn set_color_gradient(&mut self, name: &str) {
        self.color_gradient = name.to_string();
    }

    /// Look up the symbol table based on symbol type and invert flag.
    fn table(&self) -> &'static [&'static str; 25] {
        match (self.symbol, self.invert) {
            (GraphSymbol::Braille, false) => &BRAILLE_UP,
            (GraphSymbol::Braille, true) => &BRAILLE_DOWN,
            (GraphSymbol::Block, false) => &BLOCK_UP,
            (GraphSymbol::Block, true) => &BLOCK_DOWN,
            (GraphSymbol::Tty, _) => &TTY_UP,
        }
    }

    /// Look up the graph symbol for a (previous, current) value pair.
    pub fn symbol_at(&self, prev: i64, curr: i64) -> &'static str {
        let max = self.max_value;
        let p = ((prev - self.offset).clamp(0, max) * 4 / max.max(1)) as usize;
        let c = ((curr - self.offset).clamp(0, max) * 4 / max.max(1)) as usize;
        let idx = (p.min(4)) * 5 + c.min(4);
        self.table()[idx.min(24)]
    }

    /// Map a 0-100 value to a 0-4 level within a specific row's vertical range.
    /// For multi-height graphs, each row covers a slice of the full range.
    fn value_to_level(&self, value: i64, row: usize) -> usize {
        let max = self.max_value.max(1);
        let pct = ((value - self.offset).clamp(0, max) as f64 * 100.0 / max as f64).round() as i64;

        if self.height == 1 {
            return (pct * 4 / 100).clamp(0, 4) as usize;
        }

        let horizon = if self.invert { self.height - 1 - row } else { row };
        let cur_high = (100.0 * (self.height - horizon) as f64 / self.height as f64).round() as i64;
        let cur_low = (100.0 * (self.height - (horizon + 1)) as f64 / self.height as f64).round() as i64;

        if pct < cur_low {
            0
        } else if pct >= cur_high {
            4
        } else {
            let range = (cur_high - cur_low).max(1);
            ((pct - cur_low) * 4 / range).clamp(0, 4) as usize
        }
    }

    /// Create the full graph content from data, populating both buffers.
    /// This is the equivalent of btop's `_create` method.
    pub fn create(&mut self, data: &VecDeque<i64>) {
        let len = data.len();
        let h = self.height;

        // Initialize both buffers
        for buf in &mut self.graphs {
            *buf = vec![String::new(); h];
        }

        // We need at least 1 data point
        if len == 0 || self.width == 0 {
            for buf in &mut self.graphs {
                for row_str in buf.iter_mut() {
                    *row_str = " ".repeat(self.width);
                }
            }
            return;
        }

        // Determine the data range we'll render (the last `width` values,
        // or pad with zeros if less data than width)
        let start = if len > self.width { len - self.width } else { 0 };

        for col in 0..self.width {
            let data_idx = start + col;
            let curr = if data_idx < len { data[data_idx] } else { 0 };
            let prev = if data_idx > 0 && data_idx - 1 < len {
                data[data_idx - 1]
            } else if data_idx == 0 && col > 0 {
                // If we're at col > 0 but data_idx is 0, prev is 0
                0
            } else {
                0
            };

            for row in 0..h {
                let prev_level = self.value_to_level(prev, row);
                let curr_level = self.value_to_level(curr, row);
                let idx = prev_level * 5 + curr_level;
                let sym = self.table()[idx.min(24)];
                self.graphs[self.current as usize][row].push_str(sym);
            }

            self.last = curr;
        }

        // Copy current buffer to the other for double buffering
        let other = !self.current;
        self.graphs[other as usize] = self.graphs[self.current as usize].clone();
    }

    /// Update the graph by shifting old data left and appending one new column.
    /// Matches btop's `operator()` behavior.
    pub fn update(&mut self, data: &VecDeque<i64>) {
        let len = data.len();
        if len == 0 {
            return;
        }

        // Swap buffers
        self.current = !self.current;
        let other = !self.current;

        let curr = *data.back().unwrap_or(&0);
        let prev = if len >= 2 { data[len - 2] } else { self.last };

        for row in 0..self.height {
            let prev_level = self.value_to_level(prev, row);
            let curr_level = self.value_to_level(curr, row);
            let idx = prev_level * 5 + curr_level;
            let sym = self.table()[idx.min(24)];

            // Start from the other buffer, remove first char, append new char
            let other_row = &self.graphs[other as usize][row];
            let mut new_row = String::with_capacity(self.width * 4);
            let mut chars = other_row.chars();
            chars.next(); // Remove first character (shift left)
            new_row.push_str(chars.as_str());
            new_row.push_str(sym);
            self.graphs[self.current as usize][row] = new_row;
        }

        self.last = curr;
    }

    /// Render the multi-row graph with per-row gradient colors for multi-height,
    /// or per-column gradient colors for single-height.
    /// Returns Vec<String> where each element is one row with ANSI color codes.
    pub fn render_rows_colored(&self, data: &VecDeque<i64>, gradient: &[String]) -> Vec<String> {
        let h = self.height;
        let buf = &self.graphs[self.current as usize];
        let mut rows = Vec::with_capacity(h);

        if h == 1 {
            // Single-height: color per column based on data value
            let mut row_str = String::with_capacity(self.width * 20);
            let len = data.len();
            let start = if len > self.width { len - self.width } else { 0 };
            let chars: Vec<char> = buf.get(0).map(|s| s.chars().collect()).unwrap_or_default();

            for (col, ch) in chars.iter().enumerate() {
                let data_idx = start + col;
                let curr = if data_idx < len { data[data_idx] } else { 0 };
                let prev = if data_idx > 0 && data_idx.checked_sub(1).map(|i| i < len).unwrap_or(false) {
                    data[data_idx - 1]
                } else {
                    0
                };
                let max_val = curr.max(prev);
                if !gradient.is_empty() {
                    let max = self.max_value.max(1);
                    let pct = ((max_val - self.offset).clamp(0, max) * 100 / max) as usize;
                    row_str.push_str(&gradient[pct.min(100)]);
                }
                row_str.push(*ch);
            }
            row_str.push_str("\x1b[0m");
            rows.push(row_str);
        } else {
            // Multi-height: each row gets a single gradient color based on vertical position
            for row in 0..h {
                let mut row_str = String::with_capacity(self.width * 6);
                if !gradient.is_empty() {
                    let color_idx = if self.invert {
                        // Inverted: row 0 is bottom (low %), row h-1 is top (high %)
                        row * 100 / h
                    } else {
                        // Normal: row 0 is top (high %), row h-1 is bottom (low %)
                        100 - ((row + 1) * 100 / h)
                    };
                    row_str.push_str(&gradient[color_idx.min(100)]);
                }
                if let Some(r) = buf.get(row) {
                    row_str.push_str(r);
                }
                row_str.push_str("\x1b[0m");
                rows.push(row_str);
            }
        }

        rows
    }

    /// Render the graph as positioned ANSI output at the given (x, y) coordinates.
    /// Each row is placed at consecutive y positions.
    pub fn render_at(&self, x: usize, y: usize, data: &VecDeque<i64>, gradient: &[String]) -> String {
        let rows = self.render_rows_colored(data, gradient);
        let mut out = String::new();
        for (i, row) in rows.iter().enumerate() {
            out.push_str(&format!("\x1b[{};{}H{}", y + 1 + i, x + 1, row));
        }
        out
    }

    /// Render a single row of graph data into a string of graph symbols.
    /// For backward compatibility and single-height graphs.
    pub fn render_row(&mut self, data: &VecDeque<i64>) -> String {
        self.create(data);
        self.graphs[self.current as usize]
            .first()
            .cloned()
            .unwrap_or_default()
    }

    /// Render a row with gradient colors applied per column.
    /// For backward compatibility with single-height graphs.
    pub fn render_row_colored(&mut self, data: &VecDeque<i64>, gradient: &[String]) -> String {
        self.create(data);
        let rows = self.render_rows_colored(data, gradient);
        rows.into_iter().next().unwrap_or_default()
    }

    /// Get the raw (uncolored) row strings from the current buffer.
    pub fn rows(&self) -> &[String] {
        &self.graphs[self.current as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn braille_symbol_lookup_correct() {
        let graph = Graph::new(10, 1, GraphSymbol::Braille, false, false, 100, 0);
        assert_eq!(graph.symbol_at(0, 0), " ");
        assert_eq!(graph.symbol_at(100, 100), "⣿");
    }

    #[test]
    fn block_symbol_lookup_correct() {
        let graph = Graph::new(10, 1, GraphSymbol::Block, false, false, 100, 0);
        assert_eq!(graph.symbol_at(0, 0), " ");
        assert_eq!(graph.symbol_at(100, 100), "█");
    }

    #[test]
    fn tty_symbol_lookup_correct() {
        let graph = Graph::new(10, 1, GraphSymbol::Tty, false, false, 100, 0);
        assert_eq!(graph.symbol_at(0, 0), " ");
        assert_eq!(graph.symbol_at(100, 100), "█");
    }

    #[test]
    fn render_single_value_100_percent() {
        let mut graph = Graph::new(1, 1, GraphSymbol::Braille, false, false, 100, 0);
        let data: VecDeque<i64> = vec![100].into();
        let result = graph.render_row(&data);
        assert_eq!(result, "⢸"); // prev=0, curr=100 → level 0*5+4 = index 4
    }

    #[test]
    fn render_single_value_0_percent() {
        let mut graph = Graph::new(1, 1, GraphSymbol::Braille, false, false, 100, 0);
        let data: VecDeque<i64> = vec![0].into();
        let result = graph.render_row(&data);
        assert_eq!(result, " ");
    }

    #[test]
    fn render_inverted_flips_direction() {
        let graph_up = Graph::new(1, 1, GraphSymbol::Braille, false, false, 100, 0);
        let graph_down = Graph::new(1, 1, GraphSymbol::Braille, true, false, 100, 0);
        let s_up = graph_up.symbol_at(0, 50);
        let s_down = graph_down.symbol_at(0, 50);
        assert_ne!(s_up, s_down);
    }

    #[test]
    fn render_width_matches_data_length() {
        let mut graph = Graph::new(5, 1, GraphSymbol::Block, false, false, 100, 0);
        let data: VecDeque<i64> = vec![10, 20, 30, 40, 50].into();
        let result = graph.render_row(&data);
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn render_max_value_clamping() {
        let graph = Graph::new(1, 1, GraphSymbol::Braille, false, false, 100, 0);
        assert_eq!(graph.symbol_at(200, 200), "⣿");
    }

    #[test]
    fn render_offset_subtracted() {
        let graph = Graph::new(1, 1, GraphSymbol::Braille, false, false, 100, 50);
        assert_eq!(graph.symbol_at(50, 50), " ");
        assert_eq!(graph.symbol_at(150, 150), "⣿");
    }

    #[test]
    fn multi_height_creates_correct_row_count() {
        let mut graph = Graph::new(10, 3, GraphSymbol::Braille, false, false, 100, 0);
        let data: VecDeque<i64> = (0..10).map(|i| i * 10).collect();
        graph.create(&data);
        assert_eq!(graph.rows().len(), 3);
        for row in graph.rows() {
            assert_eq!(row.chars().count(), 10);
        }
    }

    #[test]
    fn multi_height_full_value_fills_all_rows() {
        let mut graph = Graph::new(5, 3, GraphSymbol::Braille, false, false, 100, 0);
        let data: VecDeque<i64> = vec![100, 100, 100, 100, 100].into();
        graph.create(&data);
        // All rows should have non-space characters for 100% values
        for row in graph.rows() {
            assert!(!row.chars().all(|c| c == ' '), "row should not be all spaces for 100%");
        }
    }

    #[test]
    fn multi_height_zero_value_empty_rows() {
        let mut graph = Graph::new(5, 3, GraphSymbol::Braille, false, false, 100, 0);
        let data: VecDeque<i64> = vec![0, 0, 0, 0, 0].into();
        graph.create(&data);
        for row in graph.rows() {
            assert!(row.chars().all(|c| c == ' '), "rows should be all spaces for 0%");
        }
    }

    #[test]
    fn update_shifts_graph_left() {
        let mut graph = Graph::new(5, 1, GraphSymbol::Braille, false, false, 100, 0);
        let data: VecDeque<i64> = vec![0, 0, 0, 0, 50].into();
        graph.create(&data);
        let row_before = graph.rows()[0].clone();

        let data2: VecDeque<i64> = vec![0, 0, 0, 50, 100].into();
        graph.update(&data2);
        let row_after = graph.rows()[0].clone();

        assert_ne!(row_before, row_after);
        assert_eq!(row_after.chars().count(), 5);
    }

    #[test]
    fn render_rows_colored_returns_correct_count() {
        let mut graph = Graph::new(10, 4, GraphSymbol::Braille, false, false, 100, 0);
        let data: VecDeque<i64> = (0..10).map(|i| i * 10).collect();
        graph.create(&data);
        let gradient: Vec<String> = (0..=100).map(|_| "\x1b[38;2;128;128;128m".to_string()).collect();
        let rows = graph.render_rows_colored(&data, &gradient);
        assert_eq!(rows.len(), 4);
    }
}
