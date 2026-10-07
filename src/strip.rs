//! Vertical layout of pages on one continuous strip of terminal rows.
//!
//! Scrolling happens in whole rows, so every page starts on a cell boundary and is only
//! ever clipped at row granularity.

/// Blank rows between consecutive pages.
pub const GAP: u32 = 1;

#[derive(Debug, Default)]
pub struct Strip {
    /// First row of each page, measured from the top of the first page.
    tops: Vec<u32>,
    heights: Vec<u32>,
}

/// A position inside a page that survives changes of page heights.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    page: usize,
    /// Offset from the top of the page in units of the page height.
    fraction: f64,
}

impl Strip {
    pub fn new(heights: impl IntoIterator<Item = u32>) -> Self {
        let heights: Vec<u32> = heights.into_iter().collect();
        let tops = heights
            .iter()
            .scan(0, |next, &height| {
                let top = *next;
                *next += height + GAP;
                Some(top)
            })
            .collect();
        Self { tops, heights }
    }

    pub fn len(&self) -> usize {
        self.tops.len()
    }

    pub fn height(&self) -> u32 {
        match (self.tops.last(), self.heights.last()) {
            (Some(top), Some(height)) => top + height,
            _ => 0,
        }
    }

    pub fn top(&self, page: usize) -> u32 {
        self.tops[page]
    }

    /// The page at `row`, or the page above when `row` lies in a gap.
    pub fn page_at(&self, row: u32) -> usize {
        self.tops
            .partition_point(|&top| top <= row)
            .saturating_sub(1)
    }

    /// The largest scroll offset that still fills a viewport of `rows` rows.
    pub fn max_scroll(&self, rows: u32) -> u32 {
        self.height().saturating_sub(rows)
    }

    pub fn anchor(&self, row: u32) -> Option<Anchor> {
        if self.tops.is_empty() {
            return None;
        }
        let page = self.page_at(row);
        let fraction = f64::from(row - self.tops[page]) / f64::from(self.heights[page].max(1));
        Some(Anchor { page, fraction })
    }

    /// The row of `anchor` in this strip, clamped to the last page.
    pub fn row(&self, anchor: Anchor) -> u32 {
        let Some(last) = self.len().checked_sub(1) else {
            return 0;
        };
        let page = anchor.page.min(last);
        let offset = (anchor.fraction * f64::from(self.heights[page])).round() as u32;
        self.tops[page] + offset
    }

    /// Pages overlapping the viewport `[scroll, scroll + rows)`, with the row offset of each
    /// page's top relative to the viewport's top.
    pub fn visible(&self, scroll: u32, rows: u32) -> impl Iterator<Item = (usize, i64)> + '_ {
        let bottom = scroll.saturating_add(rows);
        (self.page_at(scroll)..self.len())
            .take_while(move |&page| self.tops[page] < bottom)
            .filter(move |&page| self.tops[page] + self.heights[page] > scroll)
            .map(move |page| (page, i64::from(self.tops[page]) - i64::from(scroll)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip() -> Strip {
        Strip::new([10, 20, 5])
    }

    #[test]
    fn layout() {
        let strip = strip();
        assert_eq!(strip.tops, [0, 11, 32]);
        assert_eq!(strip.height(), 37);
        assert_eq!(strip.max_scroll(30), 7);
        assert_eq!(Strip::default().height(), 0);
    }

    #[test]
    fn page_at_assigns_gaps_to_the_page_above() {
        let strip = strip();
        assert_eq!(strip.page_at(0), 0);
        assert_eq!(strip.page_at(10), 0);
        assert_eq!(strip.page_at(11), 1);
        assert_eq!(strip.page_at(100), 2);
    }

    #[test]
    fn visible_pages_straddle_the_viewport() {
        let strip = strip();
        assert_eq!(strip.visible(5, 10).collect::<Vec<_>>(), [(0, -5), (1, 6)]);
        assert_eq!(strip.visible(10, 1).collect::<Vec<_>>(), []);
        assert_eq!(strip.visible(0, 100).count(), 3);
    }

    #[test]
    fn anchor_follows_page_height_changes() {
        let anchor = strip().anchor(21).unwrap();
        let taller = Strip::new([20, 40, 5]);
        assert_eq!(taller.row(anchor), 21 + 20);
        let fewer = Strip::new([10]);
        assert_eq!(fewer.row(anchor), 5);
    }
}
