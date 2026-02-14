// Oprs -- details monitor for Linux
// Copyright (C) 2020-2025  Laurent Pelecq
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

use libc::pid_t;
use log::info;
use std::{
    borrow::Cow,
    io::Write,
    time::{Duration, SystemTime},
};
use strum::{EnumMessage, IntoEnumIterator};

use crate::{
    cfg::{DisplayMode, ExportSettings, ExportType, Settings},
    clock::{DriftMonitor, Timer},
    display::{DisplayDevice, NullDevice, PaneData, PaneKind, TextDevice},
    export::{CsvExporter, Exporter, RrdExporter},
    process::{
        Collector, FlatProcessManager, ForestProcessManager, FormattedMetric, MetricDataType,
        MetricId, MetricNamesParser, ProcessManager, ProcessResult, TargetId,
    },
    sighdr::SignalHandler,
};

#[cfg(feature = "tui")]
use crate::{
    console::theme::BuiltinTheme,
    display::{DataKind, Interaction, PauseStatus, TerminalDevice},
    process::{MetricFormat, ProcessDetails},
};

/// Delay in seconds between two notifications for time drift
const DRIFT_NOTIFICATION_DELAY: u64 = 300;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("no target specified in non-terminal mode")]
    NoTargets,
    #[cfg(feature = "tui")]
    #[error("terminal not available")]
    TerminalNotAvailable,
}

#[cfg(feature = "tui")]
pub type ApplicationResult<T> = Result<T, Error>;

/// List available metrics
pub fn list_metrics() {
    for metric_id in MetricId::iter() {
        println!(
            "{:<18}\t{:<9}\t{}",
            metric_id.as_str(),
            match metric_id.data_type() {
                MetricDataType::Counter => "[counter]",
                MetricDataType::Gauge => "[gauge]",
            },
            metric_id.get_message().unwrap_or("not documented")
        );
    }
}

/// Return the best available display
#[cfg(feature = "tui")]
fn resolve_display_mode(
    mode: DisplayMode,
    theme: Option<BuiltinTheme>,
) -> ApplicationResult<(DisplayMode, Option<BuiltinTheme>)> {
    match mode {
        DisplayMode::None | DisplayMode::Text => Ok((mode, None)),
        _ => {
            if TerminalDevice::is_available() {
                Ok((DisplayMode::Terminal, theme.or_else(BuiltinTheme::guess)))
            } else {
                match mode {
                    DisplayMode::Terminal => Err(Error::TerminalNotAvailable),
                    DisplayMode::Any => Ok((DisplayMode::Text, None)),
                    _ => panic!("already handled in outter match."),
                }
            }
        }
    }
}

/// Return the best available display
#[cfg(not(feature = "tui"))]
fn resolve_display_mode(mode: DisplayMode) -> DisplayMode {
    match mode {
        DisplayMode::Any => DisplayMode::Text,
        _ => mode,
    }
}

/// State of the application
struct State<'a> {
    collector: Collector<'a>,
    manager: Box<dyn ProcessManager>,
    #[cfg(feature = "tui")]
    format: MetricFormat,
    pane_kind: PaneKind,
    #[cfg(feature = "tui")]
    root_pid: Option<pid_t>,
    #[cfg(feature = "tui")]
    details: Option<ProcessDetails<'a>>,
}

impl<'a> State<'a> {
    fn new(
        metrics: &'a [FormattedMetric],
        #[cfg(feature = "tui")] format: MetricFormat,
        target_ids: &[TargetId],
        root_pid: Option<pid_t>,
    ) -> anyhow::Result<Self> {
        let mut manager: Box<dyn ProcessManager> = if target_ids.is_empty() {
            Box::new(ForestProcessManager::new()?)
        } else {
            Box::new(FlatProcessManager::new(metrics, target_ids)?)
        };
        if let Some(ctx) = manager.context() {
            ctx.set_root_pid(root_pid);
        }
        Ok(Self {
            collector: Collector::new(Cow::Borrowed(metrics)),
            manager,
            #[cfg(feature = "tui")]
            format,
            pane_kind: PaneKind::Main,
            #[cfg(feature = "tui")]
            root_pid,
            #[cfg(feature = "tui")]
            details: None,
        })
    }

    fn refresh_metrics(&mut self) -> ProcessResult<bool> {
        self.manager.refresh(&mut self.collector)
    }

