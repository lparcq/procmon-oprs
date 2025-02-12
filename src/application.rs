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
    cfg::{DisplayMode, ExportSettings, ExportType, MetricFormat, Settings},
    clock::{DriftMonitor, Timer},
    console::BuiltinTheme,
    display::{
        DataKind, DisplayDevice, Interaction, NullDevice, PaneData, PaneKind, PauseStatus,
        TerminalDevice, TextDevice,
    },
    export::{CsvExporter, Exporter, RrdExporter},
    process::{
        Collector, FlatProcessManager, ForestProcessManager, FormattedMetric, MetricDataType,
        MetricId, MetricNamesParser, ProcessDetails, ProcessManager, SystemConf, TargetId,
    },
    sighdr::SignalHandler,
};

/// Delay in seconds between two notifications for time drift
const DRIFT_NOTIFICATION_DELAY: u64 = 300;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("no target specified in non-terminal mode")]
    NoTargets,
    #[error("terminal not available")]
    TerminalNotAvailable,
}

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

/// Application displaying the details metrics
pub struct Application<'s> {
    display_mode: DisplayMode,
    every: Duration,
    count: Option<u64>,
    metrics: Vec<FormattedMetric>,
    export_settings: &'s ExportSettings,
    theme: Option<BuiltinTheme>,
    human: bool,
}

impl<'s> Application<'s> {
    pub fn new<'m>(
        settings: &'s Settings,
        metric_names: &[&'m str],
    ) -> anyhow::Result<Application<'s>> {
        let every = Duration::from_millis((settings.display.every * 1000.0) as u64);
        let human = matches!(settings.display.format, MetricFormat::Human);
        let mut metrics_parser = MetricNamesParser::new(human);
        let (display_mode, theme) =
            resolve_display_mode(settings.display.mode, settings.display.theme)?;

        Ok(Application {
            display_mode,
            every,
            count: settings.display.count,
            metrics: metrics_parser.parse(metric_names)?,
            export_settings: &settings.export,
            theme,
            human,
        })
    }

    pub fn run(
        &self,
        target_ids: &[TargetId],
        sysconf: &'_ SystemConf,
        root_pid: Option<pid_t>,
    ) -> anyhow::Result<()> {
        info!("starting");
        let mut is_interactive = false;
        let device: Box<dyn DisplayDevice> = match self.display_mode {
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
            self.run_loop(device, sysconf, target_ids, root_pid, is_interactive)
        }
    }

    /// Get process details.
    fn get_details(&self, pid: pid_t, sysconf: &'_ SystemConf) -> Option<ProcessDetails> {
        match ProcessDetails::new(pid, self.human) {
            Ok(mut details) => details.refresh(sysconf).ok().map(|_| details),
            Err(_) => {
                log::error!("{pid}: details cannot be selected");
                None
            }
        }
    }

    /// Get parent process details.
    fn get_parent_details<'a>(
        details: Option<ProcessDetails<'a>>,
        sysconf: &'_ SystemConf,
    ) -> Option<ProcessDetails<'a>> {
        details.and_then(|details| match details.parent() {
            Ok(mut details) => details.refresh(sysconf).ok().map(|_| details),
            Err(_) => {
                log::error!(
                    "{}: details of parent cannot be selected",
                    details.process().pid()
                );
                Some(details)
            }
        })
    }

    fn run_loop(
        &self,
        mut device: Box<dyn DisplayDevice>,
        sysconf: &'_ SystemConf,
        target_ids: &[TargetId],
        mut root_pid: Option<pid_t>,
        is_interactive: bool,
    ) -> anyhow::Result<()> {
        let mut collector = Collector::new(Cow::Borrowed(&self.metrics));
        let mut tmgt: Box<dyn ProcessManager> = if target_ids.is_empty() {
            Box::new(ForestProcessManager::new(sysconf)?)
        } else {
            Box::new(FlatProcessManager::new(sysconf, &self.metrics, target_ids)?)
        };
        tmgt.context().map(|c| c.set_root_pid(root_pid));
        let mut details: Option<ProcessDetails> = None;
        let mut pane_kind = PaneKind::Main;

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
                let targets_updated = tmgt.refresh(&mut collector)?;
                if match &mut details {
                    Some(details) => details.refresh(sysconf).is_err(),
                    None => false,
                } {
                    details = None;
                    pane_kind = PaneKind::Main;
                }
                if let Some(ref mut exporter) = exporter {
                    exporter.export(&collector, &timestamp)?;
                }
                timer.reset();
                targets_updated
            } else {
                false
            };
            device.render(
                pane_kind,
                match pane_kind {
                    PaneKind::Main => PaneData::Collector(&collector),
                    PaneKind::Process(DataKind::Details) => {
                        PaneData::Details(details.as_ref().unwrap())
                    }
                    PaneKind::Process(_) => {
                        PaneData::Process(details.as_ref().unwrap().process().process())
                    }
                    PaneKind::Help => PaneData::None,
                },
                targets_updated,
            )?;

            if let Some(count) = self.count {
                loop_number += 1;
                if loop_number >= count {
                    break;
                }
            }
            if is_interactive {
                if let PauseStatus::Action(action) = device.pause(&mut timer)? {
                    match action {
                        Interaction::Quit => break,
                        Interaction::Filter(filter) => {
                            tmgt.context().map(|c| c.set_filter(filter));
                            tmgt.refresh(&mut collector)?;
                        }
                        Interaction::SwitchBack => match (pane_kind, &details) {
                            (PaneKind::Process(DataKind::Details), Some(_)) => {
                                details = None;
                                pane_kind = PaneKind::Main;
                            }
                            (PaneKind::Help | PaneKind::Process(_), Some(_)) => {
                                pane_kind = PaneKind::Process(DataKind::Details)
                            }
                            (_, _) => pane_kind = PaneKind::Main,
                        },
                        Interaction::SwitchToHelp => pane_kind = PaneKind::Help,
                        Interaction::SwitchTo(kind) => {
                            if matches!(pane_kind, PaneKind::Process(_)) {
                                pane_kind = PaneKind::Process(kind);
                            }
                        }
                        Interaction::SelectPid(pid) => {
                            details = self.get_details(pid, sysconf);
                            if details.is_some() {
                                pane_kind = PaneKind::Process(DataKind::Details);
                            }
                        }
                        Interaction::SelectParent => {
                            details = Application::get_parent_details(details, sysconf);
                            if details.is_some() {
                                pane_kind = PaneKind::Process(DataKind::Details);
                            }
                        }
                        Interaction::SelectRootPid(new_root_pid) => {
                            root_pid = new_root_pid;
                            tmgt.context().map(|c| c.set_root_pid(root_pid));
                            tmgt.refresh(&mut collector)?;
                        }
                        Interaction::Narrow(pids) => {
                            log::debug!("switch to flat mode with {} PIDs", pids.len());
                            tmgt = Box::new(FlatProcessManager::with_pids(sysconf, &pids));
                            tmgt.refresh(&mut collector)?;
                        }
                        Interaction::Wide => {
                            log::debug!("switch to explorer mode");
                            tmgt = Box::new(ForestProcessManager::new(sysconf)?);
                            tmgt.context().map(|c| c.set_root_pid(root_pid));
                            tmgt.refresh(&mut collector)?;
                        }
                        Interaction::None => (),
                    }
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
