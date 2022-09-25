// Oprs -- process monitor for Linux
// Copyright (C) 2020, 2021  Laurent Pelecq
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
    display::{DisplayDevice, PauseStatus, TerminalDevice, TextDevice},
    export::{CsvExporter, Exporter, RrdExporter},
    info::SystemConf,
    metrics::{FormattedMetric, MetricDataType, MetricId, MetricNamesParser},
    sighdr::SignalHandler,
    targets::{TargetContainer, TargetId},
};

/// Delay in seconds between two notifications for time drift
const DRIFT_NOTIFICATION_DELAY: u64 = 300;

#[derive(thiserror::Error, Debug)]
pub enum Error {
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
    _theme: Option<BuiltinTheme>,
}

/// Get export type

impl Application {
    pub fn new(settings: &Settings, metric_names: &[String]) -> anyhow::Result<Application> {
        let every = Duration::from_millis((settings.display.every * 1000.0) as u64);
        let mut metrics_parser =
            MetricNamesParser::new(matches!(settings.display.format, MetricFormat::Human));
        let display_mode = resolve_display_mode(settings.display.mode)?;
        let exporter: Option<Box<dyn Exporter>> = match settings.export.kind {
            ExportType::Csv => Some(Box::new(CsvExporter::new(&settings.export)?)),
            ExportType::Rrd => Some(Box::new(RrdExporter::new(&settings.export, every)?)),
            ExportType::None => None,
        };

        Ok(Application {
            display_mode,
            every,
            count: settings.display.count,
            metrics: metrics_parser.parse(metric_names)?,
            exporter,
            _theme: settings.display.theme,
        })
    }

    pub fn run<'a>(
        &mut self,
        target_ids: &[TargetId],
        system_conf: &'a SystemConf,
    ) -> anyhow::Result<()> {
        info!("starting");

        let mut device: Option<Box<dyn DisplayDevice>> = match self.display_mode {
            DisplayMode::Any => panic!("internal error: must use check_display_mode first"),
            DisplayMode::Terminal => Some(Box::new(TerminalDevice::new(self.every)?)),
            DisplayMode::Text => Some(Box::new(TextDevice::new())),
            DisplayMode::None => None,
        };

        let mut targets = TargetContainer::new(system_conf);
        targets.push_all(target_ids)?;
        if !targets.has_system()
            && self
                .metrics
                .iter()
                .any(|metric| metric.aggregations.has(Aggregation::Ratio))
        {
            targets.push(&TargetId::System)?; // ratio requires system
        }
        let mut collector = Collector::new(target_ids.len(), &self.metrics);

        if let Some(ref mut device) = device {
            device.open(&collector)?;
        }
        if let Some(ref mut exporter) = self.exporter {
            exporter.open(&collector)?;
        }

        let sighdr = SignalHandler::new()?;
        let mut loop_number: u64 = 0;
        let mut timer = Timer::new(self.every, true);
        let mut drift = DriftMonitor::new(timer.start_time(), DRIFT_NOTIFICATION_DELAY);
        let is_interactive = match device {
            Some(ref device) => device.is_interactive(),
            _ => false,
        };

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
            if let Some(ref mut device) = device {
                device.render(&collector, targets_updated)?;
            }
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
                if let PauseStatus::Quit = device.as_mut().unwrap().pause(&mut timer)? {
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

        if let Some(ref mut device) = device {
            device.close()?;
        }
        if let Some(ref mut exporter) = self.exporter {
            exporter.close()?;
        }
        info!("stopping");
        Ok(())
    }
}
