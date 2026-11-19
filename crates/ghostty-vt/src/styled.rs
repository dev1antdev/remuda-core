//! remuda's own addition (REMUDA.md): the screen's rows with their colors, for a
//! client that draws them itself. Encoded cell by cell, since the formatter drops a
//! blank cell's background.

use crate::{Terminal, ffi};
use std::ffi::c_void;

const RESET: &str = "\x1b[0m";
const WIDE: i32 = 3;
const SPACER_TAIL: i32 = 2;
const CONTENT_TAG: i32 = 2;
const BG_PALETTE: i32 = 2;
const BG_RGB: i32 = 3;
const CELL_PALETTE: i32 = 10;
const CELL_RGB: i32 = 11;
const COLOR_PALETTE: i32 = 1;
const COLOR_RGB: i32 = 2;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Rgb {
    r: u8,
    g: u8,
    b: u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
union ColorValue {
    palette: u8,
    rgb: Rgb,
    _pad: u64,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Color {
    tag: i32,
    value: ColorValue,
}

#[repr(C)]
struct Style {
    size: usize,
    fg: Color,
    bg: Color,
    underline_color: Color,
    bold: bool,
    italic: bool,
    faint: bool,
    blink: bool,
    inverse: bool,
    invisible: bool,
    strikethrough: bool,
    overline: bool,
    underline: i32,
}

unsafe extern "C" {
    fn ghostty_grid_ref_cell(r: *const ffi::GridRef, out: *mut u64) -> i32;
    fn ghostty_grid_ref_graphemes(r: *const ffi::GridRef, buf: *mut u32, len: usize, out_len: *mut usize) -> i32;
    fn ghostty_grid_ref_style(r: *const ffi::GridRef, out: *mut Style) -> i32;
    fn ghostty_cell_get(cell: u64, data: i32, out: *mut c_void) -> i32;
}

impl Terminal {
    /// The screen's rows. Each starts with a style reset and holds its text and
    /// SGR, nothing else; trailing blank cells without a style are left out.
    pub fn styled_screen(&self) -> Vec<String> {
        let (cols, rows) = self.size();
        (0..rows as u32).map(|y| self.styled_row(ffi::POINT_ACTIVE, y, cols)).collect()
    }

    /// Scrollback rows like `styled_screen`'s, the newest first, read as taken
    /// (main screen only: none on the alternate screen).
    pub fn styled_history(&self) -> impl Iterator<Item = String> + '_ {
        let cols = self.size().0;
        let available = if self.alt_screen() { 0 } else { self.history_rows() };
        (0..available).rev().map(move |y| self.styled_row(ffi::POINT_HISTORY, y, cols))
    }

    fn styled_row(&self, tag: i32, y: u32, cols: u16) -> String {
        let mut row = String::from(RESET);
        let Some(first) = self.grid_ref(tag, 0, y) else { return row };
        // Finding a cell walks the page list; when the whole row sits in one page,
        // move along it from its first cell instead.
        let last = self.grid_ref(tag, cols.saturating_sub(1), y);
        let one_page = last.is_some_and(|last| last.node == first.node && last.y == first.y);
        let (mut pen, mut blanks) = (String::new(), 0);
        for x in 0..cols {
            let found = if one_page { Some(ffi::GridRef { x, ..first }) } else { self.grid_ref(tag, x, y) };
            let Some(at) = found else { break };
            let mut cell = 0u64;
            if unsafe { ghostty_grid_ref_cell(&at, &mut cell) } != ffi::SUCCESS {
                break;
            }
            if cell_value::<i32>(cell, WIDE) == SPACER_TAIL {
                continue;
            }
            let (text, sgr) = (graphemes(&at), sgr(&at, cell));
            if text.is_empty() && sgr.is_empty() {
                blanks += 1;
                continue;
            }
            if blanks > 0 && !pen.is_empty() {
                row.push_str(RESET);
                pen.clear();
            }
            row.extend(std::iter::repeat_n(' ', std::mem::take(&mut blanks)));
            if sgr != pen {
                row.push_str(if sgr.is_empty() { RESET.into() } else { format!("\x1b[0;{sgr}m") }.as_str());
                pen = sgr;
            }
            row.push_str(if text.is_empty() { " " } else { &text });
        }
        if !pen.is_empty() {
            row.push_str(RESET);
        }
        row
    }
}

fn cell_value<T: Default>(cell: u64, data: i32) -> T {
    let mut out = T::default();
    unsafe { ghostty_cell_get(cell, data, (&raw mut out).cast()) };
    out
}

fn graphemes(at: &ffi::GridRef) -> String {
    let mut buf = vec![0u32; 8];
    let mut len = 0usize;
    if unsafe { ghostty_grid_ref_graphemes(at, buf.as_mut_ptr(), buf.len(), &mut len) } != ffi::SUCCESS {
        buf.resize(len, 0);
        if unsafe { ghostty_grid_ref_graphemes(at, buf.as_mut_ptr(), buf.len(), &mut len) } != ffi::SUCCESS {
            return String::new();
        }
    }
    buf[..len.min(buf.len())].iter().filter_map(|&c| char::from_u32(c)).collect()
}

/// The cell's SGR parameters, "" for the default style. The underline color is
/// left out: a reader that knows only 38 and 48 would take its numbers for others.
fn sgr(at: &ffi::GridRef, cell: u64) -> String {
    let zero = Color { tag: 0, value: ColorValue { _pad: 0 } };
    let mut style = Style {
        size: size_of::<Style>(),
        fg: zero,
        bg: zero,
        underline_color: zero,
        bold: false,
        italic: false,
        faint: false,
        blink: false,
        inverse: false,
        invisible: false,
        strikethrough: false,
        overline: false,
        underline: 0,
    };
    unsafe { ghostty_grid_ref_style(at, &mut style) };
    let flags = [
        (style.bold, "1"),
        (style.faint, "2"),
        (style.italic, "3"),
        (style.underline == 1, "4"),
        (style.blink, "5"),
        (style.inverse, "7"),
        (style.invisible, "8"),
        (style.strikethrough, "9"),
        (style.overline, "53"),
    ];
    let mut params: Vec<String> = flags.iter().filter(|(on, _)| *on).map(|(_, p)| (*p).into()).collect();
    if style.underline > 1 {
        params.push(format!("4:{}", style.underline));
    }
    params.extend(color(38, style.fg));
    // A blank cell keeps its background in the cell itself.
    let bg = match cell_value::<i32>(cell, CONTENT_TAG) {
        BG_PALETTE => Color { tag: COLOR_PALETTE, value: ColorValue { palette: cell_value(cell, CELL_PALETTE) } },
        BG_RGB => Color { tag: COLOR_RGB, value: ColorValue { rgb: cell_value(cell, CELL_RGB) } },
        _ => style.bg,
    };
    params.extend(color(48, bg));
    params.join(";")
}

fn color(base: u8, c: Color) -> Option<String> {
    match c.tag {
        COLOR_PALETTE => Some(format!("{base};5;{}", unsafe { c.value.palette })),
        COLOR_RGB => {
            let Rgb { r, g, b } = unsafe { c.value.rgb };
            Some(format!("{base};2;{r};{g};{b}"))
        }
        _ => None,
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
        t.write(b"\x1b]2;title\x07\x1b[?2004h\x1b[1;31mred\x1b[0m plain\r\n\x1b[4;38;2;1;2;3munder\r\n");
        let rows = t.styled_screen();
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().all(|r| r.starts_with(RESET) && only_sgr(r)), "{rows:?}");
        assert_eq!(rows[0], "\x1b[0m\x1b[0;1;38;5;1mred\x1b[0m plain");
        assert_eq!(rows[1], "\x1b[0m\x1b[0;4;38;2;1;2;3munder\x1b[0m");
        assert_eq!(rows[2], RESET, "a blank row is a reset alone");
    }

    #[test]
    fn a_background_shows_on_blank_cells_and_ends_where_it_ends() {
        let mut t = Terminal::new(8, 3, 1 << 20);
        // A row erased in red, then a red A, a gap the cursor skipped, and B.
        t.write(b"\x1b[41m\x1b[2K\x1b[0m\r\n\x1b[41mA\x1b[0m\x1b[4CB\r\n\x1b[44m\x1b[2K");
        let rows = t.styled_screen();
        assert_eq!(rows[0], format!("\x1b[0m\x1b[0;48;5;1m{}\x1b[0m", " ".repeat(8)), "{rows:?}");
        assert_eq!(rows[1], "\x1b[0m\x1b[0;48;5;1mA\x1b[0m    B", "{rows:?}");
        assert_eq!(rows[2], format!("\x1b[0m\x1b[0;48;5;4m{}\x1b[0m", " ".repeat(8)), "blue is not red");
    }

    #[test]
    fn each_underline_shape_reads_differently() {
        let mut t = Terminal::new(8, 2, 1 << 20);
        t.write(b"\x1b[4mword\x1b[0m\r\n\x1b[4:3mword");
        let rows = t.styled_screen();
        assert_eq!(rows[0], "\x1b[0m\x1b[0;4mword\x1b[0m");
        assert_eq!(rows[1], "\x1b[0m\x1b[0;4:3mword\x1b[0m");
    }

    #[test]
    fn a_wide_character_takes_its_two_cells_once() {
        let mut t = Terminal::new(8, 1, 1 << 20);
        t.write("a界b".as_bytes());
        assert_eq!(plain(&t.styled_screen()[0]), "a界b");
    }

    #[test]
    fn history_comes_newest_first_on_the_main_screen_only() {
        let mut t = Terminal::new(20, 2, 1 << 20);
        t.write(b"\x1b[32mone\r\ntwo\r\nthree\r\nfour");
        let history: Vec<String> = t.styled_history().map(|r| plain(&r)).collect();
        assert_eq!(history, ["two", "one"]);
        let screen: Vec<String> = t.styled_screen().iter().map(|r| plain(r)).collect();
        assert_eq!(screen, ["three", "four"]);
        t.write(b"\x1b[?1049h\x1b[Hfull");
        assert_eq!(t.styled_history().count(), 0, "a full-screen program has no history here");
    }
}
