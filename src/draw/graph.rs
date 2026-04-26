use std::collections::VecDeque;

/// Graph symbol mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // variants used in tests
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

/// Skip the first visual graph element in a row string.
/// Elements can be either a single Unicode character or an ANSI escape sequence like `\x1b[1C`.
#[allow(dead_code)] // used by update() which is tested
fn skip_first_graph_element(s: &str) -> &str {
    if let Some(stripped) = s.strip_prefix('\x1b') {
        // Find the end of the escape sequence (letter terminates CSI sequences)
        if let Some(pos) = stripped.find(|c: char| c.is_ascii_alphabetic()) {
            return &stripped[pos + 1..];
        }
        return s;
    }
    // Skip one Unicode character
    let mut chars = s.chars();
    if chars.next().is_some() {
        return chars.as_str();
    }
    s
}

/// Parse a graph buffer row into individual visual elements.
/// Each element is either a cursor-right escape sequence or a single graph character.
fn parse_graph_elements(s: &str) -> Vec<&str> {
    let mut elements = Vec::new();
    let mut remaining = s;
    while !remaining.is_empty() {
        if remaining.starts_with('\x1b') {
            // Cursor-right escape: \x1b[<digits>C or similar CSI sequence
            if let Some(pos) = remaining[1..].find(|c: char| c.is_ascii_alphabetic()) {
                let end = 1 + pos + 1;
                // A padding escape like \x1b[5C represents 5 columns; expand to 5 elements
                if remaining.as_bytes().get(end - 1) == Some(&b'C') {
                    // Parse the number between '[' and 'C'
                    if let Some(num_str) = remaining.get(2..end - 1) {
                        if let Ok(count) = num_str.parse::<usize>() {
                            let esc = &remaining[..end];
                            // For multi-column cursor-right, emit individual \x1b[1C elements
                            if count > 1 {
                                for _ in 0..count {
                                    elements.push("\x1b[1C" as &str);
                                }
                            } else {
                                elements.push(esc);
                            }
                            remaining = &remaining[end..];
                            continue;
                        }
                    }
                }
                elements.push(&remaining[..end]);
                remaining = &remaining[end..];
            } else {
                elements.push(remaining);
                break;
            }
        } else {
            // Single Unicode character
            let ch = remaining.chars().next().unwrap();
            let ch_len = ch.len_utf8();
            elements.push(&remaining[..ch_len]);
            remaining = &remaining[ch_len..];
        }
    }
    elements
}

/// A multi-row terminal graph that renders data as braille/block/tty characters.
///
/// Matches btop's Graph architecture: double-buffered, multi-row with vertical
/// slicing so each row covers a portion of the 0-100% range.
#[derive(Debug, Clone)]
#[allow(dead_code)] // fields/methods used in tests and as UI grows
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

#[allow(dead_code)]
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

        if self.width == 0 {
            return;
        }

        // If less data than width, pad left with cursor-right moves (like btop's Mv::r)
        let pad_cols = self.width.saturating_sub(len);
        if pad_cols > 0 {
            let pad = format!("\x1b[{}C", pad_cols);
            for row_str in self.graphs[self.current as usize].iter_mut() {
                row_str.push_str(&pad);
            }
        }

        if len == 0 {
            let other = !self.current;
            self.graphs[other as usize] = self.graphs[self.current as usize].clone();
            return;
        }

        // Render the actual data columns (last `width` values, or all if less)
        let data_start = len.saturating_sub(self.width);
        let data_cols = len.min(self.width);

        for di in 0..data_cols {
            let data_idx = data_start + di;
            let curr = data[data_idx];
            let prev = if data_idx > 0 {
                data[data_idx - 1]
            } else {
                0
            };

            for row in 0..h {
                let mut prev_level = self.value_to_level(prev, row);
                let mut curr_level = self.value_to_level(curr, row);

                // btop line 425: no_zero clamps the bottom row's minimum to 1
                if self.no_zero && row == h - 1 {
                    if prev_level == 0 { prev_level = 1; }
                    if curr_level == 0 { curr_level = 1; }
                }

                // btop line 436: single-height with both 0 → cursor right instead of space
                if h == 1 && prev_level + curr_level == 0 {
                    self.graphs[self.current as usize][row].push_str("\x1b[1C");
                } else {
                    let idx = prev_level * 5 + curr_level;
                    let sym = self.table()[idx.min(24)];
                    self.graphs[self.current as usize][row].push_str(sym);
                }
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
        let h = self.height;

        for row in 0..h {
            let mut prev_level = self.value_to_level(prev, row);
            let mut curr_level = self.value_to_level(curr, row);

            if self.no_zero && row == h - 1 {
                if prev_level == 0 { prev_level = 1; }
                if curr_level == 0 { curr_level = 1; }
            }

            let new_sym = if h == 1 && prev_level + curr_level == 0 {
                "\x1b[1C"
            } else {
                let idx = prev_level * 5 + curr_level;
                self.table()[idx.min(24)]
            };

            // Start from the other buffer, remove first visible element, append new
            let other_row = &self.graphs[other as usize][row];
            let mut new_row = String::with_capacity(self.width * 4);
            // Skip the first visual element (could be a char or an escape sequence like \x1b[1C)
            let trimmed = skip_first_graph_element(other_row);
            new_row.push_str(trimmed);
            new_row.push_str(new_sym);
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
            // Single-height: color per column based on data value.
            // The buffer now contains a mix of escape sequences (\x1b[...C for cursor-right)
            // and graph characters. We iterate over "graph elements" instead of chars.
            let mut row_str = String::with_capacity(self.width * 20);
            let len = data.len();
            let raw = buf.first().map(|s| s.as_str()).unwrap_or("");

            // Parse the buffer into visual elements
            let elements = parse_graph_elements(raw);

            let pad_cols = self.width.saturating_sub(len);
            let data_start = len.saturating_sub(self.width);

            for (col, elem) in elements.iter().enumerate() {
                if col < pad_cols {
                    // Padding column — emit as-is (cursor-right escape)
                    row_str.push_str(elem);
                    continue;
                }
                let data_idx = data_start + (col - pad_cols);
                let curr = if data_idx < len { data[data_idx] } else { 0 };
                let prev = if data_idx > 0 && data_idx < len {
                    data[data_idx - 1]
                } else {
                    0
                };
                let max_val = curr.max(prev);
                // Only apply color to actual graph symbols, not cursor-right escapes
                let is_escape = elem.starts_with('\x1b');
                if !is_escape && !gradient.is_empty() {
                    let max = self.max_value.max(1);
                    let pct = ((max_val - self.offset).clamp(0, max) * 100 / max) as usize;
                    row_str.push_str(&gradient[pct.min(100)]);
                }
                row_str.push_str(elem);
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
        // Single-height with both prev and curr 0 outputs cursor-right escape
        assert_eq!(result, "\x1b[1C");
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
        // All non-zero, so no cursor-right escapes; should be 5 visible chars
        let elems = parse_graph_elements(&result);
        assert_eq!(elems.len(), 5);
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
        // No padding (10 data points for width 10), each row has 10 graph chars
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
        let elems = parse_graph_elements(&row_after);
        assert_eq!(elems.len(), 5);
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
