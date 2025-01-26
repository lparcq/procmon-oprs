// Oprs -- process monitor for Linux
// Copyright (C) 2024-2025  Laurent Pelecq
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

use ratatui::{
    layout::{Alignment, Constraint, Layout},
    prelude::*,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        StatefulWidget, Table, Widget, Wrap,
    },
    Frame,
};
use std::{cmp, fmt};

use super::{
    types::{Area, MaxLength},
    KeyMap, MenuEntry,
};

pub const BORDER_SIZE: u16 = 1;

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

/// Column constraint status
#[derive(Debug)]
enum ColumnStatus {
    Accepted,
    Truncated,
    Rejected,
}

/// Calculate widths constraints to avoid an overflow
#[derive(Debug)]
struct ColumnConstraints {
    constraints: Vec<Constraint>,
    inner_width: u16,
    max_column_width: u16,
    column_spacing: u16,
    remaining_width: u16,
}

impl ColumnConstraints {
    fn new(inner_width: u16, max_column_width: u16, column_spacing: u16) -> Self {
        Self {
            constraints: Vec::new(),
            inner_width,
            max_column_width,
            column_spacing,
            remaining_width: inner_width,
        }
    }

    /// Table width (the columns plus the gaps).
    fn table_width(&self) -> u16 {
        self.inner_width - self.remaining_width
    }

    /// Add a column in the constraints.
    ///
    /// Return true if it has been added and not truncated.
    fn add_column(&mut self, width: u16) -> ColumnStatus {
        let column_spacing = if self.constraints.is_empty() {
            0
        } else {
            self.column_spacing
        };
        let mut actual_width = cmp::min(self.max_column_width, width);
        let required_width = column_spacing + actual_width;
        if required_width <= self.remaining_width {
            self.constraints.push(Constraint::Length(actual_width));
            self.remaining_width -= required_width;
            ColumnStatus::Accepted
        } else if self.remaining_width > column_spacing {
            // Partial last column
            actual_width = self.remaining_width - column_spacing;
            self.constraints.push(Constraint::Length(actual_width));
            self.remaining_width = 0;
            ColumnStatus::Truncated
        } else {
            self.remaining_width = 0;
            ColumnStatus::Rejected
        }
    }
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
    fn apply<'a>(&self, mut rows: Vec<Vec<Cell<'a>>>) -> Vec<Row<'a>> {
        rows.drain(..)
            .enumerate()
            .map(|(i, r)| {
                let style = if i % 2 != 0 {
                    self.even_row
                } else {
                    self.odd_row
                };
                Row::new(r).style(style)
            })
            .collect::<Vec<Row>>()
    }
}

/// Visible part of an element in one direction.
#[derive(Debug, Default, Clone, Copy)]
pub struct Zoom {
    pub position: usize,
    pub visible_length: usize,
    pub total_length: usize,
}

impl Zoom {
    pub fn new(position: usize, visible_length: usize, total_length: usize) -> Self {
        Self {
            position,
            visible_length,
            total_length,
        }
    }

    pub fn with_position(position: usize) -> Self {
        Self::new(position, 0, 0)
    }

    /// Check if at end.
    pub fn at_end(&self) -> bool {
        self.position + self.visible_length >= self.total_length
    }

    /// Reframe the zoom to avoid empty space at the end.
    pub fn reframe(&mut self) {
        if self.position + self.visible_length > self.total_length {
            self.position = self.total_length.saturating_sub(self.visible_length);
        }
    }

    /// Create a scrollbar state if content is bigger than visible size.
    pub fn scrollbar_state(&self) -> Option<ScrollbarState> {
        if self.position > 0 || self.visible_length < self.total_length {
            Some(
                ScrollbarState::new(self.total_length.saturating_sub(self.visible_length))
                    .position(self.position)
                    .viewport_content_length(self.visible_length),
            )
        } else {
            None
        }
    }
}

/// Widget that adapt it's layout to the available space.
pub trait ReactiveWidget: fmt::Debug + Widget {
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

impl StatefulWidget for OneLineWidget<'_> {
    type State = Option<Position>;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        *state = state.map(|pos| Position::new(self.text_length, pos.y));
        Widget::render(self, area, buf);
    }
}

