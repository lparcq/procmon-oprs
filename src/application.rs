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

use clap::arg_enum;
use log::info;
use std::thread;
use std::time::Duration;
use strum::{EnumMessage, IntoEnumIterator};
use thiserror::Error;

use crate::{
    agg::Aggregation,
    cfg,
    collector::Collector,
    console::{BuiltinTheme, Screen},
    display::{DisplayDevice, PauseStatus, TerminalDevice, TextDevice},
    info::SystemConf,
    metrics::{FormattedMetric, MetricId, MetricNamesParser},
    targets::{TargetContainer, TargetId},
};

arg_enum! {
    #[derive(Debug)]
    pub enum DisplayMode {
        None,
        Any,
        Text,
        Term,
    }
}

#[derive(Error, Debug)]
pub enum Error {
    #[error("{0}: invalid parameter value")]
    InvalidParameter(&'static str),
    #[error("terminal not available")]
    TerminalNotAvailable,
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
pub struct Application {
    every: Duration,
    count: Option<u64>,
    metrics: Vec<FormattedMetric>,
    theme: Option<BuiltinTheme>,
}

impl Application {
    pub fn new(settings: &config::Config, metric_names: &[String]) -> anyhow::Result<Application> {
        let every = Duration::from_millis(
            (settings
                .get_float(cfg::KEY_EVERY)
                .map_err(|_| Error::InvalidParameter(cfg::KEY_EVERY))?
                * 1000.0) as u64,
        );
        let count = settings.get_int(cfg::KEY_COUNT).map(|c| c as u64).ok();
        let human_format = settings.get_bool(cfg::KEY_HUMAN_FORMAT).unwrap_or(false);
        let mut metrics_parser = MetricNamesParser::new(human_format);
        let theme = settings.get_str(cfg::KEY_COLOR_THEME)?;
        Ok(Application {
            every,
            count,
            metrics: metrics_parser.parse(metric_names)?,
            theme: match theme.as_str() {
                "dark" => Some(BuiltinTheme::Dark),
                "light" => Some(BuiltinTheme::Light),
                _ => None,
            },
        })
    }

    /// Return the best display mode for any and check if mode is available otherwise.
    pub fn check_display_mode(display_mode: DisplayMode) -> Result<DisplayMode, Error> {
        match display_mode {
            DisplayMode::Any => {
                if TerminalDevice::is_available() {
                    Ok(DisplayMode::Term)
                } else {
                    Ok(DisplayMode::Text)
                }
            }
            DisplayMode::Term => {
                if TerminalDevice::is_available() {
                    Ok(DisplayMode::Term)
                } else {
                    Err(Error::TerminalNotAvailable)
                }
            }
            _ => Ok(display_mode),
        }
    }

    pub fn run<'a>(
        &mut self,
        display_mode: DisplayMode,
        target_ids: &[TargetId],
        system_conf: &'a SystemConf,
    ) -> anyhow::Result<()> {
        info!("starting");
        let mut device: Option<Box<dyn DisplayDevice>> = match display_mode {
            DisplayMode::Any => panic!("internal error: must use check_display_mode first"),
            DisplayMode::Term => {
                let mut screen = Screen::new()?;
                if let Some(theme) = &self.theme {
                    screen.set_theme(*theme);
                }
                Some(Box::new(TerminalDevice::new(self.every, screen)?))
            }
            DisplayMode::Text => Some(Box::new(TextDevice::new(self.every))),
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

        let mut loop_number: u64 = 0;
        let mut timeout: Option<Duration> = None;
        loop {
            let targets_updated = targets.refresh();
            if timeout.is_none() {
                targets.collect(&mut collector);
            }
            if let Some(ref mut device) = device {
                device.render(&collector, targets_updated)?;
            }

            if let Some(count) = self.count {
                loop_number += 1;
                if loop_number >= count {
                    break;
                }
            }
            if let Some(ref mut device) = device {
                match device.pause(timeout)? {
                    PauseStatus::Stop => break,
                    PauseStatus::TimeOut => timeout = None,
                    PauseStatus::Remaining(remaining) => timeout = Some(remaining),
                }
            } else {
                thread::sleep(self.every);
            }
        }

        if let Some(ref mut device) = device {
            device.close()?;
        }
        info!("stopping");
        Ok(())
    }
}
