use std::collections::VecDeque;

/// Graph symbol mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GraphSymbol {
    Braille,
    Block,
}

impl GraphSymbol {
    /// Parse a config string like "braille", "block", or "default" into a GraphSymbol.
    /// If `specific` is "default", falls back to `global`.
    pub fn from_config(specific: &str, global: &str) -> Self {
        let s = if specific == "default" {
            global
        } else {
            specific
        };
        match s {
            "block" => Self::Block,
            _ => Self::Braille,
        }
    }
}

/// Braille graph characters — 25 entries indexed by [prev_level * 5 + curr_level].
pub const BRAILLE_UP: [&str; 25] = [
    " ", "⢀", "⢠", "⢰", "⢸", "⡀", "⣀", "⣠", "⣰", "⣸", "⡄", "⣄", "⣤", "⣴", "⣼", "⡆", "⣆", "⣦", "⣶",
    "⣾", "⡇", "⣇", "⣧", "⣷", "⣿",
];
/// Braille graph characters for inverted (bottom-up) graphs.
pub const BRAILLE_DOWN: [&str; 25] = [
    " ", "⠈", "⠘", "⠸", "⢸", "⠁", "⠉", "⠙", "⠹", "⢹", "⠃", "⠋", "⠛", "⠻", "⢻", "⠇", "⠏", "⠟", "⠿",
    "⢿", "⡇", "⡏", "⡟", "⡿", "⣿",
];
/// Block graph characters — 5 levels from empty to full.
const BLOCK_UP: [&str; 5] = [" ", "▄", "▄", "▀", "█"];
/// Block graph characters for inverted graphs.
const BLOCK_DOWN: [&str; 5] = [" ", "▀", "▀", "▄", "█"];

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
                    if let Some(num_str) = remaining.get(2..end - 1)
                        && let Ok(count) = num_str.parse::<usize>()
                    {
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
                elements.push(&remaining[..end]);
                remaining = &remaining[end..];
            } else {
                elements.push(remaining);
                break;
            }
        } else {
            // Single Unicode character — remaining is non-empty (loop guard)
            let ch_len = remaining.chars().next().map_or(1, char::len_utf8);
            elements.push(&remaining[..ch_len]);
            remaining = &remaining[ch_len..];
        }
    }
    elements
}

/// A multi-row terminal graph that renders data as braille/block characters.
///
/// Multi-row graph renderer with vertical slicing so each row covers a
/// portion of the 0-100% range.
#[derive(Debug, Clone)]
pub struct Graph {
    pub width: usize,
    pub height: usize,
    symbol: GraphSymbol,
    invert: bool,
    pub max_value: i64,
    offset: i64,
    last: i64,
    /// Graph content: Vec<String> of `height` rows.
    buf: Vec<String>,
}

impl Graph {
    /// Create a new graph with the given dimensions, symbol mode, and value range.
    pub fn new(
        width: usize,
        height: usize,
        symbol: GraphSymbol,
        invert: bool,
        max_value: i64,
        offset: i64,
    ) -> Self {
        let h = height.max(1);
        Self {
            width,
            height: h,
            symbol,
            invert,
            max_value: if max_value == 0 { 100 } else { max_value },
            offset,
            last: 0,
            buf: vec![String::new(); h],
        }
    }

