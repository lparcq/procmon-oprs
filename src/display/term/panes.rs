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

use getset::CopyGetters;
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout},
    prelude::*,
    style::{Modifier, Style},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState,
        StatefulWidget, Table, Widget, Wrap,
    },
};
use std::{borrow::Cow, cmp, fmt, ops::Range};

use super::{
    input::MenuEntry,
    types::{Area, MaxLength, Motion, Scroll},
};

pub(crate) const BORDER_SIZE: u16 = 1;

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
        let position = cmp::min(position, total_length);
        Self {
            position,
            visible_length,
            total_length,
        }
    }

    pub fn with_position(position: usize) -> Self {
        Self::new(position, 0, 0)
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

/// Widget that have a minimum height when displayed in rows.
pub trait StackableWidget: fmt::Debug + Widget {
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

    pub(crate) fn with_menu<'a, I>(entries: I) -> Self
    where
        I: Iterator<Item = &'a MenuEntry>,
    {
        let mut spans = Vec::new();
        let mut sep = "";
        entries.into_iter().for_each(|entry| {
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

impl StackableWidget for OneLineWidget<'_> {
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
        let mut scroll_state = ScrollbarState::new(max_offset).position(state.position);
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
#[derive(Debug, CopyGetters)]
pub(crate) struct BigTableState {
    #[getset(get_copy = "pub")]
    motion: Area<Motion>,
    selected_lineno: Option<usize>,
    min_lineno: usize,
    zoom: Area<Zoom>,
}

impl BigTableState {
    pub(crate) fn new(
        motion: &Area<Motion>,
        selected_lineno: Option<usize>,
        min_lineno: usize,
    ) -> Self {
        Self {
            motion: *motion,
            selected_lineno,
            min_lineno,
            zoom: Area::default(),
        }
    }

    pub(crate) fn with_motion(motion: &Area<Motion>) -> Self {
        Self::new(motion, None, 0)
    }
}

/// Manage the visible part of the big table.
#[derive(Debug)]
pub(crate) struct TableClip<'a, 'b> {
    /// Table state.
    state: &'a BigTableState,
    /// Widths of columns.
    widths: &'b [u16],
    /// Number of header columns.
    nheadcols: usize,
    /// Range of body columns
    ranges: Vec<Range<usize>>,
}

impl<'a, 'b> TableClip<'a, 'b> {
    fn new(
        state: &'a BigTableState,
        widths: &'b [u16],
        nheadcols: usize,
        column_spacing: u16,
    ) -> Self {
        let column_spacing = column_spacing as usize;
        let body_widths = &widths[nheadcols..];
        let mut ranges = Vec::with_capacity(body_widths.len());
        let mut start = 0;
        for width in body_widths {
            let end = start + *width as usize;
            ranges.push(start..end);
            start = end + column_spacing;
        }
        Self {
            state,
            widths,
            nheadcols,
            ranges,
        }
    }

    pub(crate) fn zoom(&self) -> &Area<Zoom> {
        &self.state.zoom
    }

    pub(crate) fn selected_lineno(&self) -> Option<usize> {
        self.state.selected_lineno
    }

    /// Constraints for visible columns.
    fn constraints(&self) -> Vec<Constraint> {
        let header_widths = &self.widths[0..self.nheadcols];
        let mut constraints = Vec::with_capacity(self.widths.len());
        header_widths
            .iter()
            .for_each(|w| constraints.push(Constraint::Length(*w)));
        let hclip = self.zoom().horizontal;
        let start = hclip.position;
        let end = hclip.position + hclip.visible_length;
        constraints.extend(
            self.ranges
                .iter()
                .skip_while(|r| r.end <= start)
                .take_while(|r| r.start < end)
                .map(|r| {
                    let len = cmp::min(end, r.end) - cmp::max(start, r.start);
                    Constraint::Length(len as u16)
                }),
        );
        constraints
    }

    /// Create a text with only the visible part of a cell.
    ///
    /// * `colnum`: the body column number (starting on the first column after the header)
    /// * `value`: the string to display
    /// * `alignment`: the alignment in the cell.
    pub(crate) fn clip_cell<'t>(
        &self,
        colnum: usize,
        value: Cow<'t, str>,
        mut alignment: Alignment,
    ) -> Option<Text<'t>> {
        let widths = &self.widths[self.nheadcols..];
        let range = &self.ranges[colnum];
        let hclip = self.zoom().horizontal;
        let start = hclip.position;
        let end = hclip.position + hclip.visible_length;
        if range.end <= start || range.start >= end {
            None
        } else if range.start < start {
            let truncation = start - range.start;
            match alignment {
                Alignment::Left => Some(Text::from_iter(
                    value.lines().map(|l| Self::suffix(l, truncation)),
                )),
                Alignment::Right => {
                    let width = widths[colnum] as usize;
                    Some(Text::from_iter(value.lines().map(|l| {
                        let offset = (truncation + l.len()).saturating_sub(width);
                        Self::suffix(l, offset)
                    })))
                }
                Alignment::Center => {
                    alignment = Alignment::Left;
                    let width = widths[colnum] as usize;
                    Some(Self::truncate_centered(value, truncation, width, alignment))
                }
            }
        } else if range.end > end {
            let truncation = range.end - end;
            match alignment {
                Alignment::Left => {
                    let len = widths[colnum] as usize - truncation;
                    Some(Text::from_iter(value.lines().map(|l| Self::prefix(l, len))))
                }
                Alignment::Right => {
                    let len = value.len().saturating_sub(truncation);
                    Some(Text::from(value.get(..len).unwrap_or("").to_owned()))
                }
                Alignment::Center => {
                    alignment = Alignment::Right;
                    let width = widths[colnum] as usize;
                    Some(Self::truncate_centered(value, truncation, width, alignment))
                }
            }
        } else {
            Some(Text::from(value.into_owned()))
        }
        .map(|t| t.alignment(alignment))
    }

    fn prefix(s: &str, len: usize) -> String {
        if s.len() <= len {
            s
        } else {
            s.get(..len).unwrap_or("")
        }
        .to_owned()
    }

    fn suffix(s: &str, offset: usize) -> String {
        s.get(offset..).unwrap_or("").to_owned()
    }

    fn truncate_centered(
        value: Cow<'_, str>,
        truncation: usize,
        width: usize,
        alignement: Alignment,
    ) -> Text<'_> {
        let len = width - truncation;
        Text::from_iter(value.lines().map(|l| {
            let llen = l.len();
            let indent = if width >= llen {
                (width - llen) / 2
            } else {
                log::error!("text \"{l}\" centered length {llen} is larger than column {width}");
                0
            };
            let llen = llen + indent;
            let s = match alignement {
                Alignment::Left => format!("{l: <w$}", w = llen),
                Alignment::Right => format!("{l: >w$}", w = llen),
                _ => panic!("must be called with left or right alignment"),
            };
            Self::prefix(&s, len).trim().to_owned()
        }))
    }
}

/// Table generator
pub(crate) trait TableGenerator {
    /// The number of fixed columns on the left and fixed rows on the top.
    ///
    /// If the width is not zero, it's a crosstab.
    fn headers_size(&self) -> Area<usize>;

    /// The headers on top.
    fn top_headers(&self, clip: &TableClip<'_, '_>) -> Vec<Cell>;

    /// The visible rows.
    ///
    /// * `zoom` - the visibles rows start index and size.
    fn rows(&self, clip: &TableClip<'_, '_>) -> Vec<Vec<Cell>>;

    /// Number of rows in the body.
    fn body_row_count(&self) -> usize;

    /// The width of each column.
    fn widths(&self) -> &[u16];

    /// Number of columns in the body.
    fn _body_column_count(&self) -> usize {
        self.widths().len() - self.headers_size().horizontal
    }

    /// Calculate the width of a range of columns including the space between them.
    fn range_widths(&self, range: std::ops::Range<usize>, column_spacing: u16) -> u16 {
        let count = range.end - range.start;
        self.widths()
            .iter()
            .skip(range.start)
            .take(count)
            .copied()
            .sum::<u16>()
            + count.saturating_sub(1) as u16 * column_spacing
    }
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

    fn move_position(
        position: usize,
        last_position: usize,
        page_length: usize,
        scroll: Scroll,
    ) -> usize {
        match scroll {
            Scroll::CurrentPosition => position,
            Scroll::FirstPosition => 0,
            Scroll::LastPosition => last_position,
            Scroll::PreviousPosition => position.saturating_sub(1),
            Scroll::NextPosition => cmp::min(last_position, position + 1),
            Scroll::PreviousPage => position.saturating_sub(page_length),
            Scroll::NextPage => cmp::min(last_position, position + page_length),
        }
    }

    /// Move the position and adapt the zoom so that the position is visible.
    ///
    /// If a position is given, it is moved and the zoom also if required.
    /// If there is no position, the position is set at the beginning or the
    /// end of the visible area.
    fn new_zoom(
        position: Option<usize>,
        min_position: usize,
        motion: &Motion,
        visible_length: u16,
        total_length: u16,
    ) -> (Option<usize>, Zoom) {
        let visible_length = visible_length as usize;
        let total_length = total_length as usize;
        let page_length = visible_length.div_ceil(2);
        let (position, top) = match position {
            Some(position) => {
                let last_position = total_length.saturating_sub(1);
                let position =
                    Self::move_position(position, last_position, page_length, motion.scroll);
                let top =
                    if position < motion.position || position >= motion.position + visible_length {
                        position.saturating_sub(page_length)
                    } else {
                        motion.position
                    };
                (Some(position), top)
            }
            None => {
                let top = motion.position;
                match motion.scroll {
                    Scroll::PreviousPosition => (Some(top + visible_length.saturating_sub(1)), top),
                    Scroll::NextPosition => (Some(cmp::max(top, min_position)), top),
                    _ => {
                        let last_position = total_length.saturating_sub(visible_length);
                        let top =
                            Self::move_position(top, last_position, page_length, motion.scroll);
                        (None, top)
                    }
                }
            }
        };
        (
            position.map(|p| cmp::max(p, min_position)),
            Zoom::new(top, visible_length, total_length),
        )
    }
}

impl<T: TableGenerator> StatefulWidget for BigTableWidget<'_, T> {
    type State = BigTableState;

    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State)
    where
        Self: Sized,
    {
        let borders = BORDER_SIZE * 2;
        let widths = self.table.widths();
        let column_spacing = self.style.column_spacing;
        let Area {
            horizontal: nheadcols,
            vertical: nheadrows,
        } = self.table.headers_size();
        let headers_width = self.table.range_widths(0..nheadcols, column_spacing) + column_spacing;
        let body_width = self
            .table
            .range_widths(nheadcols..widths.len(), column_spacing);
        let visible_width = area
            .width
            .saturating_sub(borders)
            .saturating_sub(headers_width);
        let body_height = self.table.body_row_count() as u16;
        let visible_height = area
            .height
            .saturating_sub(borders)
            .saturating_sub(nheadrows as u16);
        let hzoom = Zoom::new(
            Self::move_position(
                state.motion.horizontal.position,
                body_width.saturating_sub(visible_width) as usize,
                visible_width.div_ceil(2) as usize,
                state.motion.horizontal.scroll,
            ),
            visible_width as usize,
            body_width as usize,
        );
        state.zoom.horizontal = hzoom;
        let (selected_lineno, vzoom) = Self::new_zoom(
            state.selected_lineno,
            state.min_lineno,
            &state.motion.vertical,
            visible_height,
            body_height,
        );
        state.selected_lineno = selected_lineno;
        state.zoom.vertical = vzoom;
        let clip = TableClip::new(state, widths, nheadcols, column_spacing);

        let constraints = clip.constraints();
        let headers = self.table.top_headers(&clip);
        let rows = self.style.apply(self.table.rows(&clip));

        let table = {
            let table = Table::new(rows, constraints)
                .block(Block::default().borders(Borders::ALL))
                .column_spacing(self.style.column_spacing);
            if headers.is_empty() {
                table
            } else {
                table.header(Row::new(headers).height(nheadrows as u16))
            }
        };
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
            let y = area.y + BORDER_SIZE + nheadrows as u16;
            let height = state.zoom.vertical.visible_length as u16;
            if state.zoom.vertical.total_length > 0 {
                let area = Rect::new(area.x, y, area.width, height);
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(None)
                    .end_symbol(None)
                    .render(area, buf, &mut bar_state);
            }
        }
        state.motion.horizontal.move_to(hzoom.position);
        state.motion.vertical.move_to(vzoom.position);
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

impl StackableWidget for FieldsWidget<'_> {
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
    pub(crate) fn with<W: StackableWidget>(mut self, widget: &W) -> Self {
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

    pub(crate) fn with_row<W: StackableWidget>(mut self, row: &[&W]) -> Self {
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

    pub(crate) fn with_row_if<W: StackableWidget>(self, row: &[&W], cond: bool) -> Self {
        if cond { self.with_row(row) } else { self }
    }

    pub(crate) fn with_line<W: StackableWidget>(mut self, widget: &W) -> Self {
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

    use ratatui::{
        buffer::Buffer,
        layout::{Alignment, Constraint, Rect},
        text::Text,
        widgets::Widget,
    };
    use rstest::*;
    use std::{borrow::Cow, cmp};

    use crate::display::term::types::{Area, MaxLength};

    use super::{
        BigTableState, GridPane, Pane, SingleScrollablePane, StackableWidget, TableClip, Zoom,
    };

    const COLUMN_SPACING: u16 = 1;

    /// State and widths from a rows of strings.
    fn new_state_and_widths(
        rows: &[&[&'static str]],
        position: usize,
        screen_width: usize,
        nheadcols: usize,
        column_spacing: u16,
    ) -> (BigTableState, Vec<u16>) {
        let column_spacing = column_spacing as usize;
        let mut mw = (0..rows[0].len())
            .map(|_| MaxLength::from(0))
            .collect::<Vec<_>>();
        rows.iter()
            .for_each(|row| row.iter().enumerate().for_each(|(i, s)| mw[i].check(s)));
        let widths = mw.iter().map(MaxLength::len).collect::<Vec<_>>();
        let body_widths = &widths[nheadcols..];
        let total_width = body_widths.iter().sum::<u16>() as usize
            + body_widths.len().saturating_sub(1) * column_spacing;
        let visible_width = screen_width
            - widths.iter().take(nheadcols).sum::<u16>() as usize
            - nheadcols * column_spacing;
        let zoom = Area::new(
            Zoom::new(position, visible_width, total_width),
            Zoom::default(),
        );
        let state = BigTableState {
            motion: Area::default(),
            selected_lineno: None,
            min_lineno: 0,
            zoom,
        };
        (state, widths)
    }

    fn new_constraints(widths: &[u16]) -> Vec<Constraint> {
        widths
            .iter()
            .copied()
            .map(Constraint::Length)
            .collect::<Vec<_>>()
    }

    /// 0         1
    /// 01234567890123456789
    /// abcde  fgh ijkl
    /// ABC   DEFG   HI
    const ROWS_5_4_4: &[&[&str]] = &[&["abcde", "fgh", "ijkl"], &["ABC", "DEFG", "HI"]];

    /// 0         1
    /// 01234567890123456789
    /// abcde  fgh ijkl  mno
    /// ABC   DEFG   HI JKLM
    const ROWS_5_4_4_4: &[&[&str]] = &[
        &["abcde", "fgh", "ijkl", "mno"],
        &["ABC", "DEFG", "HI", "JKLM"],
    ];

    #[rstest]
    #[case(ROWS_5_4_4, &[5, 4, 4])]
    #[case(ROWS_5_4_4_4, &[5, 4, 4, 4])]
    fn test_constraints_without_headers_and_clip(
        #[case] rows: &[&[&'static str]],
        #[case] expected_constraints: &[u16],
    ) {
        const SCREEN_WIDTH: usize = 20;
        const NHEADCOLS: usize = 0;
        let (zoom, widths) = new_state_and_widths(rows, 0, SCREEN_WIDTH, NHEADCOLS, COLUMN_SPACING);
        let tc = TableClip::new(&zoom, &widths, NHEADCOLS, COLUMN_SPACING);
        let expected_constraints = new_constraints(expected_constraints);
        let constraints = tc.constraints();
        assert_eq!(expected_constraints, constraints);
        for row in rows {
            for (colnum, cell) in row.iter().enumerate() {
                let (alignment, expected) = if colnum == 0 {
                    (Alignment::Left, Text::from(*cell).left_aligned())
                } else {
                    (Alignment::Right, Text::from(*cell).right_aligned())
                };
                let value = tc
                    .clip_cell(colnum, Cow::Borrowed(cell), alignment)
                    .expect("not empty cell");
                assert_eq!(expected, value);
            }
        }
    }

    fn assert_bodies_match(
        tc: &TableClip,
        rows: &[&[&'static str]],
        alignment: Alignment,
        expected_rows: &[&[&'static str]],
        expected_alignments: &[Alignment],
        ncols: usize,
        nheadcols: usize,
    ) {
        let body_size = ncols - nheadcols;
        for (row, expected_row) in rows.iter().zip(expected_rows) {
            let expected = expected_row
                .iter()
                .zip(expected_alignments)
                .map(|(t, a)| Text::from(*t).alignment(*a))
                .collect::<Vec<_>>();
            let value = (0..body_size)
                .flat_map(|offset| {
                    tc.clip_cell(offset, Cow::Borrowed(row[offset + nheadcols]), alignment)
                })
                .collect::<Vec<_>>();
            assert_eq!(expected, value);
        }
    }

    const ROWS_5_4_4_5: &[&[&str]] = &[
        &["abcde", "fgh", "ijkl", "mnopqr"],
        &["ABC", "DEFG", "HI", "JKLM"],
    ];

    /// 0         1
    /// 01234567890123456789
    /// abcde  fgh ijkl mnopqr
    /// ABC   DEFG   HI   JKLM
    const EXPECTED_BODY_CASE_1: &[&[&str]] = &[&["fgh", "ijkl", "mnop"], &["DEFG", "HI", "JK"]];

    /// 0         1
    /// 01234567890123456789
    /// abcde fgh  ijkl mnopqr
    /// ABC   DEFG HI   JKLM
    const EXPECTED_BODY_CASE_2: &[&[&str]] = &[&["fgh", "ijkl", "mnop"], &["DEFG", "HI", "JKLM"]];

    const ROWS_5_4_2_9: &[&[&str]] = &[
        &["abcde", "fgh", "ij", "klmnopqrs"],
        &["ABC", "DEFG", "HI", "JKLMN"],
    ];

    /// 0         1
    /// 01234567890123456789
    /// abcde  fgh ij klmnopqrs
    /// ABC   DEFG HI   JKLMN
    const EXPECTED_BODY_CASE_3: &[&[&str]] = &[&["fgh", "ij", "klmnop"], &["DEFG", "HI", "JKLM"]];

    /// Columns truncated on the right.
    #[rstest]
    #[case(ROWS_5_4_4_5, Alignment::Right, &[5, 4, 4, 4],
           EXPECTED_BODY_CASE_1, &[Alignment::Right, Alignment::Right, Alignment::Right])]
    #[case(ROWS_5_4_4_5, Alignment::Left, &[5, 4, 4, 4],
           EXPECTED_BODY_CASE_2, &[Alignment::Left, Alignment::Left, Alignment::Left])]
    #[case(ROWS_5_4_2_9, Alignment::Center, &[5, 4, 2, 6],
           EXPECTED_BODY_CASE_3, &[Alignment::Center, Alignment::Center, Alignment::Right])]
    fn test_constraints_right_truncated_without_clip(
        #[case] rows: &[&[&'static str]],
        #[case] alignment: Alignment,
        #[case] expected_constraints: &[u16],
        #[case] expected_rows: &[&[&'static str]],
        #[case] expected_alignments: &[Alignment],
    ) {
        const SCREEN_WIDTH: usize = 20;
        const NHEADCOLS: usize = 1;
        let (state, widths) =
            new_state_and_widths(rows, 0, SCREEN_WIDTH, NHEADCOLS, COLUMN_SPACING);
        let tc = TableClip::new(&state, &widths, NHEADCOLS, COLUMN_SPACING);
        let expected_constraints = new_constraints(expected_constraints);
        let constraints = tc.constraints();
        assert_eq!(expected_constraints, constraints);
        assert_bodies_match(
            &tc,
            rows,
            alignment,
            expected_rows,
            expected_alignments,
            widths.len(),
            NHEADCOLS,
        );
    }

    const ROWS_5_4_5_7: &[&[&str]] = &[
        &["abcde", "fgh", "ijklm", "nopqrst"],
        &["ABC", "DEFG", "HI", "JKLM"],
    ];

    /// 0         1
    /// 01234567890123456789
    /// abcde h  ijklm nopqrst
    /// ABC   FG HI    JKLM
    const EXPECTED_ROWS_CLIP_CASE_1: &[&[&str]] =
        &[&["h", "ijklm", "nopqr"], &["FG", "HI", "JKLM"]];

    /// 0         1
    /// 01234567890123456789
    /// abcde gh ijklm nopqrst   abcde h  ijkl mnopqrst
    ///   ABC FG  HI      JKLM   ABC   FG  HI    JKLM
    const EXPECTED_ROWS_CLIP_CASE_2: &[&[&str]] = &[&["gh", "ijklm", "nopqr"], &["FG", "HI", "JK"]];

    /// Columns truncated on the left.
    #[rstest]
    #[case(ROWS_5_4_5_7, 2, Alignment::Left, &[5, 2, 5, 5], EXPECTED_ROWS_CLIP_CASE_1)]
    #[case(ROWS_5_4_5_7, 2, Alignment::Right, &[5, 2, 5, 5], EXPECTED_ROWS_CLIP_CASE_2)]
    fn test_constraints_with_clip(
        #[case] rows: &[&[&'static str]],
        #[case] position: usize,
        #[case] alignment: Alignment,
        #[case] expected_constraints: &[u16],
        #[case] expected_rows: &[&[&'static str]],
    ) {
        const SCREEN_WIDTH: usize = 20;
        const NHEADCOLS: usize = 1;
        let (state, widths) =
            new_state_and_widths(rows, position, SCREEN_WIDTH, NHEADCOLS, COLUMN_SPACING);
        let tc = TableClip::new(&state, &widths, NHEADCOLS, COLUMN_SPACING);
        let expected_constraints = new_constraints(expected_constraints);
        let constraints = tc.constraints();
        assert_eq!(expected_constraints, constraints);
        let body_size = widths.len() - NHEADCOLS;
        assert_bodies_match(
            &tc,
            rows,
            alignment,
            expected_rows,
            &(0..body_size).map(|_| alignment).collect::<Vec<_>>(),
            widths.len(),
            NHEADCOLS,
        );
    }

    #[derive(Debug)]
    struct MockWidget(u16);

    impl StackableWidget for MockWidget {
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
