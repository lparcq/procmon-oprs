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
use procfs::process::{Limit, LimitValue, Limits};
use ratatui::{
    layout::Alignment,
    style::{Color, Modifier, Style, Stylize},
    text::Text,
    widgets::Cell,
};
use std::{
    borrow::Cow,
    cell::RefCell,
    cmp::Ordering,
    collections::{BTreeSet, HashMap},
    ffi::OsString,
    rc::Rc,
};

use super::{
    input::Bookmarks,
    panes::{TableClip, TableGenerator},
    types::{Area, MaxLength},
};

use crate::{
    console::BuiltinTheme,
    process::{
        format::{human_format, Unit},
        Collector, ProcessIdentity, ProcessSamples,
    },
};

/// Aligned cell.
macro_rules! aligned_cell {
    ($s:expr, $align:expr) => {
        Cell::from(Text::from($s).alignment($align))
    };
}

/// Left aligned cell.
macro_rules! lcell {
    ($s:expr) => {
        aligned_cell!($s, Alignment::Left)
    };
}

/// Right aligned cell.
macro_rules! rcell {
    ($s:expr) => {
        aligned_cell!($s, Alignment::Right)
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

impl TreeData<'_> {
    pub(crate) fn new(styles: Styles) -> Self {
        Self {
            metric_headers: Vec::new(),
            styles,
            bookmarks: Bookmarks::default(),
            occurrences: BTreeSet::default(),
        }
    }

    /// Incremental search pattern.
    pub(crate) fn incremental_search_pattern(&self) -> Option<String> {
        if self.bookmarks.is_incremental_search() {
            Some(self.bookmarks.search_pattern().unwrap())
        } else {
            None
        }
    }

    /// Status of a process.
    fn pid_status(&self, pid: pid_t) -> PidStatus {
        if self.occurrences.contains(&pid) {
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
    /// Selected PID that must move to a next or previous page.
    #[getset(get = "pub")]
    selected_pid: RefCell<Option<pid_t>>,
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
            selected_pid: RefCell::new(None),
        }
    }
}

impl TableGenerator for ProcessTreeTable<'_, '_, '_> {
    fn headers_size(&self) -> Area<usize> {
        self.headers_size
    }

    fn top_headers(&self) -> Vec<Cell> {
        Self::FIXED_HEADERS
            .iter()
            .map(|s| lcell!(*s))
            .chain(
                self.data
                    .metric_headers
                    .iter()
                    .map(|text| Cell::from(text.clone().alignment(Alignment::Center))),
            )
            .collect::<Vec<Cell>>()
    }

    fn rows(&self, clip: &TableClip<'_, '_>) -> Vec<Vec<Cell>> {
        let offset = clip.zoom().vertical.position;
        let height = clip.zoom().vertical.visible_length;
        self.collector
            .lines()
            .skip(offset)
            .take(height)
            .enumerate()
            .map(|(n, ps)| {
                let pid = ps.pid();
                let pid_status = match clip.selected_lineno() {
                    Some(lineno) if lineno == offset + n => {
                        *self.selected_pid.borrow_mut() = Some(pid);
                        PidStatus::Selected
                    }
                    _ => self.data.pid_status(pid),
                };
                let name = {
                    let name = ps.name();
                    format!("{:>width$}", name, width = self.indents[n] + name.len())
                };
                let name_style = self.data.styles.name_style(pid_status);
                let mut i = 0;
                vec![
                    Cell::from(name).style(name_style),
                    rcell!(pid.to_string()),
                    rcell!(ps.state().to_string()),
                ]
                .drain(..)
                .chain(ps.samples().flat_map(|sample| {
                    izip!(sample.strings(), sample.trends()).filter_map(move |(value, trend)| {
                        let colnum = i;
                        i += 1;
                        clip.clip_cell(colnum, Cow::Borrowed(value.as_str()), Alignment::Right)
                            .map(|t| Cell::from(t.style(self.data.styles.trend_style(trend))))
                    })
                }))
                .collect::<Vec<Cell>>()
            })
            .collect::<Vec<Vec<Cell>>>()
    }

    fn body_row_count(&self) -> usize {
        self.collector.line_count()
    }

    fn widths(&self) -> &[u16] {
        &self.widths
    }
}

/// A soft and hard limit with a name.
#[derive(Debug)]
struct NamedLimit {
    name: &'static str,
    soft: String,
    hard: String,
}

impl NamedLimit {
    fn new(name: &'static str, limit: Limit, unit: Unit) -> Self {
        let soft = NamedLimit::format_limit(limit.soft_limit, unit);
        let hard = NamedLimit::format_limit(limit.hard_limit, unit);
        Self { name, soft, hard }
    }

    fn format_limit(limit: LimitValue, unit: Unit) -> String {
        const INFINITY: &str = "∞";
        match limit {
            LimitValue::Unlimited => INFINITY.to_string(),
            LimitValue::Value(value) => human_format(value, unit),
        }
    }
}

/// Table generator for process limits.
pub(crate) struct LimitsTable {
    headers: Vec<&'static str>,
    limits: Vec<NamedLimit>,
    widths: Vec<u16>,
}

