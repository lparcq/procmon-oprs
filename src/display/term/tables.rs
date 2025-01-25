// Oprs -- process monitor for Linux
// Copyright (C) 2025  Laurent Pelecq
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

use getset::Getters;
use itertools::izip;
use libc::pid_t;
use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style},
    text::Text,
    widgets::Cell,
};
use std::{cmp::Ordering, collections::BTreeSet, rc::Rc};

use super::{
    input::Bookmarks,
    panes::{DoubleZoom, TableGenerator, Zoom},
    types::{Area, MaxLength},
};

use crate::{
    console::BuiltinTheme,
    process::{Collector, ProcessIdentity, ProcessSamples},
};

/// Right aligned cell.
macro_rules! rcell {
    ($s:expr) => {
        Cell::from(Text::from($s).alignment(Alignment::Right))
    };
}

/// Status of a process.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PidStatus {
    /// No specific status.
    Unknown,
    /// Under the cursor.
    Selected,
    /// Bookmarked.
    Marked,
    /// Search match.
    Matching,
}

/// Theme styles
#[derive(Debug)]
pub(crate) struct Styles {
    /// Even rows
    pub(crate) even_row: Style,
    /// Odd rows
    pub(crate) odd_row: Style,
    /// Increasing value
    pub(crate) increase: Style,
    /// Decreasing value
    pub(crate) decrease: Style,
    /// Unselected line
    pub(crate) unselected: Style,
    /// Selected line
    pub(crate) selected: Style,
    /// Bookmarked line.
    pub(crate) marked: Style,
    /// Matching line
    pub(crate) matching: Style,
    /// Status line
    pub(crate) status: Style,
    /// Space between columns in number of characters
    pub(crate) column_spacing: u16,
}

impl Styles {
    pub(crate) fn new(theme: Option<BuiltinTheme>) -> Self {
        let default_style = Style::default();
        let bold = Style::default().add_modifier(Modifier::BOLD);
        let bold_reversed = bold.add_modifier(Modifier::REVERSED);
        let white_on_blue = Style::default().fg(Color::White).bg(Color::Blue);
        match theme {
            Some(BuiltinTheme::Dark) => Styles {
                even_row: default_style,
                odd_row: Style::default().bg(Color::Indexed(238)),
                increase: Style::default().fg(Color::Indexed(196)),
                decrease: Style::default().fg(Color::Indexed(46)),
                unselected: bold,
                selected: Style::default().fg(Color::Black).bg(Color::LightMagenta),
                marked: Style::default().fg(Color::LightCyan),
                matching: Style::default().fg(Color::LightMagenta),
                status: white_on_blue,
                column_spacing: 2,
            },
            Some(BuiltinTheme::Light) => Styles {
                even_row: default_style,
                odd_row: Style::default().bg(Color::Indexed(254)),
                increase: Style::default().fg(Color::Indexed(124)),
                decrease: Style::default().fg(Color::Indexed(40)),
                unselected: bold,
                selected: Style::default().fg(Color::White).bg(Color::Magenta),
                marked: Style::default().fg(Color::Cyan),
                matching: Style::default().fg(Color::Magenta),
                status: white_on_blue,
                column_spacing: 2,
            },
            Some(BuiltinTheme::Dark16) => Styles {
                even_row: default_style,
                odd_row: default_style,
                increase: Style::default().fg(Color::LightMagenta),
                decrease: Style::default().fg(Color::LightGreen),
                unselected: bold,
                selected: Style::default().fg(Color::Black).bg(Color::LightMagenta),
                marked: Style::default().fg(Color::LightCyan),
                matching: Style::default().fg(Color::LightMagenta),
                status: white_on_blue,
                column_spacing: 2,
            },
            Some(BuiltinTheme::Light16) => Styles {
                even_row: default_style,
                odd_row: default_style,
                increase: Style::default().fg(Color::Red),
                decrease: Style::default().fg(Color::Green),
                unselected: bold,
                selected: Style::default().fg(Color::White).bg(Color::Magenta),
                marked: Style::default().fg(Color::Cyan),
                matching: Style::default().fg(Color::Magenta),
                status: white_on_blue,
                column_spacing: 2,
            },
            None => Styles {
                even_row: default_style,
                odd_row: default_style,
                increase: bold,
                decrease: bold,
                unselected: bold,
                selected: bold_reversed,
                marked: bold.add_modifier(Modifier::UNDERLINED),
                matching: Style::default().add_modifier(Modifier::UNDERLINED),
                status: bold_reversed,
                column_spacing: 2,
            },
        }
    }

    fn name_style(&self, status: PidStatus) -> Style {
        match status {
            PidStatus::Unknown => self.unselected,
            PidStatus::Selected => self.selected,
            PidStatus::Marked => self.marked,
            PidStatus::Matching => self.matching,
        }
    }

    fn trend_style(&self, trend: &Ordering) -> Style {
        match trend {
            Ordering::Less => self.decrease,
            Ordering::Equal => Style::default(),
            Ordering::Greater => self.increase,
        }
    }
}

/// Stack of parent child PIDs
#[derive(Default)]
struct PidStack(Vec<pid_t>);

impl PidStack {
    /// Stack len
    fn len(&self) -> usize {
        self.0.len()
    }

    /// Pop pids that are not a parent of the given process and push the new pid on the stack.
    fn push(&mut self, samples: &ProcessSamples) {
        let Self(ref mut stack) = self;
        match samples.parent_pid() {
            Some(parent_pid) => {
                while let Some(top_pid) = stack.last() {
                    if *top_pid == parent_pid {
                        break;
                    }
                    let _ = stack.pop();
                }
                stack.push(samples.pid());
            }
            None => stack.clear(), // Cannot happened. Only the system has no parent.
        }
    }
}

