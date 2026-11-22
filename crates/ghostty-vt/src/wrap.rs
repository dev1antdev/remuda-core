//! remuda's own addition (REMUDA.md): what a paint needs of the grid's rows beyond the formatter's own word —
//! whether a row continues the soft wrap above it, whether it holds text, and where a block's end falls.

use crate::{Terminal, ffi};
use std::ffi::c_void;

const ROW_WRAP: i32 = 1;
const ROW_WRAP_CONTINUATION: i32 = 2;
const CELL_HAS_TEXT: i32 = 4;
const CELL_WIDE: i32 = 3;
const WIDE_SPACER_HEAD: i32 = 3;

/// Grid rows counted from the oldest kept: the history, then the screen.
pub(crate) const ALL_ROWS: i32 = 2;

unsafe extern "C" {
    fn ghostty_grid_ref_cell(r: *const ffi::GridRef, out: *mut u64) -> i32;
    fn ghostty_grid_ref_row(r: *const ffi::GridRef, out: *mut u64) -> i32;
    fn ghostty_row_get(row: u64, data: i32, out: *mut c_void) -> i32;
    fn ghostty_cell_get(cell: u64, data: i32, out: *mut c_void) -> i32;
}

impl Terminal {
    /// Whether the row below row `y` continues it: the formatter leaves the break after such a row to the wrap.
    fn wraps(&self, tag: i32, y: u32) -> bool {
        let Some(at) = self.grid_ref(tag, 0, y) else { return false };
        let mut row = 0u64;
        if unsafe { ghostty_grid_ref_row(&at, &mut row) } != ffi::SUCCESS {
            return false;
        }
        row_data::<bool>(row, ROW_WRAP)
    }

    /// Whether row `y` continues the soft wrap of the row above it: the two flags admit a line only together.
    fn continues(&self, tag: i32, y: u32) -> bool {
        let Some(at) = self.grid_ref(tag, 0, y) else { return false };
        let mut row = 0u64;
        if unsafe { ghostty_grid_ref_row(&at, &mut row) } != ffi::SUCCESS {
            return false;
        }
        row_data::<bool>(row, ROW_WRAP_CONTINUATION)
    }

    /// Whether any cell of row `y` holds text.
    fn holds_text(&self, tag: i32, y: u32) -> bool {
        for x in 0..self.size().0 {
            let Some(at) = self.grid_ref(tag, x, y) else { break };
            let mut cell = 0u64;
            if unsafe { ghostty_grid_ref_cell(&at, &mut cell) } != ffi::SUCCESS {
                break;
            }
            if cell_data::<bool>(cell, CELL_HAS_TEXT) {
                return true;
            }
        }
        false
    }

    /// Whether the rows from..=to may be painted with their soft wraps joined. The break after a wrapped row is
    /// left to the wrap, so the row below has to carry it: its own continuation flag, its text — a row inserted
    /// under a wrap keeps the flag above it and loses its own, and a blank row writes no cells, so the one break
    /// the formatter writes there would stand for two rows — and no wide character forcing it, whose width a
    /// viewer may count for itself and count as one cell.
    pub(crate) fn joins_wraps(&self, tag: i32, from: u32, to: u32) -> bool {
        (from..to).all(|y| {
            !self.wraps(tag, y)
                || (self.continues(tag, y + 1) && self.holds_text(tag, y + 1) && !self.ends_on_a_spacer_head(tag, y))
        })
    }

    /// Whether the last cell of row `y` is the spacer of a wide character living on the row below: a paint ending
    /// there pulls that character in with the wrap (`PageListFormatter.format` moves such an end), and the row that
    /// owns it would then paint it a second time.
    pub(crate) fn ends_on_a_spacer_head(&self, tag: i32, y: u32) -> bool {
        let Some(at) = self.grid_ref(tag, self.size().0.saturating_sub(1), y) else { return false };
        let mut cell = 0u64;
        if unsafe { ghostty_grid_ref_cell(&at, &mut cell) } != ffi::SUCCESS {
            return false;
        }
        cell_data::<i32>(cell, CELL_WIDE) == WIDE_SPACER_HEAD
    }
}

fn row_data<T: Default>(row: u64, data: i32) -> T {
    let mut out = T::default();
    unsafe { ghostty_row_get(row, data, (&raw mut out).cast()) };
    out
}

fn cell_data<T: Default>(cell: u64, data: i32) -> T {
    let mut out = T::default();
    unsafe { ghostty_cell_get(cell, data, (&raw mut out).cast()) };
    out
}

#[cfg(test)]
mod tests {
    use crate::Terminal;

    #[test]
    fn a_painted_history_keeps_its_soft_wraps() {
        let mut t = Terminal::new(20, 3, 1 << 20);
        // The word straddles the wrap; it is history by the time the paint is asked for.
        t.write(b"0123456789abcdefghijKLMNOP\r\none\r\ntwo\r\nthree\r\n");
        let mut fresh = Terminal::new(20, 3, 1 << 20);
        fresh.write(&t.paint(3));
        assert!(fresh.plain_text().contains("0123456789abcdefghijKLMNOP"), "{:?}", fresh.plain_text());
    }

