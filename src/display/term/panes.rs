// Oprs -- process monitor for Linux
// Copyright (C) 2024  Laurent Pelecq
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use num_traits::Zero;
use ratatui::{
    layout::{Alignment, Constraint, Layout},
    prelude::*,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Widget, Wrap},
    Frame,
};
use std::{cmp, fmt};

use super::{
    types::{Area, MaxLength, UnboundedArea},
    KeyMap, MenuEntry,
};

const BORDER_SIZE: u16 = 1;

/// Format a text by applying header style.
///
/// A header of level 1 or level 2 are followed by lines starting
/// respectively with ==== and ----.
fn format_text<'l>(help: &'static str) -> Vec<Line<'l>> {
    help.lines()
        .map(|s| {
            if s.starts_with("## ") {
                let (_, s) = s.split_at(3);
                Line::from(s).style(Style::default().add_modifier(Modifier::UNDERLINED))
            } else if s.starts_with("# ") {
                let (_, s) = s.split_at(2);
                Line::from(s).style(
                    Style::default()
                        .add_modifier(Modifier::BOLD)
                        .add_modifier(Modifier::UNDERLINED),
                )
            } else {
                Line::from(s)
            }
        })
        .collect()
}

/// Calculate widths constraints to avoid an overflow
///
/// # Arguments
///
/// * `inner_width` - The usable width to display the table.
fn width_constraints(
    inner_width: u16,
    column_widths: &[u16],
    column_spacing: u16,
) -> (u16, Vec<Constraint>, bool) {
    let mut total_width = 0;
    let mut constraints = Vec::with_capacity(column_widths.len());
    let mut current_column_spacing = 0;
    let mut truncated = false;
    while constraints.len() < column_widths.len() {
        let index = constraints.len();
        let col_width = column_widths[index];
        let new_total_width = total_width + current_column_spacing + col_width;
        if new_total_width <= inner_width {
            constraints.push(Constraint::Length(col_width));
        } else {
            let remaining = inner_width - total_width;
            if remaining > column_spacing {
                // Partial last column
                constraints.push(Constraint::Length(remaining - column_spacing));
                total_width = inner_width;
                truncated = true;
            }
            break;
        }
        total_width = new_total_width;
        current_column_spacing = column_spacing;
    }
    let hoverflow = constraints.len() < column_widths.len() || truncated;
    (total_width, constraints, hoverflow)
}

/// Navigation arrows dependending on table overflows
///
/// # Arguments
///
/// * `shifted` - Area boolean saying if the first line or first column are hidden.
/// * `overflow` - Area boolean saying if the end is visible.
fn navigation_arrows(shifted: Area<bool>, overflows: Area<bool>) -> Text<'static> {
    let up_arrow = if shifted.vertical { " " } else { "⬆" };
    let down_arrow = if overflows.vertical { "⬇" } else { " " };
    let left_arrow = if shifted.horizontal { " " } else { "⬅" };
    let right_arrow = if overflows.horizontal { "➡" } else { " " };
    Text::from(vec![
        Line::from(up_arrow),
        Line::from(format!("{left_arrow} {down_arrow} {right_arrow}")),
    ])
    .alignment(Alignment::Center)
}

/// Table style
#[derive(Debug)]
pub(crate) struct TableStyle {
    column_spacing: u16,
    even_row: Style,
    odd_row: Style,
}

impl TableStyle {
    pub(crate) fn new(column_spacing: u16, even_row: Style, odd_row: Style) -> Self {
        Self {
            column_spacing,
            even_row,
            odd_row,
        }
    }

    /// Apply style to rows
    ///
    /// The table is truncated to keep only `ncols` column.
    fn apply<'a>(&self, mut rows: Vec<Vec<Cell<'a>>>, ncols: usize) -> Vec<Row<'a>> {
        rows.drain(..)
            .enumerate()
            .map(|(i, mut r)| {
                let style = if i % 2 != 0 {
                    self.even_row
                } else {
                    self.odd_row
                };
                if r.len() < ncols {
                    panic!("rows must have {} columns instead of {}", ncols, r.len());
                }
                Row::new(r.drain(0..ncols)).style(style)
            })
            .collect::<Vec<Row>>()
    }
}