/// Scrollable long text that can exceed the screen height.
#[derive(Debug)]
pub(crate) struct MarkdownWidget<'l> {
    title: &'static str,
    text: Vec<Line<'l>>,
}

impl MarkdownWidget<'_> {
    pub(crate) fn new(title: &'static str, text: &'static str) -> Self {
        let text = format_text(text);
        Self { title, text }
    }
}

impl StatefulWidget for MarkdownWidget<'_> {
    type State = Zoom;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State)
    where
        Self: Sized,
    {
        let borders = BORDER_SIZE * 2;
        let inner_height = area.height - borders;
        let max_offset = self.text.len().saturating_sub(inner_height as usize / 2);
        state.position = cmp::min(state.position, max_offset);
        state.visible_length = inner_height as usize;
        let mut scroll_state = ScrollbarState::new(max_offset as usize).position(state.position);
        Paragraph::new(Text::from(self.text))
            .block(
                Block::new()
                    .title(format!(" {} ", self.title))
                    .title_alignment(Alignment::Center)
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false })
            .scroll((state.position as u16, 0))
            .render(area, buf);
        let inner_area = area.inner(Margin::new(0, BORDER_SIZE));
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None)
            .render(inner_area, buf, &mut scroll_state);
    }
}

/// State for the `BigTableWidget`.
#[derive(Debug)]
pub(crate) struct BigTableState {
    pub(crate) zoom: Area<Zoom>,
}

impl BigTableState {
    pub(crate) fn new(hzoom: Zoom, vzoom: Zoom) -> Self {
        Self {
            zoom: Area::new(hzoom, vzoom),
        }
    }
}

/// Table generator
pub(crate) trait TableGenerator {
    /// The number of fixed columns on the left and fixed rows on the top.
    ///
    /// If the width is not zero, it's a crosstab.
    fn headers_size(&self) -> Area<usize>;

    /// The visible headers.
    ///
    /// The fixed columns must always be included.
    fn top_headers(&self, zoom: &Zoom) -> Vec<Cell>;

    /// The visible rows.
    ///
    /// The fixed columns must always be included.
    fn rows(&self, zoom: &BigTableState) -> Vec<Vec<Cell>>;

    /// The width of each column.
    fn widths(&self) -> &[u16];
}

/// Table that can overflow horizontally and vertically.
pub(crate) struct BigTableWidget<'a, T: TableGenerator> {
    table: &'a T,
    style: TableStyle,
}

impl<'a, T: TableGenerator> BigTableWidget<'a, T> {
    pub(crate) fn new(table: &'a T, style: TableStyle) -> Self {
        Self { table, style }
    }
}

