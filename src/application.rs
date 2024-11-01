// Oprs -- process monitor for Linux
// Copyright (C) 2020-2024  Laurent Pelecq
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

use log::info;
use std::{
    io::Write,
    time::{Duration, SystemTime},
};
use strum::{EnumMessage, IntoEnumIterator};

use crate::{
    agg::Aggregation,
    cfg::{DisplayMode, ExportSettings, ExportType, MetricFormat, Settings},
    clock::{DriftMonitor, Timer},
    collector::Collector,
    console::BuiltinTheme,
    display::{DisplayDevice, NullDevice, PauseStatus, TerminalDevice, TextDevice},
    export::{CsvExporter, Exporter, RrdExporter},
    metrics::{FormattedMetric, MetricDataType, MetricId, MetricNamesParser},
    process::SystemConf,
    sighdr::SignalHandler,
    targets::{TargetContainer, TargetError, TargetId},
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
fn resolve_display_mode(mode: DisplayMode) -> Result<DisplayMode, Error> {
    match mode {
        DisplayMode::Any => {
            if TerminalDevice::is_available() {
                Ok(DisplayMode::Terminal)
            } else {
                Ok(DisplayMode::Text)
            }
        }
        DisplayMode::Terminal => {
            if TerminalDevice::is_available() {
                Ok(DisplayMode::Terminal)
            } else {
                Err(Error::TerminalNotAvailable)
            }
        }
        _ => Ok(mode),
    }
}

trait ProcessManager {
    fn initialize(&mut self, collector: &mut Collector);
    fn refresh(&mut self, collector: &mut Collector) -> bool;
}

struct FlatProcessManager<'s> {
    targets: TargetContainer<'s>,
}

impl<'s> FlatProcessManager<'s> {
    fn new(
        system_conf: &'s SystemConf,
        metrics: &[FormattedMetric],
        target_ids: &[TargetId],
    ) -> Result<Self, TargetError> {
        let with_system = metrics
            .iter()
            .any(|metric| metric.aggregations.has(Aggregation::Ratio));

        let mut targets = TargetContainer::new(system_conf, with_system);
        targets.push_all(target_ids)?;
        Ok(Self { targets })
    }
}

impl<'s> ProcessManager for FlatProcessManager<'s> {
    fn initialize(&mut self, collector: &mut Collector) {
        self.targets.initialize(collector);
    }

    fn refresh(&mut self, collector: &mut Collector) -> bool {
        let targets_updated = self.targets.refresh();
        self.targets.collect(collector);
        targets_updated
    }
}

/// Application displaying the process metrics
pub struct Application<'s> {
    display_mode: DisplayMode,
    every: Duration,
    count: Option<u64>,
    metrics: Vec<FormattedMetric>,
    export_settings: &'s ExportSettings,
    theme: Option<BuiltinTheme>,
}

/// Get export type

impl<'s> Application<'s> {
    pub fn new(settings: &'s Settings, metric_names: &[String]) -> anyhow::Result<Application<'s>> {
        let every = Duration::from_millis((settings.display.every * 1000.0) as u64);
        let mut metrics_parser =
            MetricNamesParser::new(matches!(settings.display.format, MetricFormat::Human));
        let display_mode = resolve_display_mode(settings.display.mode)?;

        Ok(Application {
            display_mode,
            every,
            count: settings.display.count,
            metrics: metrics_parser.parse(metric_names)?,
            export_settings: &settings.export,
            theme: settings.display.theme,
        })
    }

    pub fn run(&self, target_ids: &[TargetId], system_conf: &'_ SystemConf) -> anyhow::Result<()> {
        info!("starting");

        if target_ids.is_empty() {
            panic!("not implemented");
            // match self.display_mode {
            //     DisplayMode::Terminal => self.run_ui(targets)?,
            //     _ => return Err(anyhow::anyhow!(Error::NoTargets)),
            // }
        } else {
            let mut is_interactive = false;
            let device: Box<dyn DisplayDevice> = match self.display_mode {
                DisplayMode::Terminal => {
                    is_interactive = true;
                    Box::new(TerminalDevice::new(self.every, self.theme)?)
                }
                DisplayMode::Text => Box::new(TextDevice::new()),
                _ => Box::new(NullDevice::new()),
            };
            let mut tmgt = FlatProcessManager::new(&system_conf, &self.metrics, target_ids)?;
            self.run_loop(&mut tmgt, device, is_interactive)?;
        }
        Ok(())
    }

    fn run_loop(
        &self,
        tmgt: &mut dyn ProcessManager,
        mut device: Box<dyn DisplayDevice>,
        is_interactive: bool,
    ) -> anyhow::Result<()> {
        let mut collector = Collector::new(&self.metrics);

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

        tmgt.initialize(&mut collector);

        while !sighdr.caught() {
            let targets_updated = if timer.expired() {
                let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
                let targets_updated = tmgt.refresh(&mut collector);
                if let Some(ref mut exporter) = exporter {
                    exporter.export(&collector, &timestamp)?;
                }
                timer.reset();
                targets_updated
            } else {
                false
            };
            device.render(&collector, targets_updated)?;

            if let Some(count) = self.count {
                loop_number += 1;
                if loop_number >= count {
                    break;
                }
            }
            if is_interactive {
                if let PauseStatus::Quit = device.pause(&mut timer)? {
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