/// Widget that adapt it's layout to the available space.
pub trait ReactiveWidget: fmt::Debug + Widget {
    /// Cursor position relative to widget origin
    fn cursor(&self) -> Option<Position> {
        None
    }

    /// Minimum height in area.
    fn min_height(&self, area: Rect) -> u16;
}

/// One line only.
#[derive(Debug)]
pub(crate) struct OneLineWidget<'t> {
    text: Paragraph<'t>,
    text_length: u16,
    title: Option<&'static str>,
}

impl<'t> OneLineWidget<'t> {
    pub(crate) fn new(text: Text<'t>, style: Style, title: Option<&'static str>) -> Self {
        let text_length = text.width() as u16;
        Self {
            text: Paragraph::new(text).style(style),
            text_length,
            title,
        }
    }

    pub(crate) fn with_menu<'a, I>(entries: I, keymap: KeyMap) -> Self
    where
        I: Iterator<Item = &'a MenuEntry>,
    {
        let mut spans = Vec::new();
        let mut sep = "";
        entries
            .into_iter()
            .filter(|e| e.keymaps().contains(keymap))
            .for_each(|entry| {
                spans.push(Span::raw(sep));
                spans.push(Span::styled(
                    entry.key().to_string(),
                    Style::default().add_modifier(Modifier::REVERSED),
                ));
                spans.push(Span::raw(format!(" {}", entry.label())));
                sep = "  ";
            });
        Self::new(Text::from(Line::from(spans)), Style::default(), None)
    }
}

impl ReactiveWidget for OneLineWidget<'_> {
    fn cursor(&self) -> Option<Position> {
        Some(Position::new(self.text_length, 0))
    }

    fn min_height(&self, area: Rect) -> u16 {
        let borders = if self.title.is_some() {
            2 * BORDER_SIZE
        } else {
            0
        };
        let inner_width = area.width.saturating_sub(borders);
        self.text_length.div_ceil(inner_width) + borders
    }
}

impl Widget for OneLineWidget<'_> {
    // Required method
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        match self.title {
            Some(title) => self
                .text
                .block(
                    Block::new()
                        .title(format!(" {} ", title))
                        .title_alignment(Alignment::Left)
                        .borders(Borders::ALL),
                )
                .wrap(Wrap { trim: false })
                .render(area, buf),
            None => self.text.render(area, buf),
        }
    }
}

/// Scrollable long text that can exceed the screen height.
#[derive(Debug)]
pub(crate) struct MarkdownWidget<'l> {
    title: &'static str,
    text: Vec<Line<'l>>,
    text_height: u16,
    offset: u16,
}

impl MarkdownWidget<'_> {
    pub(crate) fn new(title: &'static str, text: &'static str, offset: u16) -> Self {
        let text = format_text(text);
        let text_height = text.len() as u16;
        Self {
            title,
            text,
            text_height,
            offset,
        }
    }

    /// Prepare the widget to fit in the area.
    ///
    /// Returns the inner height and the offset.
    pub(crate) fn prepare(&mut self, area: &Rect) -> (u16, u16) {
        let borders = BORDER_SIZE * 2;
        let inner_height = area.height - borders;
        let max_offset = self.text_height.saturating_sub(inner_height / 2);
        self.offset = cmp::min(self.offset, max_offset);
        (inner_height, self.offset)
    }
}

impl ReactiveWidget for MarkdownWidget<'_> {
    fn min_height(&self, area: Rect) -> u16 {
        cmp::min(self.text_height + BORDER_SIZE * 2, area.height)
    }
}

impl Widget for MarkdownWidget<'_> {
    // Required method
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        Paragraph::new(Text::from(self.text))
            .block(
                Block::new()
                    .title(format!(" {} ", self.title))
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false })
            .scroll((self.offset, 0))
            .render(area, buf);
    }
}

/// Table that can overflow horizontally and vertically.
#[derive(Debug)]
pub(crate) struct BigTableWidget<'a, 'b, 'c> {
    headers: Vec<Cell<'a>>,
    headers_height: u16,
    rows: Vec<Vec<Cell<'b>>>,
    widths: &'c [u16],
    offset: UnboundedArea,
    constraints: Vec<Constraint>,
    style: TableStyle,
}

