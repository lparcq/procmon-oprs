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
use std::io::Write;
use std::result;
use std::time::{Duration, SystemTime};
use strum::{EnumMessage, IntoEnumIterator};

use crate::{
    agg::Aggregation,
    cfg::{DisplayMode, ExportType, MetricFormat, Settings},
    clock::{DriftMonitor, Timer},
    collector::Collector,
    console::BuiltinTheme,
    display::{DisplayDevice, NullDevice, PauseStatus, TerminalDevice, TextDevice},
    export::{CsvExporter, Exporter, RrdExporter},
    metrics::{FormattedMetric, MetricDataType, MetricId, MetricNamesParser},
    process::SystemConf,
    sighdr::SignalHandler,
    targets::{TargetContainer, TargetId},
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
fn resolve_display_mode(mode: DisplayMode) -> result::Result<DisplayMode, Error> {
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

/// Application displaying the process metrics
pub struct Application {
    display_mode: DisplayMode,
    every: Duration,
    count: Option<u64>,
    metrics: Vec<FormattedMetric>,
    exporter: Option<Box<dyn Exporter>>,
    theme: Option<BuiltinTheme>,
}

/// Get export type

impl Application {
    pub fn new(settings: &Settings, metric_names: &[String]) -> anyhow::Result<Application> {
        let every = Duration::from_millis((settings.display.every * 1000.0) as u64);
        let mut metrics_parser =
            MetricNamesParser::new(matches!(settings.display.format, MetricFormat::Human));
        let display_mode = resolve_display_mode(settings.display.mode)?;
        let exporter: Option<Box<dyn Exporter>> = match settings.export.kind {
            ExportType::Csv | ExportType::Tsv => {
                Some(Box::new(CsvExporter::new(&settings.export)?))
            }
            ExportType::Rrd | ExportType::RrdGraph => {
                Some(Box::new(RrdExporter::new(&settings.export, every)?))
            }
            ExportType::None => None,
        };

        Ok(Application {
            display_mode,
            every,
            count: settings.display.count,
            metrics: metrics_parser.parse(metric_names)?,
            exporter,
            theme: settings.display.theme,
        })
    }

    pub fn run(
        &mut self,
        target_ids: &[TargetId],
        system_conf: &'_ SystemConf,
    ) -> anyhow::Result<()> {
        info!("starting");

        let with_system = self
            .metrics
            .iter()
            .any(|metric| metric.aggregations.has(Aggregation::Ratio));
        let mut targets = TargetContainer::new(system_conf, with_system);
        targets.push_all(target_ids)?;

        if target_ids.is_empty() {
            match self.display_mode {
                DisplayMode::Terminal => self.run_ui(targets)?,
                _ => return Err(anyhow::anyhow!(Error::NoTargets)),
            }
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
            self.run_loop(targets, device, is_interactive)?;
        }
        Ok(())
    }

    fn run_loop(
        &mut self,
        mut targets: TargetContainer,
        mut device: Box<dyn DisplayDevice>,
        is_interactive: bool,
    ) -> anyhow::Result<()> {
        let mut collector = Collector::new(targets.len(), &self.metrics);

        device.open(&collector)?;
        if let Some(ref mut exporter) = self.exporter {
            exporter.open(&collector)?;
        }

        let sighdr = SignalHandler::new()?;
        let mut loop_number: u64 = 0;
        let mut timer = Timer::new(self.every, true);
        let mut drift = DriftMonitor::new(timer.start_time(), DRIFT_NOTIFICATION_DELAY);

        targets.initialize(&collector);

        while !sighdr.caught() {
            let targets_updated = targets.refresh();
            let collect_timestamp = if timer.expired() {
                timer.reset();
                let timestamp = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
                targets.collect(&mut collector);
                Some(timestamp)
            } else {
                None
            };
            device.render(&collector, targets_updated)?;
            if let Some(timestamp) = collect_timestamp {
                if let Some(ref mut exporter) = self.exporter {
                    exporter.export(&collector, &timestamp)?;
                }
            }

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
        if let Some(ref mut exporter) = self.exporter {
            exporter.close()?;
        }
        info!("stopping");
        Ok(())
    }

    fn run_ui(&self, mut _targets: TargetContainer) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("run_ui is not implemented"))
    }
}