/// Data used to generate the tree as a table.
#[derive(Debug)]
pub(crate) struct TreeData<'t> {
    /// Column headers for metrics
    pub(crate) metric_headers: Vec<Text<'t>>,
    /// Display styles
    pub(crate) styles: Styles,
    /// Bookmarks for PIDs.
    pub(crate) bookmarks: Bookmarks,
    /// PID matched by a search.
    pub(crate) occurrences: BTreeSet<pid_t>,
}

impl<'t> TreeData<'t> {
    pub(crate) fn new(styles: Styles) -> Self {
        Self {
            metric_headers: Vec::new(),
            styles,
            bookmarks: Bookmarks::default(),
            occurrences: BTreeSet::default(),
        }
    }

    /// Status of a process.
    fn pid_status(&self, pid: pid_t) -> PidStatus {
        if self.bookmarks.is_selected(pid) {
            PidStatus::Selected
        } else if self.occurrences.contains(&pid) {
            PidStatus::Matching
        } else if self.bookmarks.is_marked(pid) {
            PidStatus::Marked
        } else {
            PidStatus::Unknown
        }
    }
}

/// Table generator for a tree of processes.
#[derive(Getters)]
pub(crate) struct ProcessTreeTable<'a, 'b, 't> {
    /// Sample collector.
    collector: &'b Collector<'a>,
    /// Tree data.
    data: Rc<TreeData<'t>>,
    /// Headers size.
    #[getset(get = "pub")]
    headers_size: Area<usize>,
    /// Column widths
    widths: Vec<u16>,
    /// Indentation
    indents: Vec<usize>,
}

impl<'a, 'b, 't> ProcessTreeTable<'a, 'b, 't> {
    const TITLE_PROCESS: &'static str = "Process";
    const TITLE_PID: &'static str = "PID";
    const TITLE_STATE: &'static str = "S";
    const FIXED_HEADERS: [&'static str; 3] =
        [Self::TITLE_PROCESS, Self::TITLE_PID, Self::TITLE_STATE];

    pub(crate) fn new(collector: &'b Collector<'a>, data: Rc<TreeData<'t>>) -> Self {
        let mut pids = PidStack::default();
        let mut headers_height = 0;
        let mut widths = Self::FIXED_HEADERS
            .iter()
            .map(|s| MaxLength::from(*s))
            .chain(data.metric_headers.iter().map(|text| {
                if headers_height < text.lines.len() {
                    headers_height = text.lines.len();
                }
                MaxLength::from(text.iter().map(|line| line.width()).max().unwrap_or(0))
            }))
            .collect::<Vec<MaxLength>>();
        let headers_size = Area::new(Self::FIXED_HEADERS.len(), headers_height);
        let mut indents = Vec::with_capacity(collector.line_count());
        collector.lines().for_each(|ps| {
            pids.push(ps);
            let indent = pids.len().saturating_sub(1);
            indents.push(indent);
            widths[0].set_min(indent + ps.name().len());
            widths[1].set_min(ps.pid().to_string().len());
            // widths[2].set_min(1);
            ps.samples().enumerate().for_each(|(i, s)| {
                widths[i + headers_size.horizontal]
                    .set_min(s.strings().map(|s| s.len()).max().unwrap_or(0))
            });
        });
        Self {
            collector,
            headers_size,
            data,
            widths: widths.iter().map(|ml| ml.len()).collect::<Vec<u16>>(),
            indents,
        }
    }

    /// Number of columns in the body.
    pub(crate) fn body_column_count(&self) -> usize {
        self.data.metric_headers.len()
    }

    /// Number of rows in the body.
    pub(crate) fn body_row_count(&self) -> usize {
        self.collector.line_count()
    }
}

impl<'a, 'b, 't> TableGenerator for ProcessTreeTable<'a, 'b, 't> {
    fn headers_size(&self) -> Area<usize> {
        self.headers_size
    }

    fn top_headers(&self, zoom: &Zoom) -> Vec<Cell> {
        Self::FIXED_HEADERS
            .iter()
            .map(|s| Cell::from(Text::from(*s)))
            .chain(
                self.data
                    .metric_headers
                    .iter()
                    .skip(zoom.position)
                    .take(zoom.visible_length)
                    .map(|text| Cell::from(text.clone().alignment(Alignment::Center))),
            )
            .collect::<Vec<Cell>>()
    }

    fn rows(&self, zoom: &DoubleZoom) -> Vec<Vec<Cell>> {
        self.collector
            .lines()
            .skip(zoom.vertical.position)
            .take(zoom.vertical.visible_length)
            .enumerate()
            .map(|(n, ps)| {
                let pid_status = self.data.pid_status(ps.pid());
                let name = {
                    let name = ps.name();
                    format!("{:>width$}", name, width = self.indents[n] + name.len())
                };
                let name_style = self.data.styles.name_style(pid_status);
                vec![
                    Cell::from(name).style(name_style),
                    rcell!(ps.pid().to_string()),
                    rcell!(ps.state().to_string()),
                ]
                .drain(..)
                .chain(
                    ps.samples()
                        .flat_map(|sample| {
                            izip!(sample.strings(), sample.trends()).map(|(value, trend)| {
                                Cell::from(
                                    Text::from(value.as_str())
                                        .style(self.data.styles.trend_style(trend))
                                        .alignment(Alignment::Right),
                                )
                            })
                        })
                        .skip(zoom.horizontal.position)
                        .take(zoom.horizontal.visible_length),
                )
                .collect::<Vec<Cell>>()
            })
            .collect::<Vec<Vec<Cell>>>()
    }

    fn widths(&self) -> &[u16] {
        &self.widths
    }
}