impl<'a, 'b, 'c> BigTableWidget<'a, 'b, 'c> {
    pub(crate) fn new(
        headers: Vec<Cell<'a>>,
        headers_height: u16,
        rows: Vec<Vec<Cell<'b>>>,
        widths: &'c [u16],
        offset: UnboundedArea,
        style: TableStyle,
    ) -> Self {
        Self {
            headers,
            headers_height,
            rows,
            widths,
            offset,
            constraints: Vec::new(),
            style,
        }
    }

    /// Prepare the widget to fit in the area.
    ///
    /// Returns the inner height and the overflow.
    pub(crate) fn prepare(&mut self, area: &Rect) -> (u16, Area<bool>) {
        let borders = BORDER_SIZE * 2;
        let outter_area = Size::new(area.width, area.height);
        let inner_area = Size::new(outter_area.width - borders, outter_area.height - borders);
        let (_table_width, constraints, hoverflow) =
            width_constraints(inner_area.width, self.widths, self.style.column_spacing);
        self.constraints = constraints;
        let table_height = self.headers_height + self.rows.len() as u16;
        let overflow = Area::new(hoverflow, table_height > inner_area.height);
        let shifted = Area::new(
            self.offset.horizontal.is_zero(),
            self.offset.vertical.is_zero(),
        );
        let nav = navigation_arrows(shifted, overflow);
        self.headers[0] = Cell::from(nav);
        (inner_area.height, overflow)
    }
}

impl ReactiveWidget for BigTableWidget<'_, '_, '_> {
    fn min_height(&self, area: Rect) -> u16 {
        cmp::min(self.headers_height + BORDER_SIZE * 2, area.height)
    }
}

impl Widget for BigTableWidget<'_, '_, '_> {
    // Required method
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let rows = self.style.apply(self.rows, self.widths.len());
        let table = Table::new(rows, self.constraints)
            .block(Block::default().borders(Borders::ALL))
            .header(Row::new(self.headers).height(self.headers_height))
            .column_spacing(self.style.column_spacing);
        Widget::render(table, area, buf);
    }
}

/// Sequence of name and value fields.
#[derive(Debug)]
pub(crate) struct FieldsWidget<'l> {
    title: &'static str,
    lines: &'l [(&'static str, String)],
}

impl<'l> FieldsWidget<'l> {
    pub fn new(title: &'static str, lines: &'l [(&'static str, String)]) -> Self {
        Self { title, lines }
    }
}

impl ReactiveWidget for FieldsWidget<'_> {
    fn min_height(&self, area: Rect) -> u16 {
        cmp::min(self.lines.len() as u16 + BORDER_SIZE * 2, area.height)
    }
}

impl Widget for FieldsWidget<'_> {
    // Required method
    fn render(self, area: Rect, buf: &mut Buffer)
    where
        Self: Sized,
    {
        let rows = self.lines.iter().map(|(name, value)| {
            Row::new(vec![
                Text::from(name.to_string()),
                Text::from(value.to_string()).alignment(Alignment::Right),
            ])
        });
        let cw1 = MaxLength::with_lines(self.lines.iter().map(|(name, _)| *name));
        let constraints = [Constraint::Length(cw1.len()), Constraint::Min(0)];
        let table = Table::new(rows, constraints).block(
            Block::new()
                .title(self.title)
                .title_alignment(Alignment::Left)
                .borders(Borders::ALL),
        );
        Widget::render(table, area, buf);
    }
}

/// A builder for a list of rectangle to draw widgets on the screen.
pub(crate) trait Pane {
    fn build(self) -> Vec<Option<Rect>>;
}

/// Pane with a main scrollable area on top and fixed height widgets at the bottom.
#[derive(Debug, Default)]
pub(crate) struct SingleScrollablePane {
    area: Rect,
    rects: Vec<Rect>,
}

impl SingleScrollablePane {
    pub(crate) fn new(area: Rect, capacity: usize) -> Self {
        let mut rects = Vec::with_capacity(capacity);
        rects.push(area);
        Self { area, rects }
    }

