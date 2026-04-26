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

/// A terminal graph that renders data as braille/block/tty characters.
#[derive(Debug, Clone)]
pub struct Graph {
    pub width: usize,
    pub height: usize,
    symbol: GraphSymbol,
    invert: bool,
    no_zero: bool,
    max_value: i64,
    offset: i64,
    last: i64,
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
        Self {
            width,
            height,
            symbol,
            invert,
            no_zero,
            max_value: if max_value == 0 { 100 } else { max_value },
            offset,
            last: 0,
        }
    }

    /// Look up the graph symbol for a (previous, current) value pair.
    pub fn symbol_at(&self, prev: i64, curr: i64) -> &'static str {
        let max = self.max_value;
        let p = ((prev - self.offset).clamp(0, max) * 4 / max.max(1)) as usize;
        let c = ((curr - self.offset).clamp(0, max) * 4 / max.max(1)) as usize;
        let idx = (p.min(4)) * 5 + c.min(4);
        let table = match (self.symbol, self.invert) {
            (GraphSymbol::Braille, false) => &BRAILLE_UP,
            (GraphSymbol::Braille, true) => &BRAILLE_DOWN,
            (GraphSymbol::Block, false) => &BLOCK_UP,
            (GraphSymbol::Block, true) => &BLOCK_DOWN,
            (GraphSymbol::Tty, _) => &TTY_UP,
        };
        table[idx.min(24)]
    }

    /// Render a single row of graph data into a string of graph symbols.
    pub fn render_row(&mut self, data: &VecDeque<i64>) -> String {
        let mut result = String::with_capacity(self.width * 4);
        let len = data.len();

        for i in 0..self.width {
            let data_idx = if len > self.width {
                i + len - self.width
            } else {
                i
            };
            let curr = data.get(data_idx).copied().unwrap_or(0);
            let prev = if data_idx > 0 {
                data.get(data_idx - 1).copied().unwrap_or(0)
            } else {
                0
            };
            result.push_str(self.symbol_at(prev, curr));
            self.last = curr;
        }

        result
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
        assert_eq!(result, "⢸"); // prev=0 (4th level), curr=100 (4th level) → index 4
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
        // Same values should produce different symbols
        let s_up = graph_up.symbol_at(0, 50);
        let s_down = graph_down.symbol_at(0, 50);
        assert_ne!(s_up, s_down);
    }

    #[test]
    fn render_width_matches_data_length() {
        let mut graph = Graph::new(5, 1, GraphSymbol::Block, false, false, 100, 0);
        let data: VecDeque<i64> = vec![10, 20, 30, 40, 50].into();
        let result = graph.render_row(&data);
        // Each block char is a single Unicode char
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn render_max_value_clamping() {
        let graph = Graph::new(1, 1, GraphSymbol::Braille, false, false, 100, 0);
        // Values above max should clamp to max
        assert_eq!(graph.symbol_at(200, 200), "⣿");
    }

    #[test]
    fn render_offset_subtracted() {
        let graph = Graph::new(1, 1, GraphSymbol::Braille, false, false, 100, 50);
        // With offset=50, value 50 is effectively 0
        assert_eq!(graph.symbol_at(50, 50), " ");
        // Value 150 is effectively 100
        assert_eq!(graph.symbol_at(150, 150), "⣿");
    }
}
