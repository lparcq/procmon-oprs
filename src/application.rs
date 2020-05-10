use clap::arg_enum;
use log::info;
use std::time;
use strum::{EnumMessage, IntoEnumIterator};
use thiserror::Error;

use crate::cfg;
use crate::collector::GridCollector;
use crate::format::Formatter;
use crate::info::SystemConf;
use crate::metrics::{AggregationMap, MetricId, MetricNamesParser};
use crate::output::{Output, TerminalOutput, TextOutput};
use crate::targets::{TargetContainer, TargetId};

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
    every: time::Duration,
    count: Option<u64>,
    metric_ids: Vec<MetricId>,
    aggregations: AggregationMap,
    formatters: Vec<Formatter>,
}

impl Application {
    pub fn new(settings: &config::Config, metric_names: &[String]) -> anyhow::Result<Application> {
        let every = time::Duration::from_millis(
            (settings
                .get_float(cfg::KEY_EVERY)
                .map_err(|_| Error::InvalidParameter(cfg::KEY_EVERY))?
                * 1000.0) as u64,
        );
        let count = settings.get_int(cfg::KEY_COUNT).map(|c| c as u64).ok();
        let human_format = settings.get_bool(cfg::KEY_HUMAN_FORMAT).unwrap_or(false);
        let mut metrics_parser = MetricNamesParser::new(human_format);
        metrics_parser.parse_metric_names(metric_names)?;

        Ok(Application {
            every,
            count,
            metric_ids: metrics_parser.get_metric_ids().to_vec(),
            aggregations: metrics_parser.get_aggregations().clone(),
            formatters: metrics_parser.get_formatters().to_vec(),
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
        let mut collector = GridCollector::new(
            target_ids.len(),
            self.metric_ids.to_vec(),
            &self.aggregations,
        );

        output.open(&collector)?;

        let mut loop_number: u64 = 0;
        loop {
            let targets_updated = targets.refresh();
            targets.collect(&mut collector);
            output.render(&collector, &self.formatters, targets_updated)?;

            if let Some(count) = self.count {
                loop_number += 1;
                if loop_number >= count {
                    break;
                }
            }
            if !output.pause()? {
                break;
            }
        }

        output.close()?;
        info!("stopping");
        Ok(())
    }
}