    /// Push a fixed height widget at the bottom.
    pub(crate) fn with<W: ReactiveWidget>(mut self, widget: &W) -> Self {
        let height = widget.min_height(self.area);
        let main_rect = self.rects.first_mut().expect("must have a first rectangle");
        main_rect.height = main_rect.height.saturating_sub(height);
        self.rects.iter_mut().skip(1).for_each(|r| {
            if r.y < height {
                r.height -= height.saturating_sub(r.y);
                r.y = 0;
            } else {
                r.y -= height
            }
        });
        self.rects.push(Rect::new(
            self.area.x,
            self.area.height.saturating_sub(height),
            self.area.width,
            height,
        ));
        self
    }
}

impl Pane for SingleScrollablePane {
    fn build(mut self) -> Vec<Option<Rect>> {
        self.rects
            .drain(..)
            .map(|r| if r.height == 0 { None } else { Some(r) })
            .collect()
    }
}

#[derive(Debug, Clone, Copy)]
enum GridLine {
    Row(usize, u16),
    Fill,
    Line(u16),
}

/// Pane where widgets are on the same row are evenly distributed.
///
/// Widgets on top are organized in rows. Widgets are evenly distributed in the row.
/// Rows are truncated if the screen size is too small.
///
/// Bottom widgets are one per line and always displayed.
#[derive(Debug)]
pub(crate) struct GridPane {
    area: Rect,
    lines: Vec<GridLine>,
}

impl GridPane {
    pub(crate) fn new(area: Rect) -> Self {
        let lines = Vec::new();
        Self { area, lines }
    }

    pub(crate) fn with_row<W: ReactiveWidget>(mut self, row: &[&W]) -> Self {
        let area = self.area;
        let height = row.iter().map(|w| w.min_height(area)).max().unwrap_or(0);
        if matches!(
            self.lines.last(),
            Some(GridLine::Fill) | Some(GridLine::Line(_))
        ) {
            panic!("rows cannot follow a non-row");
        }
        self.lines.push(GridLine::Row(row.len(), height));
        self
    }

    pub(crate) fn with_line<W: ReactiveWidget>(mut self, widget: &W) -> Self {
        if matches!(self.lines.last(), Some(GridLine::Row(_, _))) {
            self.lines.push(GridLine::Fill);
        }
        let height = widget.min_height(self.area);
        self.lines.push(GridLine::Line(height));
        self
    }
}

impl Pane for GridPane {
    fn build(self) -> Vec<Option<Rect>> {
        let ncells = self
            .lines
            .iter()
            .map(|l| match l {
                GridLine::Row(ncols, _) => *ncols,
                _ => 1,
            })
            .sum();
        let mut bottom_height = self
            .lines
            .iter()
            .map(|l| match l {
                GridLine::Line(height) => *height,
                _ => 0,
            })
            .sum();
        let x = self.area.x;
        let width = self.area.width;
        let mut y = self.area.y;
        let mut body_height = self.area.height.saturating_sub(bottom_height);
        let mut rects = Vec::with_capacity(ncells);
        self.lines.iter().for_each(|l| match l {
            GridLine::Row(ncols, height) if body_height > 0 => {
                let height = cmp::min(body_height, *height);
                let ratio = 100 / (*ncols as u16);
                if ratio == 0 {
                    (0..*ncols).for_each(|_| rects.push(None));
                } else {
                    let rect = Rect::new(x, y, width, height);
                    let mut constraints = Vec::with_capacity(*ncols);
                    constraints.extend((0..(ncols - 1)).map(|_| Constraint::Percentage(ratio)));
                    constraints.push(Constraint::Fill(1));
                    rects.extend(
                        Layout::horizontal(constraints)
                            .split(rect)
                            .iter()
                            .map(|r| Some(r.to_owned())),
                    );
                    body_height -= height;
                    y += height;
                }
            }
            GridLine::Row(ncols, _) => (0..*ncols).for_each(|_| rects.push(None)),
            GridLine::Fill if body_height > 0 => {
                rects.push(Some(Rect::new(x, y, width, body_height)));
                y += body_height;
                body_height = 0;
            }
            GridLine::Fill => rects.push(None),
            GridLine::Line(height) if *height <= bottom_height => {
                rects.push(Some(Rect::new(x, y, width, *height)));
                y += *height;
                bottom_height -= height;
            }
            GridLine::Line(_) => rects.push(None),
        });
        rects
    }
}

