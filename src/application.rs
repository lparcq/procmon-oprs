// Oprs -- process monitor for Linux
// Copyright (C) 2020  Laurent Pelecq
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
use std::time::{Duration, SystemTime};
use strum::{EnumMessage, IntoEnumIterator};

use crate::{
    agg::Aggregation,
    cfg::{DisplayMode, ExportSettings, ExportType, MetricFormat, Settings},
    clock::{DriftMonitor, Timer},
    collector::Collector,
    console::{BuiltinTheme, Screen},
    display::{DisplayDevice, PauseStatus, TerminalDevice, TextDevice},
    export::{CsvExporter, Exporter, RrdExporter},
    info::SystemConf,
    metrics::{FormattedMetric, MetricId, MetricNamesParser},
    sighdr::SignalHandler,
    targets::{TargetContainer, TargetId},
};

/// Delay in seconds between two notifications for time drift
const DRIFT_NOTIFICATION_DELAY: u64 = 300;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("terminal not available")]
    TerminalNotAvailable,
    #[error("missing export size")]
    MissingExportSize,
}

pub fn list_metrics() {
    for metric_id in MetricId::iter() {
        println!(
            "{:<18}\t{}",
            metric_id.to_str(),
            metric_id.get_message().unwrap_or("not documented")
        );
    }
}

/// Application displaying the process metrics
pub struct Application<'s> {
    display_mode: DisplayMode,
    every: Duration,
    count: Option<u64>,
    metrics: Vec<FormattedMetric>,
    export_params: &'s ExportSettings,
    theme: Option<BuiltinTheme>,
}

/// Get export type

impl<'s> Application<'s> {
    pub fn new(settings: &'s Settings, metric_names: &[String]) -> anyhow::Result<Application<'s>> {
        let every = Duration::from_millis((settings.display.every * 1000.0) as u64);
        let mut metrics_parser = MetricNamesParser::new(match settings.display.format {
            MetricFormat::Human => true,
            _ => false,
        });
        let display_mode = match settings.display.mode {
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
            display_mode => Ok(display_mode),
        }?;

        Ok(Application {
            display_mode,
            every,
            count: settings.display.count,
            metrics: metrics_parser.parse(metric_names)?,
            export_params: &settings.export,
            theme: settings.display.theme,
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
            DisplayMode::Terminal => {
                let mut screen = Screen::new()?;
                if let Some(theme) = &self.theme {
                    screen.set_theme(*theme);
                }
                Some(Box::new(TerminalDevice::new(self.every, screen)?))
            }
            DisplayMode::Text => Some(Box::new(TextDevice::new())),
            DisplayMode::None => None,
        };
        let mut exporter: Option<Box<dyn Exporter>> = match self.export_params.kind {
            ExportType::Csv => Some(Box::new(CsvExporter::new(self.export_params.dir.as_path()))),
            ExportType::Rrd => Some(Box::new(RrdExporter::new(
                self.export_params.dir.as_path(),
                self.every,
                self.export_params.size.ok_or(Error::MissingExportSize)?,
            )?)),
            ExportType::None => None,
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
        if let Some(ref mut exporter) = exporter {
            exporter.open(&collector)?;
        }

        let sighdr = SignalHandler::new()?;
        let mut loop_number: u64 = 0;
        let mut timer = Timer::new(self.every, true);
        let mut drift = DriftMonitor::new(timer.start_time(), DRIFT_NOTIFICATION_DELAY);
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
                if let Some(ref mut exporter) = exporter {
                    exporter.export(&collector, &timestamp)?;
                }
            }

            if let Some(count) = self.count {
                loop_number += 1;
                if loop_number >= count {
                    break;
                }
            }
            if let Some(ref mut device) = device {
                if let PauseStatus::Quit = device.pause(&mut timer)? {
                    break;
                }
            } else {
                timer.sleep();
            }
            drift.update(timer.get_delay());
        }

        if let Some(ref mut device) = device {
            device.close()?;
        }
        if let Some(ref mut exporter) = exporter {
            exporter.close()?;
        }
        info!("stopping");
        Ok(())
    }
}