    fn pane_data<'p>(&self) -> PaneData<'_, 'p> {
        match self.pane_kind {
            PaneKind::Main => PaneData::Collector(&self.collector),
            #[cfg(feature = "tui")]
            PaneKind::Process(DataKind::Details) => {
                PaneData::Details(self.details.as_ref().unwrap())
            }
            #[cfg(feature = "tui")]
            PaneKind::Process(_) => {
                PaneData::Process(self.details.as_ref().unwrap().process().process())
            }
            #[cfg(feature = "tui")]
            PaneKind::Help => PaneData::None,
        }
    }

    /// Get process details.
    #[cfg(feature = "tui")]
    fn get_details(pid: pid_t, format: MetricFormat) -> Option<ProcessDetails<'a>> {
        match ProcessDetails::new(pid, format) {
            Ok(mut details) => details.refresh().ok().map(|_| details),
            Err(_) => {
                log::error!("{pid}: details cannot be selected");
                None
            }
        }
    }

    /// Get parent process details.
    #[cfg(feature = "tui")]
    fn get_parent_details(details: Option<ProcessDetails<'a>>) -> Option<ProcessDetails<'a>> {
        details.and_then(|details| match details.parent() {
            Ok(mut details) => details.refresh().ok().map(|_| details),
            Err(_) => {
                log::error!(
                    "{}: details of parent cannot be selected",
                    details.process().pid()
                );
                Some(details)
            }
        })
    }

    #[cfg(feature = "tui")]
    fn refresh_details(&mut self) {
        if match &mut self.details {
            Some(details) => details.refresh().is_err(),
            None => false,
        } {
            self.details = None;
            self.pane_kind = PaneKind::Main;
        }
    }

    #[cfg(feature = "tui")]
    fn interact(&mut self, action: &Interaction) -> anyhow::Result<bool> {
        match action {
            Interaction::Quit => return Ok(false),
            Interaction::Filter(filter) => {
                self.manager.context().map(|c| c.set_filter(*filter));
                self.manager.refresh(&mut self.collector)?;
            }
            Interaction::SwitchBack => match (self.pane_kind, &self.details) {
                (PaneKind::Process(DataKind::Details), Some(_)) => {
                    self.details = None;
                    self.pane_kind = PaneKind::Main;
                }
                (PaneKind::Help | PaneKind::Process(_), Some(_)) => {
                    self.pane_kind = PaneKind::Process(DataKind::Details)
                }
                (_, _) => self.pane_kind = PaneKind::Main,
            },
            Interaction::SwitchToHelp => self.pane_kind = PaneKind::Help,
            Interaction::SwitchTo(kind) => {
                if matches!(self.pane_kind, PaneKind::Process(_)) {
                    self.pane_kind = PaneKind::Process(*kind);
                }
            }
            Interaction::KillProcess(signal) => {
                if let Some(details) = &self.details {
                    details.process().kill(*signal);
                }
            }
            Interaction::SelectPid(pid) => {
                self.details = Self::get_details(*pid, self.format);
                if self.details.is_some() {
                    self.pane_kind = PaneKind::Process(DataKind::Details);
                }
            }
            Interaction::SelectParent => {
                self.details = Self::get_parent_details(self.details.take());
                if self.details.is_some() {
                    self.pane_kind = PaneKind::Process(DataKind::Details);
                }
            }
            Interaction::SelectRootPid(new_root_pid) => {
                self.root_pid = *new_root_pid;
                self.manager
                    .context()
                    .map(|c| c.set_root_pid(self.root_pid));
                self.manager.refresh(&mut self.collector)?;
            }
            Interaction::Narrow(pids) => {
                log::debug!("switch to flat mode with {} PIDs", pids.len());
                self.manager = Box::new(FlatProcessManager::with_pids(pids));
                self.manager.refresh(&mut self.collector)?;
            }
            Interaction::Wide => {
                log::debug!("switch to explorer mode");
                self.manager = Box::new(ForestProcessManager::new()?);
                self.manager
                    .context()
                    .map(|c| c.set_root_pid(self.root_pid));
                self.manager.refresh(&mut self.collector)?;
            }
            Interaction::None => (),
        }
        Ok(true)
    }
}

/// Application displaying the details metrics
pub struct Application<'s> {
    display_mode: DisplayMode,
    every: Duration,
    count: Option<u64>,
    metrics: Vec<FormattedMetric>,
    export_settings: &'s ExportSettings,
    #[cfg(feature = "tui")]
    theme: Option<BuiltinTheme>,
    #[cfg(feature = "tui")]
    format: MetricFormat,
}

