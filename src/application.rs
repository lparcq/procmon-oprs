use clap::arg_enum;
use log::info;
use std::time;
use strum::{EnumMessage, IntoEnumIterator};
use thiserror::Error;

use crate::cfg;
use crate::info::SystemConf;
use crate::metric::{MetricId, MetricNamesParser};
use crate::output::{Output, TerminalOutput, TextOutput};
use crate::targets::TargetId;

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
pub struct Application<'a> {
    every: time::Duration,
    count: Option<u64>,
    output: Box<(dyn Output + 'a)>,
}

impl<'a> Application<'a> {
    pub fn new(
        settings: &config::Config,
        metric_names: &[String],
        target_ids: &[TargetId],
        output_type: OutputType,
        system_conf: &'a SystemConf,
    ) -> anyhow::Result<Application<'a>> {
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
        let use_term = match output_type {
            OutputType::Any | OutputType::Term => TerminalOutput::is_available(),
            _ => false,
        };
        let output: Box<dyn Output> = if use_term {
            Box::new(TerminalOutput::new(
                target_ids,
                metrics_parser.get_metric_ids(),
                metrics_parser.get_formatters(),
                &system_conf,
            )?)
        } else {
            Box::new(TextOutput::new(
                target_ids,
                metrics_parser.get_metric_ids(),
                metrics_parser.get_formatters(),
                &system_conf,
            )?)
        };

        Ok(Application {
            every,
            count,
            output,
        })
    }

    pub fn run(&mut self) -> anyhow::Result<()> {
        info!("starting");
        self.output.run(self.every, self.count)?;
        info!("stopping");
        Ok(())
    }
}
