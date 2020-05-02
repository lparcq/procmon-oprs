use clap::arg_enum;
use std::time;
use strum::{EnumMessage, IntoEnumIterator};
use thiserror::Error;

use crate::cfg;
use crate::info::SystemConf;
use crate::metric::{parse_metric_names, MetricId};
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

pub fn run(
    settings: &config::Config,
    metric_names: &[String],
    target_ids: &[TargetId],
    output_type: OutputType,
) -> anyhow::Result<()> {
    let every_ms = time::Duration::from_millis(
        (settings
            .get_float(cfg::KEY_EVERY)
            .map_err(|_| Error::InvalidParameter(cfg::KEY_EVERY))?
            * 1000.0) as u64,
    );
    let system_conf = SystemConf::new()?;
    let mut metric_ids = Vec::new();
    let mut formatters = Vec::new();
    let human_format = settings.get_bool(cfg::KEY_HUMAN_FORMAT).unwrap_or(false);
    parse_metric_names(&mut metric_ids, &mut formatters, metric_names, human_format)?;
    let count = settings.get_int(cfg::KEY_COUNT).map(|c| c as u64).ok();
    let use_term = match output_type {
        OutputType::Any | OutputType::Term => TerminalOutput::is_available(),
        _ => false,
    };
    let mut output: Box<dyn Output> = if use_term {
        Box::new(TerminalOutput::new(
            target_ids,
            metric_ids,
            formatters,
            &system_conf,
        )?)
    } else {
        Box::new(TextOutput::new(
            target_ids,
            metric_ids,
            formatters,
            &system_conf,
        )?)
    };
    output.run(every_ms, count)?;
    Ok(())
}
