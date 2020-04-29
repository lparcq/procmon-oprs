use std::time;
use thiserror::Error;

use crate::cfg;
use crate::info::SystemConf;
use crate::metric::{parse_metric_names, MetricMapper};
use crate::output::{Output, TextOutput};
use crate::targets::TargetId;

#[derive(Error, Debug)]
enum Error {
    #[error("{0}: invalid parameter value")]
    InvalidParameter(&'static str),
}

pub fn list_metrics() {
    let metric_mapper = MetricMapper::new();
    metric_mapper.for_each(|id, name| {
        println!("{:<15}\t{}", name, MetricMapper::help(id));
    })
}

pub fn run(
    settings: &config::Config,
    metric_names: &[String],
    target_ids: &[TargetId],
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
    let mut output = TextOutput::new(target_ids, metric_ids, formatters, &system_conf)?;
    output.run(every_ms, count);
    Ok(())
}