/// Render widgets that may be invisible.
pub(crate) struct OptionalRenderer<'a, 'f, 'v> {
    frame: &'f mut Frame<'a>,
    iter: std::vec::Drain<'v, Option<Rect>>,
}

impl<'a, 'f, 'v> OptionalRenderer<'a, 'f, 'v> {
    pub(crate) fn new(frame: &'f mut Frame<'a>, rects: &'v mut Vec<Option<Rect>>) -> Self {
        let iter = rects.drain(..);
        Self { frame, iter }
    }

    pub(crate) fn render_widget<W: Widget>(&mut self, widget: W) -> bool {
        match self.iter.next() {
            Some(Some(rect)) => {
                self.frame.render_widget(widget, rect);
                true
            }
            Some(None) => true,
            None => false,
        }
    }
}

#[cfg(test)]
mod test {

    use ratatui::{buffer::Buffer, layout::Constraint, layout::Rect, widgets::Widget};
    use rstest::*;
    use std::cmp;

    use super::{width_constraints, GridPane, Pane, ReactiveWidget, SingleScrollablePane};

    #[test]
    fn test_width_constraints_underflow() {
        // 0         1
        // 01234567890123456789
        // aaaaa bbbb bbbb
        const SCREEN_WIDTH: u16 = 20;
        const FIRST_COLUMN_WIDTH: u16 = 5;
        const COLUMN_WIDTH: u16 = 4;
        let column_widths = vec![FIRST_COLUMN_WIDTH, COLUMN_WIDTH, COLUMN_WIDTH];
        const COLUMN_SPACING: u16 = 1;
        const NCOLS: usize = 3;
        let (table_width, widths, hoverflow) =
            width_constraints(SCREEN_WIDTH, &column_widths, COLUMN_SPACING);
        const SPACED_COLUMN_WIDTH: u16 = COLUMN_WIDTH + COLUMN_SPACING;
        const EXPECTED_WIDTH: u16 = FIRST_COLUMN_WIDTH + (NCOLS as u16 - 1) * SPACED_COLUMN_WIDTH;
        assert_eq!(EXPECTED_WIDTH, table_width);
        assert_eq!(NCOLS, widths.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), widths[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), widths[1]);
        assert!(!hoverflow);
    }

    #[test]
    fn test_width_constraints_exact() {
        // 0         1
        // 01234567890123456789
        // aaaaa bbbb bbbb bbbb
        const SCREEN_WIDTH: u16 = 20;
        const FIRST_COLUMN_WIDTH: u16 = 5;
        const COLUMN_WIDTH: u16 = 4;
        let column_widths = vec![FIRST_COLUMN_WIDTH, COLUMN_WIDTH, COLUMN_WIDTH, COLUMN_WIDTH];
        const COLUMN_SPACING: u16 = 1;
        let (table_width, widths, hoverflow) =
            width_constraints(SCREEN_WIDTH, &column_widths, COLUMN_SPACING);
        const SPACED_COLUMN_WIDTH: u16 = COLUMN_WIDTH + COLUMN_SPACING;
        let expected_width: u16 =
            FIRST_COLUMN_WIDTH + (column_widths.len() as u16 - 1) * SPACED_COLUMN_WIDTH;
        const EXPECTED_NCOLS: usize = 4;
        assert_eq!(expected_width, table_width);
        assert_eq!(EXPECTED_NCOLS, widths.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), widths[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), widths[1]);
        assert!(!hoverflow);
    }

    #[test]
    fn test_width_constraints_overflow() {
        // 0         1
        // 01234567890123456789
        // aaaaa bbb bbb bbb bbb bbb
        const SCREEN_WIDTH: u16 = 20;
        const FIRST_COLUMN_WIDTH: u16 = 5;
        const COLUMN_WIDTH: u16 = 3;
        let column_widths = vec![
            FIRST_COLUMN_WIDTH,
            COLUMN_WIDTH,
            COLUMN_WIDTH,
            COLUMN_WIDTH,
            COLUMN_WIDTH,
            COLUMN_WIDTH,
        ];
        const COLUMN_SPACING: u16 = 1;
        let (table_width, widths, hoverflow) =
            width_constraints(SCREEN_WIDTH, &column_widths, COLUMN_SPACING);
        const EXPECTED_NCOLS: usize = 5;
        assert_eq!(SCREEN_WIDTH, table_width);
        assert_eq!(EXPECTED_NCOLS, widths.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), widths[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), widths[1]);
        assert_eq!(Constraint::Length(2), widths[4]);
        assert!(hoverflow);
    }

    #[derive(Debug)]
    struct MockWidget(u16);

    impl ReactiveWidget for MockWidget {
        fn min_height(&self, area: Rect) -> u16 {
            let Self(height) = self;
            cmp::min(*height, area.height)
        }
    }

    impl Widget for MockWidget {
        fn render(self, _area: Rect, _buf: &mut Buffer) {
            panic!("MockWidget::render is not implemented");
        }
    }

    /// SingleScrollablePane
    ///
    /// Case 1:
    /// 0 main
    /// ...
    /// 6 main
    /// 7 w1
    /// 8 w1
    /// 9 w2
    ///
    /// Case 2:
    /// - w1
    /// 0 w1
    /// 1 w2
    ///
    /// Case 3:
    /// - w1
    /// - w1
    /// 0 w2
    ///
    #[rstest]
    #[case(10, vec![ Some(Rect::new(0, 0, 15, 7)),
                     Some(Rect::new(0, 7, 15, 2)),
                     Some(Rect::new(0, 9, 15, 1)) ])]
    #[case(2, vec![None, Some(Rect::new(0, 0, 15, 1)), Some(Rect::new(0, 1, 15, 1))])]
    #[case(1, vec![None, None, Some(Rect::new(0, 0, 15, 1))])]
    fn test_single_scrollable_pane(#[case] height: u16, #[case] expected: Vec<Option<Rect>>) {
        let screen = Rect::new(0, 0, 15, height);
        let w1 = MockWidget(2);
        let w2 = MockWidget(1);
        let rects = SingleScrollablePane::new(screen, 3)
            .with(&w1)
            .with(&w2)
            .build();
        assert_eq!(3, rects.len());
        assert_eq!(expected, rects);
    }

    /// GridPane
    ///
    /// Case 1: large height with a gap between the last widget and the bottom line.
    /// Case 2: no gap between the last widget and the bottom line.
    /// Case 3: last row of widgets is truncated.
    /// Case 4: last row of widgets is invisible.
    #[rstest]
    #[case(10, vec![ Some(Rect::new(0, 0, 15, 2)),
                     Some(Rect::new(0, 2, 8, 3)),
                     Some(Rect::new(8, 2, 7, 3)),
                     Some(Rect::new(0, 5, 15, 4)),
                     Some(Rect::new(0, 9, 15, 1)) ])]
    #[case(6, vec![ Some(Rect::new(0, 0, 15, 2)),
                    Some(Rect::new(0, 2, 8, 3)),
                    Some(Rect::new(8, 2, 7, 3)),
                    None,
                    Some(Rect::new(0, 5, 15, 1)) ])]
    #[case(5, vec![ Some(Rect::new(0, 0, 15, 2)),
                    Some(Rect::new(0, 2, 8, 2)),
                    Some(Rect::new(8, 2, 7, 2)),
                    None,
                    Some(Rect::new(0, 4, 15, 1)) ])]
    #[case(3, vec![ Some(Rect::new(0, 0, 15, 2)),
                    None,
                    None,
                    None,
                    Some(Rect::new(0, 2, 15, 1)) ])]
    fn test_grid_pane(#[case] height: u16, #[case] expected: Vec<Option<Rect>>) {
        let screen = Rect::new(0, 0, 15, height);
        let w1 = MockWidget(2);
        let w21 = MockWidget(3);
        let w22 = MockWidget(1);
        let w3 = MockWidget(1);
        let rects = GridPane::new(screen)
            .with_row(&[&w1])
            .with_row(&[&w21, &w22])
            .with_line(&w3)
            .build();
        assert_eq!(expected, rects);
    }
}
