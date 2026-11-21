//! remuda's own addition (REMUDA.md): a first paint cut to what a stream's queue can carry.

use crate::wire::MAX_PENDING;

/// A paint with the newest `history` rows that fit a stream's queue, the oldest cut: `paint` renders it with
/// that many rows, and the screen and the state always come with it. A paint past the queue is dropped whole
/// (`wire::Outbox::data`), and a viewer would get nothing at all. A screen that alone is past the queue is
/// returned as it is: there is no history left to cut. The rows are not told apart by size, so the cut keeps
/// the most that fit: the average cost is tried first, then the boundary found between what fits and what does
/// not.
pub(crate) fn within(history: usize, paint: impl Fn(usize) -> Vec<u8>) -> Vec<u8> {
    let whole = paint(history);
    if whole.len() <= MAX_PENDING || history == 0 {
        return whole;
    }
    let screen = paint(0);
    if screen.len() >= MAX_PENDING || history <= 1 {
        return screen;
    }
    let room = MAX_PENDING - screen.len();
    let (mut fits, mut over) = (0, history);
    let mut best = screen;
    let mut rows = (room * history / (whole.len() - best.len())).clamp(1, history - 1);
    loop {
        let painted = paint(rows);
        if painted.len() <= MAX_PENDING {
            fits = rows;
            best = painted;
        } else {
            over = rows;
        }
        if over <= fits + 1 {
            return best;
        }
        rows = fits + (over - fits) / 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: usize = 1_000;
    const ROW: usize = 100;
    fn render(rows: usize) -> Vec<u8> {
        vec![b'a'; SCREEN + rows * ROW]
    }

    #[test]
    fn a_paint_that_fits_is_the_one_painted() {
        assert_eq!(within(10, render).len(), SCREEN + 10 * ROW);
    }

    #[test]
    fn a_paint_past_the_queue_keeps_the_most_rows_that_fit() {
        let out = within(100_000, render);
        assert!(out.len() <= MAX_PENDING, "{} is past the queue", out.len());
        assert!(out.len() > MAX_PENDING - ROW, "cut short by more than a row: {}", out.len());
    }

    #[test]
    fn denser_newest_rows_do_not_cost_rows_that_fit() {
        // the newest rows weigh twice the rest, so the average over the history understates them
        const WIDE: usize = 2_000;
        let render = |rows: usize| {
            let mut out = vec![b'a'; SCREEN];
            out.extend(std::iter::repeat_n(b'a', rows * WIDE));
            out.extend(std::iter::repeat_n(b'a', rows.saturating_sub(1000) * WIDE));
            out
        };
        let kept = std::cell::Cell::new(0);
        let out = within(3000, |rows| {
            let painted = render(rows);
            if painted.len() <= MAX_PENDING {
                kept.set(rows);
            }
            painted
        });
        assert!(out.len() <= MAX_PENDING, "{} is past the queue", out.len());
        assert!(kept.get() > 1000, "kept only {} rows", kept.get());
        assert!(render(kept.get() + 1).len() > MAX_PENDING, "kept {} rows, one more would fit", kept.get());
    }

    #[test]
    fn no_history_is_one_paint() {
        let calls = std::cell::Cell::new(0);
        let out = within(0, |rows| {
            calls.set(calls.get() + 1);
            render(rows)
        });
        assert_eq!((out.len(), calls.get()), (SCREEN, 1));
    }

    #[test]
    fn one_row_that_does_not_fit_leaves_the_screen() {
        let out = within(1, |rows| vec![b'a'; if rows == 1 { MAX_PENDING + 1 } else { SCREEN }]);
        assert_eq!(out.len(), SCREEN);
    }

    #[test]
    fn a_screen_alone_past_the_queue_is_painted_as_it_is() {
        let out = within(10, |rows| vec![b'a'; MAX_PENDING + rows * ROW]);
        assert_eq!(out.len(), MAX_PENDING);
    }
}