impl LimitsTable {
    pub(crate) fn new(limits: Limits) -> Self {
        let headers = vec!["Limit", "Soft", "Hard"];
        let limits = vec![
            NamedLimit::new("CPU Time", limits.max_cpu_time, Unit::Seconds),
            NamedLimit::new("File Size", limits.max_file_size, Unit::Size),
            NamedLimit::new("Data Size", limits.max_data_size, Unit::Size),
            NamedLimit::new("Stack Size", limits.max_stack_size, Unit::Size),
            NamedLimit::new("Core File Size", limits.max_core_file_size, Unit::Size),
            NamedLimit::new("Resident Set", limits.max_resident_set, Unit::Size),
            NamedLimit::new("Processes", limits.max_processes, Unit::Number),
            NamedLimit::new("Open Files", limits.max_open_files, Unit::Number),
            NamedLimit::new("Locked Memory", limits.max_locked_memory, Unit::Size),
            NamedLimit::new("Address Space", limits.max_address_space, Unit::Size),
            NamedLimit::new("File Locks", limits.max_file_locks, Unit::Number),
            NamedLimit::new("Pending Signals", limits.max_pending_signals, Unit::Number),
            NamedLimit::new("Msgqueue Size", limits.max_msgqueue_size, Unit::Size),
            NamedLimit::new("Nice Priority", limits.max_nice_priority, Unit::Number),
            NamedLimit::new(
                "Realtime Priority",
                limits.max_realtime_priority,
                Unit::Number,
            ),
            NamedLimit::new(
                "Realtime Timeout",
                limits.max_realtime_timeout,
                Unit::Number,
            ),
        ];
        let limit_width = MaxLength::with_lines(
            limits
                .iter()
                .map(|l| l.soft.as_str())
                .chain(limits.iter().map(|l| l.hard.as_str())),
        )
        .len();
        let widths = vec![
            MaxLength::with_lines(limits.iter().map(|l| l.name)).len(),
            limit_width,
            limit_width,
        ];
        Self {
            headers,
            limits,
            widths,
        }
    }
}

impl TableGenerator for LimitsTable {
    fn headers_size(&self) -> Area<usize> {
        Area::new(1, 1)
    }

    fn top_headers(&self) -> Vec<Cell> {
        let bold = Style::default().bold();
        self.headers
            .iter()
            .enumerate()
            .map(|(i, s)| {
                Cell::from(
                    Text::styled(*s, bold)
                        .alignment(if i == 0 {
                            Alignment::Left
                        } else {
                            Alignment::Right
                        })
                        .bold(),
                )
            })
            .collect::<Vec<Cell>>()
    }

    fn rows(&self, clip: &TableClip<'_, '_>) -> Vec<Vec<Cell>> {
        let vzoom = &clip.zoom().vertical;
        self.limits
            .iter()
            .skip(vzoom.position)
            .take(vzoom.visible_length)
            .map(|limit| {
                vec![
                    Some(Text::from(limit.name).alignment(Alignment::Left)),
                    clip.clip_cell(0, Cow::Borrowed(limit.soft.as_str()), Alignment::Right),
                    clip.clip_cell(1, Cow::Borrowed(limit.hard.as_str()), Alignment::Right),
                ]
                .drain(..)
                .filter_map(|t| t.map(Cell::from))
                .collect::<Vec<Cell>>()
            })
            .collect::<Vec<Vec<Cell>>>()
    }

    fn body_row_count(&self) -> usize {
        self.limits.len()
    }

    fn widths(&self) -> &[u16] {
        &self.widths
    }
}

/// Table generator for process environment.
pub(crate) struct EnvironmentTable {
    env: Vec<(String, String)>,
    widths: Vec<u16>,
}

impl EnvironmentTable {
    pub(crate) fn new(mut env: HashMap<OsString, OsString>) -> Self {
        let mut env = env
            .drain()
            .map(|(k, v)| (Self::into_string(k), Self::into_string(v)))
            .collect::<Vec<(String, String)>>();
        env.sort_by(|(k1, _), (k2, _)| k1.cmp(k2));
        let widths = vec![
            MaxLength::with_lines(env.iter().map(|(k, _)| k.as_str())).len(),
            MaxLength::with_lines(env.iter().map(|(_, v)| v.as_str())).len(),
        ];
        Self { env, widths }
    }

    fn into_string(os: OsString) -> String {
        os.into_string().unwrap_or_else(|os| format!("{os:?}"))
    }
}

impl TableGenerator for EnvironmentTable {
    fn headers_size(&self) -> Area<usize> {
        Area::new(1, 1)
    }

    fn top_headers(&self) -> Vec<Cell> {
        let bold = Style::default().bold();
        ["Variable", "Value"]
            .iter()
            .map(|s| Cell::from(Text::from(*s).style(bold)))
            .collect::<Vec<_>>()
    }

    fn rows(&self, clip: &TableClip<'_, '_>) -> Vec<Vec<Cell>> {
        let vzoom = &clip.zoom().vertical;
        self.env
            .iter()
            .skip(vzoom.position)
            .take(vzoom.visible_length)
            .map(|(k, v)| {
                vec![
                    Some(Text::from(k.to_string()).alignment(Alignment::Left)),
                    clip.clip_cell(0, Cow::Owned(v.to_string()), Alignment::Left),
                ]
                .drain(..)
                .filter_map(|t| t.map(Cell::from))
                .collect::<Vec<Cell>>()
            })
            .collect::<Vec<Vec<Cell>>>()
    }

    fn body_row_count(&self) -> usize {
        self.env.iter().count()
    }

    fn widths(&self) -> &[u16] {
        &self.widths
    }
}