impl<'s> Application<'s> {
    pub fn new<'m>(
        settings: &'s Settings,
        metric_names: &[&'m str],
    ) -> anyhow::Result<Application<'s>> {
        let every = Duration::from_millis((settings.display.every * 1000.0) as u64);
        let format = settings.display.format;
        let mut metrics_parser = MetricNamesParser::new(format);
        #[cfg(feature = "tui")]
        let (display_mode, theme) =
            resolve_display_mode(settings.display.mode, settings.display.theme)?;
        #[cfg(not(feature = "tui"))]
        let display_mode = resolve_display_mode(settings.display.mode);

        Ok(Application {
            display_mode,
            every,
            count: settings.display.count,
            metrics: metrics_parser.parse(metric_names)?,
            export_settings: &settings.export,
            #[cfg(feature = "tui")]
            theme,
            #[cfg(feature = "tui")]
            format,
        })
    }

    pub fn run(&self, target_ids: &[TargetId], root_pid: Option<pid_t>) -> anyhow::Result<()> {
        info!("starting");
        #[cfg(feature = "tui")]
        let mut is_interactive = false;
        #[cfg(not(feature = "tui"))]
        let is_interactive = false;
        let device: Box<dyn DisplayDevice> = match self.display_mode {
            #[cfg(feature = "tui")]
            DisplayMode::Terminal => {
                is_interactive = true;
                Box::new(TerminalDevice::new(self.every, self.theme)?)
            }
            DisplayMode::Text => Box::new(TextDevice::new()),
            _ => Box::new(NullDevice::new()),
        };

        if target_ids.is_empty() && !is_interactive {
            Err(anyhow::anyhow!(Error::NoTargets))
        } else {
            self.run_loop(device, target_ids, root_pid, is_interactive)
        }
    }

    fn run_loop(
        &self,
        mut device: Box<dyn DisplayDevice>,
        target_ids: &[TargetId],
        root_pid: Option<pid_t>,
        is_interactive: bool,
    ) -> anyhow::Result<()> {
        let mut state = State::new(
            &self.metrics,
            #[cfg(feature = "tui")]
            self.format,
            target_ids,
            root_pid,
        )?;

        device.open(self.metrics.iter())?;
        let mut exporter: Option<Box<dyn Exporter>> = match self.export_settings.kind {
            ExportType::Csv | ExportType::Tsv => {
                Some(Box::new(CsvExporter::new(self.export_settings)?))
            }
            ExportType::Rrd | ExportType::RrdGraph => Some(Box::new(RrdExporter::new(
                self.export_settings,
                self.every,
            )?)),
            ExportType::None => None,
        };

        if let Some(ref mut exporter) = exporter {
            exporter.open(self.metrics.iter())?;
        }

        let sighdr = SignalHandler::new()?;
        let mut loop_number: u64 = 0;
        let mut timer = Timer::new(self.every, true);
        let mut drift = DriftMonitor::new(timer.start_time(), DRIFT_NOTIFICATION_DELAY);

        while !sighdr.caught() {
            let targets_updated = if timer.expired() {
                let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
                let targets_updated = state.refresh_metrics()?;
                #[cfg(feature = "tui")]
                state.refresh_details();
                if let Some(ref mut exporter) = exporter {
                    exporter.export(&state.collector, &timestamp)?;
                }
                timer.reset();
                targets_updated
            } else {
                false
            };
            device.render(state.pane_kind, state.pane_data(), targets_updated)?;

            if let Some(count) = self.count {
                loop_number += 1;
                if loop_number >= count {
                    break;
                }
            }
            if is_interactive {
                #[cfg(feature = "tui")]
                if let PauseStatus::Action(action) = device.pause(&mut timer)?
                    && !state.interact(&action)?
                {
                    break;
                }
            } else {
                let mut remaining = Some(self.every);
                while let Some(delay) = remaining {
                    remaining = timer.sleep(delay);
                    std::io::stdout().flush()?; // hack: signal not caught otherwise
                    if sighdr.caught() {
                        info!("signal caught, exiting.");
                        break;
                    }
                }
            }
            drift.update(timer.get_delay());
        }

        device.close()?;
        if let Some(ref mut exporter) = exporter {
            exporter.close()?;
        }
        info!("stopping");
        Ok(())
    }
}
