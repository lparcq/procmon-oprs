use std::time;
use thiserror::Error;

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
            .get_float("every")
            .map_err(|_| Error::InvalidParameter("every"))?
            * 1000.0) as u64,
    );
    let mut metric_ids = Vec::new();
    let mut formatters = Vec::new();
    parse_metric_names(&mut metric_ids, &mut formatters, metric_names)?;
    let count = settings.get_int("count").map(|c| c as u64).ok();
    let mut output = TextOutput::new(target_ids, metric_ids, formatters)?;
    output.run(every_ms, count);
    Ok(())
}