    #[test]
    fn a_painted_screen_keeps_its_soft_wraps() {
        let mut t = Terminal::new(20, 3, 1 << 20);
        t.write(b"0123456789abcdefghijKLMNOP");
        let mut fresh = Terminal::new(20, 3, 1 << 20);
        fresh.write(&t.paint(0));
        assert!(fresh.plain_text().contains("0123456789abcdefghijKLMNOP"), "{:?}", fresh.plain_text());
    }

    #[test]
    fn a_painted_history_whose_wrap_falls_on_a_wide_character_keeps_its_rows() {
        let head = "a".repeat(19);
        let mut t = Terminal::new(20, 3, 1 << 20);
        t.write(format!("{head}\u{4e16}\u{754c}\r\none\r\ntwo\r\n").as_bytes());
        let mut fresh = Terminal::new(20, 3, 1 << 20);
        fresh.write(&t.paint(2));
        let text = fresh.plain_text();
        // The viewer counts the wide character's cells for itself, so the row it forces keeps its own break.
        assert!(text.contains(&format!("{head}\n\u{4e16}\u{754c}")), "{text:?}");
    }

    #[test]
    fn a_blank_row_that_continues_a_wrap_keeps_its_row() {
        let mut t = Terminal::new(10, 3, 1 << 20);
        // The wrap's continuation was erased: the row takes no text, but it still takes a row.
        t.write(b"0123456789abc\x1b[2K\r\nnext");
        let mut fresh = Terminal::new(10, 3, 1 << 20);
        fresh.write(&t.paint(0));
        assert_eq!(fresh.screen_rows(), t.screen_rows());
    }

    #[test]
    fn a_blank_row_inserted_after_a_wrap_keeps_its_row() {
        let mut t = Terminal::new(10, 4, 1 << 20);
        // The row the wrap ran into was inserted away: the row above keeps its wrap, the new one takes no text.
        t.write(b"0123456789abc\x1b[2;1H\x1b[L");
        let mut fresh = Terminal::new(10, 4, 1 << 20);
        fresh.write(&t.paint(0));
        assert_eq!(fresh.screen_rows(), t.screen_rows());
    }

    #[test]
    fn a_row_inserted_under_a_wrap_takes_its_own_row() {
        let mut t = Terminal::new(10, 4, 1 << 20);
        // A row inserted under a wide character's wrap keeps the wrap flag above it and loses its own.
        t.write("123456789\u{4e16}X\x1b[2;1H\x1b[Lnext".as_bytes());
        let mut fresh = Terminal::new(10, 4, 1 << 20);
        fresh.write(&t.paint(0));
        assert_eq!(fresh.screen_rows(), t.screen_rows());
    }

    #[test]
    fn erasing_the_wide_character_that_wrapped_leaves_the_rows_apart() {
        let mut t = Terminal::new(10, 3, 1 << 20);
        // Both wrap flags survive the erase, and nothing is left to force the wrap the row above leaves to it.
        t.write("123456789\u{4e16}X\x1b[2;2H\x1b[1K".as_bytes());
        let mut fresh = Terminal::new(10, 3, 1 << 20);
        fresh.write(&t.paint(0));
        assert_eq!(fresh.screen_rows(), t.screen_rows());
    }

    #[test]
    fn a_screen_whose_program_turned_autowrap_off_keeps_its_rows() {
        let mut t = Terminal::new(10, 3, 1 << 20);
        t.write(b"0123456789abc\x1b[?7l");
        let mut fresh = Terminal::new(10, 3, 1 << 20);
        fresh.write(&t.paint(0));
        assert_eq!(fresh.screen_rows(), t.screen_rows(), "a joined row is drawn with autowrap on");
        assert!(!fresh.dec_mode(7), "the program's autowrap off goes back with the paint");
    }

    #[test]
    fn a_paint_that_may_not_join_keeps_the_rows_apart() {
        let mut t = Terminal::new(10, 3, 1 << 20);
        t.write(b"0123456789abc");
        // A viewer that did not take this grid: its own width, and the rows it is given stay rows.
        let mut fresh = Terminal::new(14, 3, 1 << 20);
        fresh.write(&t.paint_with(0, None, false));
        let rows = fresh.screen_rows();
        assert_eq!((rows[0].trim_end(), rows[1].trim_end()), ("0123456789", "abc"));
    }

    #[test]
    fn a_wide_character_at_the_history_cut_is_painted_once() {
        let mut t = Terminal::new(10, 3, 1 << 20);
        // The last history row ends on the spacer of the wide character the screen's first row owns.
        t.write("123456789\u{4e16}X\r\none\r\ntwo".as_bytes());
        let mut fresh = Terminal::new(10, 3, 1 << 20);
        fresh.write(&t.paint(1));
        assert_eq!(fresh.plain_text().matches('\u{4e16}').count(), 1, "{:?}", fresh.plain_text());
    }
}