impl<'a, T: TableGenerator> StatefulWidget for BigTableWidget<'a, T> {
    type State = BigTableState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State)
    where
        Self: Sized,
    {
        let borders = BORDER_SIZE * 2;
        let outter_dim = Size::new(area.width, area.height);
        let inner_dim = Size::new(outter_dim.width - borders, outter_dim.height - borders);
        let widths = self.table.widths();
        // Max column width hard-coded to half the line width.
        let mut cc = ColumnConstraints::new(
            inner_dim.width,
            inner_dim.width / 2,
            self.style.column_spacing,
        );
        let headers_size = self.table.headers_size();
        let mut start = state.zoom.horizontal.position;
        let mut index = 0;
        let mut headers_width = 0;
        while index < widths.len() {
            if index == headers_size.horizontal {
                headers_width = cc.table_width() + cc.column_spacing;
                index += state.zoom.horizontal.position;
                if index >= widths.len() {
                    log::error!(
                        "first column index {index} exceeds the number of columns {}",
                        widths.len()
                    );
                    index = widths.len() - 1;
                }
                start = index;
            }
            match cc.add_column(widths[index]) {
                ColumnStatus::Truncated | ColumnStatus::Rejected => break,
                ColumnStatus::Accepted => index += 1,
            }
        }
        state.zoom.horizontal.visible_length = index - start;
        state.zoom.vertical.visible_length =
            (inner_dim.height as usize).saturating_sub(headers_size.vertical);
        state.zoom.vertical.reframe();
        let headers = self.table.top_headers(&state.zoom.horizontal);
        let rows = self.style.apply(self.table.rows(state));

        let table = Table::new(rows, cc.constraints)
            .block(Block::default().borders(Borders::ALL))
            .header(Row::new(headers).height(headers_size.vertical as u16))
            .column_spacing(self.style.column_spacing);
        Widget::render(table, area, buf);
        if let Some(mut bar_state) = state.zoom.horizontal.scrollbar_state() {
            let x = area.x + BORDER_SIZE + headers_width;
            let width = area.width.saturating_sub(x + BORDER_SIZE);
            let area = Rect::new(x, area.y, width, area.height);
            Scrollbar::new(ScrollbarOrientation::HorizontalTop)
                .begin_symbol(None)
                .end_symbol(None)
                .render(area, buf, &mut bar_state);
        }
        if let Some(mut bar_state) = state.zoom.vertical.scrollbar_state() {
            let y = area.y + BORDER_SIZE + headers_size.vertical as u16;
            let height = state.zoom.vertical.visible_length as u16;
            if state.zoom.vertical.total_length > 0 {
                let area = Rect::new(area.x, y, area.width, height);
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .render(area, buf, &mut bar_state);
            }
        }
        state.zoom.vertical.visible_length = inner_dim.height as usize;
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

    pub(crate) fn with_row_if<W: ReactiveWidget>(self, row: &[&W], cond: bool) -> Self {
        if cond {
            self.with_row(row)
        } else {
            self
        }
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

    pub(crate) fn render_stateful_widget<W: StatefulWidget>(
        &mut self,
        widget: W,
        state: &mut W::State,
    ) -> bool {
        match self.iter.next() {
            Some(Some(rect)) => {
                self.frame.render_stateful_widget(widget, rect, state);
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

    use super::{
        ColumnConstraints, ColumnStatus, GridPane, Pane, ReactiveWidget, SingleScrollablePane,
    };

    /// Create a column constraints object and feed it.
    ///
    /// A columns are alike except the first column.
    fn new_column_constraints(
        screen_width: u16,
        max_col_width: u16,
        column_spacing: u16,
        ncols: usize,
        first_column_width: u16,
        column_width: u16,
    ) -> (ColumnConstraints, ColumnStatus) {
        let mut cc = ColumnConstraints::new(screen_width, max_col_width, column_spacing);
        for width in vec![first_column_width]
            .iter()
            .chain(vec![column_width; ncols - 1].iter())
        {
            match cc.add_column(*width) {
                ColumnStatus::Accepted => (),
                status => return (cc, status),
            }
        }
        (cc, ColumnStatus::Accepted)
    }

    #[test]
    fn test_column_constraints_underflow() {
        // 0         1
        // 01234567890123456789
        // aaaaa bbbb bbbb
        const SCREEN_WIDTH: u16 = 20;
        const FIRST_COLUMN_WIDTH: u16 = 5;
        const COLUMN_WIDTH: u16 = 4;
        const COLUMN_SPACING: u16 = 1;
        const NCOLS: usize = 3;
        let (cc, status) = new_column_constraints(
            SCREEN_WIDTH,
            SCREEN_WIDTH,
            COLUMN_SPACING,
            NCOLS,
            FIRST_COLUMN_WIDTH,
            COLUMN_WIDTH,
        );
        assert!(matches!(status, ColumnStatus::Accepted));
        const SPACED_COLUMN_WIDTH: u16 = COLUMN_WIDTH + COLUMN_SPACING;
        const EXPECTED_WIDTH: u16 = FIRST_COLUMN_WIDTH + (NCOLS as u16 - 1) * SPACED_COLUMN_WIDTH;
        assert_eq!(EXPECTED_WIDTH, cc.inner_width - cc.remaining_width);
        assert_eq!(NCOLS, cc.constraints.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), cc.constraints[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), cc.constraints[1]);
    }

    #[test]
    fn test_column_constraints_exact() {
        // 0         1
        // 01234567890123456789
        // aaaaa bbbb bbbb bbbb
        const SCREEN_WIDTH: u16 = 20;
        const FIRST_COLUMN_WIDTH: u16 = 5;
        const COLUMN_WIDTH: u16 = 4;
        const COLUMN_SPACING: u16 = 1;
        const NCOLS: usize = 4;
        let (cc, status) = new_column_constraints(
            SCREEN_WIDTH,
            SCREEN_WIDTH,
            COLUMN_SPACING,
            NCOLS,
            FIRST_COLUMN_WIDTH,
            COLUMN_WIDTH,
        );
        assert!(matches!(status, ColumnStatus::Accepted));
        const SPACED_COLUMN_WIDTH: u16 = COLUMN_WIDTH + COLUMN_SPACING;
        let expected_width: u16 = FIRST_COLUMN_WIDTH + (NCOLS as u16 - 1) * SPACED_COLUMN_WIDTH;
        const EXPECTED_NCOLS: usize = 4;
        assert_eq!(expected_width, cc.inner_width);
        assert_eq!(0, cc.remaining_width);
        assert_eq!(EXPECTED_NCOLS, cc.constraints.len());
        assert_eq!(Constraint::Length(FIRST_COLUMN_WIDTH), cc.constraints[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), cc.constraints[1]);
    }

    #[rstest]
    #[case(5, 5, 5, 2, true)]
    #[case(6, 4, 5, 3, false)]
    #[case(5, 7, 4, 3, false)]
    fn test_column_constraints_overflow(
        #[case] ncols: usize,
        #[case] first_column_width: u16,
        #[case] expected_ncols: usize,
        #[case] expected_last_width: u16,
        #[case] truncated: bool,
    ) {
        //    0         1
        //    01234567890123456789
        // #1 aaaaa bbb bbb bbb bbb
        // #2 aaaa bbb bbb bbb bbb bbb
        // #1 aaaaaaa bbb bbb bbb bbb
        const SCREEN_WIDTH: u16 = 20;
        const COLUMN_WIDTH: u16 = 3;
        const COLUMN_SPACING: u16 = 1;
        let (cc, status) = new_column_constraints(
            SCREEN_WIDTH,
            SCREEN_WIDTH,
            COLUMN_SPACING,
            ncols,
            first_column_width,
            COLUMN_WIDTH,
        );
        match status {
            ColumnStatus::Truncated if truncated => (),
            ColumnStatus::Rejected if !truncated => (),
            status => panic!("invalid status {status:?}"),
        }
        assert_eq!(SCREEN_WIDTH, cc.inner_width);
        assert_eq!(0, cc.remaining_width);
        assert_eq!(expected_ncols, cc.constraints.len());
        assert_eq!(Constraint::Length(first_column_width), cc.constraints[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), cc.constraints[1]);
        assert_eq!(
            Constraint::Length(expected_last_width),
            cc.constraints.last().unwrap().clone()
        );
    }

    #[test]
    fn test_column_constraints_max_col_width() {
        // 0         1
        // 01234567890123456789
        // aaaaaaa bbb bbb bbb
        const SCREEN_WIDTH: u16 = 20;
        const MAX_COL_WIDTH: u16 = 7;
        const FIRST_COLUMN_WIDTH: u16 = 12;
        const COLUMN_WIDTH: u16 = 3;
        const COLUMN_SPACING: u16 = 1;
        const NCOLS: usize = 4;
        let (cc, status) = new_column_constraints(
            SCREEN_WIDTH,
            MAX_COL_WIDTH,
            COLUMN_SPACING,
            NCOLS,
            FIRST_COLUMN_WIDTH,
            COLUMN_WIDTH,
        );
        assert!(
            matches!(status, ColumnStatus::Accepted),
            "invalid status {status:?}",
        );
        const EXPECTED_NCOLS: usize = 4;
        assert_eq!(
            SCREEN_WIDTH - COLUMN_SPACING,
            cc.inner_width - cc.remaining_width
        );
        assert_eq!(EXPECTED_NCOLS, cc.constraints.len());
        assert_eq!(Constraint::Length(MAX_COL_WIDTH), cc.constraints[0]);
        assert_eq!(Constraint::Length(COLUMN_WIDTH), cc.constraints[1]);
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
