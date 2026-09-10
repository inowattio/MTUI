use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Span;
use ratatui::widgets::{Block, Widget};

pub struct TableRow {
    pub spans: Vec<Span<'static>>,
    pub style: Style,
}

impl TableRow {
    pub fn plain(text: String, style: Style) -> Self {
        Self {
            spans: vec![Span::raw(text)],
            style,
        }
    }
}

pub struct RowsTable {
    block: Block<'static>,
    header: String,
    header_style: Style,
    rows: Vec<TableRow>,
    prefix: u16,
    h_off: u16,
}

impl RowsTable {
    pub fn new(
        block: Block<'static>,
        header: String,
        header_style: Style,
        rows: Vec<TableRow>,
    ) -> Self {
        Self {
            block,
            header,
            header_style,
            rows,
            prefix: 0,
            h_off: 0,
        }
    }

    pub fn hscroll(mut self, prefix: u16, offset: u16) -> Self {
        self.prefix = prefix;
        self.h_off = offset;
        self
    }
}

impl Widget for RowsTable {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let inner = self.block.inner(area);
        self.block.render(area, buf);
        if inner.is_empty() {
            return;
        }
        let header = [Span::raw(self.header)];
        write_row(
            buf,
            inner,
            inner.y,
            &header,
            self.header_style,
            self.prefix,
            self.h_off,
        );
        for (i, row) in self.rows.iter().enumerate() {
            let Some(y) = inner.y.checked_add(i as u16 + 1) else {
                break;
            };
            if y >= inner.bottom() {
                break;
            }
            write_row(
                buf,
                inner,
                y,
                &row.spans,
                row.style,
                self.prefix,
                self.h_off,
            );
        }
    }
}

fn is_plain(text: &str) -> bool {
    text.chars().all(|c| c == ' ' || c.is_ascii_graphic())
}

fn write_row(
    buf: &mut Buffer,
    inner: Rect,
    y: u16,
    spans: &[Span<'static>],
    style: Style,
    prefix: u16,
    h_off: u16,
) {
    buf.set_style(Rect::new(inner.x, y, inner.width, 1), style);
    let right = inner.right();
    let hidden = prefix as usize..(prefix as usize + h_off as usize);
    let mut x = inner.x;
    let mut index = 0usize;
    for span in spans {
        if x >= right {
            break;
        }
        if is_plain(&span.content) {
            let styled = span.style != Style::default();
            for ch in span.content.chars() {
                let skip = hidden.contains(&index);
                index += 1;
                if skip {
                    continue;
                }
                if x >= right {
                    break;
                }
                let cell = &mut buf[(x, y)];
                cell.set_char(ch);
                if styled {
                    cell.set_style(span.style);
                }
                x += 1;
            }
        } else {
            let visible: String = span
                .content
                .chars()
                .filter(|_| {
                    let skip = hidden.contains(&index);
                    index += 1;
                    !skip
                })
                .collect();
            x = buf
                .set_stringn(x, y, visible, (right - x) as usize, span.style)
                .0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    fn render(table: RowsTable, width: u16, height: u16) -> Buffer {
        let area = Rect::new(0, 0, width, height);
        let mut buf = Buffer::empty(area);
        table.render(area, &mut buf);
        buf
    }

    fn line(buf: &Buffer, y: u16) -> String {
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    #[test]
    fn header_then_rows_truncated_to_width() {
        let rows = vec![
            TableRow::plain("first row".into(), Style::default()),
            TableRow::plain("second row that is long".into(), Style::default()),
            TableRow::plain("never shown".into(), Style::default()),
        ];
        let buf = render(
            RowsTable::new(Block::default(), "head".into(), Style::default(), rows),
            10,
            3,
        );
        assert_eq!(line(&buf, 0), "head      ");
        assert_eq!(line(&buf, 1), "first row ");
        assert_eq!(line(&buf, 2), "second row");
    }

    #[test]
    fn hscroll_keeps_prefix_and_skips_offset() {
        let rows = vec![TableRow::plain("ab0123456789".into(), Style::default())];
        let buf = render(
            RowsTable::new(
                Block::default(),
                "hd0123456789".into(),
                Style::default(),
                rows,
            )
            .hscroll(2, 4),
            8,
            2,
        );
        assert_eq!(line(&buf, 0), "hd456789");
        assert_eq!(line(&buf, 1), "ab456789");
    }

    #[test]
    fn row_style_fills_row_and_span_style_patches() {
        let red = Style::default().fg(Color::Red);
        let zebra = Style::default().bg(Color::Blue);
        let rows = vec![TableRow {
            spans: vec![Span::raw("a"), Span::styled("b", red)],
            style: zebra,
        }];
        let buf = render(
            RowsTable::new(Block::default(), String::new(), Style::default(), rows),
            4,
            2,
        );
        assert_eq!(
            (buf[(0, 1)].fg, buf[(0, 1)].bg),
            (Color::Reset, Color::Blue)
        );
        assert_eq!((buf[(1, 1)].fg, buf[(1, 1)].bg), (Color::Red, Color::Blue));
        assert_eq!(
            (buf[(3, 1)].fg, buf[(3, 1)].bg),
            (Color::Reset, Color::Blue)
        );
    }

    #[test]
    fn printable_ascii_takes_the_fast_path() {
        assert!(is_plain("12:00 - ? | label"));
        assert!(!is_plain("a\tb"));
        assert!(!is_plain("caf\u{e9}"));
        assert!(!is_plain("\u{4e2d}"));
    }

    #[test]
    fn wide_glyphs_fall_back_to_unicode_aware_path() {
        let rows = vec![TableRow::plain("x\u{4e2d}y".into(), Style::default())];
        let buf = render(
            RowsTable::new(Block::default(), String::new(), Style::default(), rows),
            5,
            2,
        );
        let mut expected = Buffer::empty(Rect::new(0, 0, 5, 1));
        expected.set_stringn(0, 0, "x\u{4e2d}y", 5, Style::default());
        assert_eq!(line(&buf, 1), line(&expected, 0));
        assert_eq!(buf[(3, 1)].symbol(), "y");
    }

    #[test]
    fn hscroll_applies_to_fallback_rows() {
        let rows = vec![TableRow::plain("ab\u{4e2d}cdef".into(), Style::default())];
        let buf = render(
            RowsTable::new(Block::default(), String::new(), Style::default(), rows).hscroll(2, 1),
            6,
            2,
        );
        assert_eq!(line(&buf, 1), "abcdef");
    }
}
