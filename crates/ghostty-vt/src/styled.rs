//! remuda's own addition (REMUDA.md): the screen's rows with their colors, for a
//! client that draws them itself.

use crate::{Terminal, ffi};

const RESET: &str = "\x1b[0m";

impl Terminal {
    /// Up to `history` scrollback rows (main screen only), then the screen. Each
    /// row starts with a style reset and holds its text and SGR, nothing else.
    pub fn styled_rows(&self, history: usize) -> Vec<String> {
        let (cols, rows) = self.size();
        let available = if self.alt_screen() { 0 } else { self.history_rows() };
        let from = available.saturating_sub(u32::try_from(history).unwrap_or(u32::MAX));
        let history = (from..available).map(|y| self.styled_row(ffi::POINT_HISTORY, y, cols));
        history.chain((0..rows as u32).map(|y| self.styled_row(ffi::POINT_ACTIVE, y, cols))).collect()
    }

    fn styled_row(&self, tag: i32, y: u32, cols: u16) -> String {
        let row = self.select(tag, (0, y), (cols.saturating_sub(1), y));
        let bytes = row.map(|sel| self.format_with(ffi::FORMAT_VT, Some(&sel), false, false)).unwrap_or_default();
        let row = String::from_utf8_lossy(&bytes);
        if row.starts_with(RESET) { row.into_owned() } else { format!("{RESET}{row}") }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn only_sgr(row: &str) -> bool {
        row.split('\x1b').skip(1).all(|seq| {
            seq.strip_prefix('[')
                .and_then(|s| s.split_once('m'))
                .is_some_and(|(params, _)| params.bytes().all(|b| b.is_ascii_digit() || b == b';' || b == b':'))
        })
    }

    fn plain(row: &str) -> String {
        row.split('\x1b').map(|seq| seq.split_once('m').map_or(seq, |(_, text)| text)).collect()
    }

    #[test]
    fn rows_carry_their_colors_and_nothing_else() {
        let mut t = Terminal::new(20, 3, 1 << 20);
        t.write(b"\x1b]2;title\x07\x1b[?2004h\x1b[1;31mred\x1b[0m plain\r\n\x1b[4;38;5;33munder\r\n");
        let rows = t.styled_rows(0);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.starts_with(RESET) && only_sgr(r)), "{rows:?}");
        assert!(rows[0].contains("\x1b[1m") && rows[0].contains("38;5;1m") && rows[0].contains("red"), "{rows:?}");
        assert!(rows[1].contains("\x1b[4m") && rows[1].contains("38;5;33munder"), "{rows:?}");
        assert_eq!(rows[2], RESET, "a blank row is a reset alone");
    }

    #[test]
    fn history_comes_first_on_the_main_screen_only() {
        let mut t = Terminal::new(20, 2, 1 << 20);
        t.write(b"\x1b[32mone\r\ntwo\r\nthree\r\nfour");
        let rows = t.styled_rows(5);
        let text: Vec<String> = rows.iter().map(|r| plain(r)).collect();
        assert_eq!(text, ["one", "two", "three", "four"], "{rows:?}");
        assert_eq!(t.styled_rows(1).len(), 3, "as many history rows as asked");
        t.write(b"\x1b[?1049h\x1b[Hfull");
        assert_eq!(t.styled_rows(5).len(), 2, "a full-screen program shows its screen alone");
    }
}
