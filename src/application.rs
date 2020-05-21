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
use std::time::Duration;
use strum::{EnumMessage, IntoEnumIterator};
use thiserror::Error;

use crate::{
    agg::Aggregation,
    cfg,
    collector::Collector,
    info::SystemConf,
    metrics::{FormattedMetric, MetricId, MetricNamesParser},
    output::{Output, PauseStatus, TerminalOutput, TextOutput},
    targets::{TargetContainer, TargetId},
};

arg_enum! {
    #[derive(Debug)]
    pub enum OutputType {
        Any,
        Text,
        Term,
    }
}

#[derive(Error, Debug)]
enum Error {
    #[error("{0}: invalid parameter value")]
    InvalidParameter(&'static str),
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

        Ok(Application {
            every,
            count,
            metrics: metrics_parser.parse(metric_names)?,
        })
    }

    pub fn run<'a>(
        &mut self,
        output_type: OutputType,
        target_ids: &[TargetId],
        system_conf: &'a SystemConf,
    ) -> anyhow::Result<()> {
        info!("starting");
        let use_term = match output_type {
            OutputType::Any | OutputType::Term => TerminalOutput::is_available(),
            _ => false,
        };
        let mut output: Box<dyn Output> = if use_term {
            Box::new(TerminalOutput::new(self.every)?)
        } else {
            Box::new(TextOutput::new(self.every))
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

        output.open(&collector)?;

        let mut loop_number: u64 = 0;
        let mut timeout: Option<Duration> = None;
        loop {
            let targets_updated = targets.refresh();
            if timeout.is_none() {
                targets.collect(&mut collector);
            }
            output.render(&collector, targets_updated)?;

            if let Some(count) = self.count {
                loop_number += 1;
                if loop_number >= count {
                    break;
                }
            }
            match output.pause(timeout)? {
                PauseStatus::Stop => break,
                PauseStatus::TimeOut => timeout = None,
                PauseStatus::Remaining(remaining) => timeout = Some(remaining),
            }
        }

        output.close()?;
        info!("stopping");
        Ok(())
    }
}