    /// Look up the braille symbol table (25 entries for prev×curr pairs).
    /// For block, returns None — it uses single-level lookup instead.
    fn braille_table(&self) -> Option<&'static [&'static str; 25]> {
        match (self.symbol, self.invert) {
            (GraphSymbol::Braille, false) => Some(&BRAILLE_UP),
            (GraphSymbol::Braille, true) => Some(&BRAILLE_DOWN),
            _ => None,
        }
    }

    /// Look up a single-character symbol for block mode.
    fn simple_char(&self, level: usize) -> &'static str {
        let lvl = level.min(4);
        match (self.symbol, self.invert) {
            (GraphSymbol::Block, false) => BLOCK_UP[lvl],
            (GraphSymbol::Block, true) => BLOCK_DOWN[lvl],
            (GraphSymbol::Braille, _) => unreachable!(),
        }
    }

    /// Map a value to a 0–4 braille/block level within a specific row.
    ///
    /// For single-height graphs, the full 0–100% range maps directly to 5
    /// levels (0 = empty, 4 = full).
    ///
    /// For multi-height graphs, the 0–100% range is divided vertically
    /// across rows. Each row covers a band:
    ///
    /// ```text
    /// ┌──────────────────┐
    /// │ Row 0: 75–100%   │ ← top (high values)
    /// │ Row 1: 50–75%    │
    /// │ Row 2: 25–50%    │
    /// │ Row 3: 0–25%     │ ← bottom (low values)
    /// └──────────────────┘
    /// ```
    ///
    /// A value falling below a row's band yields level 0 (empty).
    /// A value at or above the band yields level 4 (full).
    /// Values within the band interpolate to levels 1–3.
    ///
    /// The `invert` flag flips the row assignment so row 0 is the bottom.
    fn value_to_level(&self, value: i64, row: usize) -> usize {
        let max = self.max_value.max(1);
        let pct = ((value - self.offset).clamp(0, max) as f64 * 100.0 / max as f64).round() as i64;

        if self.height == 1 {
            return (pct * 4 / 100).clamp(0, 4) as usize;
        }

        // horizon: row index from the bottom (0 = lowest row).
        // Inverted graphs flip the mapping so row 0 draws low values.
        let horizon = if self.invert {
            self.height - 1 - row
        } else {
            row
        };
        // Percentage band covered by this row
        let cur_high = (100.0 * (self.height - horizon) as f64 / self.height as f64).round() as i64;
        let cur_low =
            (100.0 * (self.height - (horizon + 1)) as f64 / self.height as f64).round() as i64;

        if pct < cur_low {
            0
        } else if pct >= cur_high {
            4
        } else {
            let range = (cur_high - cur_low).max(1);
            ((pct - cur_low) * 4 / range).clamp(0, 4) as usize
        }
    }

    /// Create the full graph content from data.
    pub fn create(&mut self, data: &VecDeque<i64>) {
        let len = data.len();
        let h = self.height;

        self.buf = vec![String::new(); h];

        if self.width == 0 {
            return;
        }

        // If less data than width, pad left with cursor-right moves
        let pad_cols = self.width.saturating_sub(len);
        if pad_cols > 0 {
            let pad = format!("\x1b[{}C", pad_cols);
            for row_str in self.buf.iter_mut() {
                row_str.push_str(&pad);
            }
        }

        if len == 0 {
            return;
        }

        // Render the actual data columns (last `width` values, or all if less)
        let data_start = len.saturating_sub(self.width);
        let data_cols = len.min(self.width);

        for di in 0..data_cols {
            let data_idx = data_start + di;
            let curr = data[data_idx];
            let prev = if data_idx > 0 { data[data_idx - 1] } else { 0 };

            for row in 0..h {
                let mut prev_level = self.value_to_level(prev, row);
                let mut curr_level = self.value_to_level(curr, row);

                // Baseline row always shows at least level 1 so idle
                // graphs display a visible reference line.
                let is_baseline = if self.invert { row == 0 } else { row == h - 1 };
                if is_baseline {
                    if prev_level == 0 {
                        prev_level = 1;
                    }
                    if curr_level == 0 {
                        curr_level = 1;
                    }
                }

                if let Some(table) = self.braille_table() {
                    let idx = prev_level * 5 + curr_level;
                    let sym = table[idx.min(24)];
                    self.buf[row].push_str(sym);
                } else {
                    // Block: use current level only (no prev/curr pairing)
                    let sym = self.simple_char(curr_level);
                    self.buf[row].push_str(sym);
                }
            }

            self.last = curr;
        }
    }

    /// Render the multi-row graph with per-row gradient colors for multi-height,
    /// or per-column gradient colors for single-height.
    /// Returns Vec<String> where each element is one row with ANSI color codes.
    pub fn render_rows_colored(&self, data: &VecDeque<i64>, gradient: &[String]) -> Vec<String> {
        let h = self.height;
        let buf = &self.buf;
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

    /// Render a row with gradient colors applied per column.
    /// For backward compatibility with single-height graphs.
    pub fn render_row_colored(&mut self, data: &VecDeque<i64>, gradient: &[String]) -> String {
        self.create(data);
        let rows = self.render_rows_colored(data, gradient);
        rows.into_iter().next().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_rows_colored_returns_correct_count() {
        let mut graph = Graph::new(10, 4, GraphSymbol::Braille, false, 100, 0);
        let data: VecDeque<i64> = (0..10).map(|i| i * 10).collect();
        graph.create(&data);
        let gradient: Vec<String> = (0..=100)
            .map(|_| "\x1b[38;2;128;128;128m".to_string())
            .collect();
        let rows = graph.render_rows_colored(&data, &gradient);
        assert_eq!(rows.len(), 4);
    }
}
