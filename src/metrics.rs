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

use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::result;
use strum_macros::{EnumIter, EnumMessage, EnumString, IntoStaticStr};

use crate::{
    agg::{Aggregation, AggregationSet},
    format::{self, Formatter},
    parsers::parse_metric_spec,
};

const SHORT_NAME_MAX_LEN: usize = 10;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("{0}: duplicate metric")]
    DuplicateMetric(String),
    #[error("invalid syntax: {0}")]
    InvalidSyntax(String),
    #[error("{0}: unknown metric or pattern")]
    UnknownMetric(String),
}

/// Metric data type
///
/// There are two types:
/// - Counter is always increasing such as the number of read calls
/// - Gauge is a positive number that may decrease like the memory consumption
pub enum MetricDataType {
    Counter,
    Gauge,
}

/// Metrics that can be collected for a process
#[derive(
    Copy,
    Clone,
    Debug,
    Hash,
    PartialEq,
    Eq,
    PartialOrd,
    EnumIter,
    EnumString,
    EnumMessage,
    IntoStaticStr,
)]
pub enum MetricId {
    #[strum(serialize = "fault:minor", message = "page faults without disk access")]
    FaultMinor,
    #[strum(serialize = "fault:major", message = "page faults with disk access")]
    FaultMajor,
    #[strum(serialize = "fd:all", message = "number of file descriptors")]
    FdAll,
    #[strum(serialize = "fd:high", message = "highest value of file descriptors")]
    FdHigh,
    #[strum(serialize = "fd:file", message = "number of files")]
    FdFile,
    #[strum(serialize = "fd:socket", message = "number of sockets")]
    FdSocket,
    #[strum(serialize = "fd:net", message = "number of net file descriptors")]
    FdNet,
    #[strum(serialize = "fd:pipe", message = "number of pipes")]
    FdPipe,
    #[strum(
        serialize = "fd:anon",
        message = "number of file decriptors without inode "
    )]
    FdAnon,
    #[strum(serialize = "fd:mfd", message = "number of in-memory file")]
    FdMemFile,
    #[strum(
        serialize = "fd:other",
        message = "number of file descriptors in no other category"
    )]
    FdOther,
    #[strum(
        serialize = "io:read:call",
        message = "number of read operations with system calls such as read(2) and pread(2)"
    )]
    IoReadCall,
    #[strum(
        serialize = "io:read:total",
        message = "number of bytes read from storage or page cache"
    )]
    IoReadTotal,
    #[strum(
        serialize = "io:read:storage",
        message = "number of bytes really fetched from storage"
    )]
    IoReadStorage,
    #[strum(
        serialize = "io:write:call",
        message = "number of write operations with system calls such as write(2) and pwrite(2)"
    )]
    IoWriteCall,
    #[strum(
        serialize = "io:write:total",
        message = "number of bytes written to storage or page cache"
    )]
    IoWriteTotal,
    #[strum(
        serialize = "io:write:storage",
        message = "number of bytes really sent to storage"
    )]
    IoWriteStorage,
    #[strum(
        serialize = "map:anon:count",
        message = "number of anonymous mapped memory region"
    )]
    MapAnonCount,
    #[strum(
        serialize = "map:anon:size",
        message = "total size of mapped heap region"
    )]
    MapAnonSize,
    #[strum(serialize = "map:heap:count", message = "number of mapped heap region")]
    MapHeapCount,
    #[strum(
        serialize = "map:heap:size",
        message = "total size of memory mapped files"
    )]
    MapHeapSize,
    #[strum(
        serialize = "map:file:count",
        message = "number of memory mapped files"
    )]
    MapFileCount,
    #[strum(
        serialize = "map:file:size",
        message = "total size of mapped main stack"
    )]
    MapFileSize,
    #[strum(
        serialize = "map:stack:count",
        message = "number of mapped main stack (always 1)"
    )]
    MapStackCount,
    #[strum(
        serialize = "map:stack:size",
        message = "total size of mapped thread stacks"
    )]
    MapStackSize,
    #[strum(
        serialize = "map:tstack:count",
        message = "number of mapped thread stacks"
    )]
    MapThreadStackCount,
    #[strum(
        serialize = "map:tstack:size",
        message = "total size of mapped vdso region, see: vdso(7)"
    )]
    MapThreadStackSize,
    #[strum(
        serialize = "map:vdso:count",
        message = "number of mapped vdso region, see: vdso(7)"
    )]
    MapVdsoCount,
    #[strum(
        serialize = "map:vdso:size",
        message = "total size of mapped vdso region, see: vdso(7)"
    )]
    MapVdsoSize,
    #[strum(
        serialize = "map:vsys:count",
        message = "number of shared memory segment"
    )]
    MapVsysCount,
    #[strum(
        serialize = "map:vsys:size",
        message = "total size of shared memory segments"
    )]
    MapVsysSize,
    #[strum(
        serialize = "map:vsyscall:count",
        message = "number of mapped vsyscall region, see: vdso(7)"
    )]
    MapVsyscallCount,
    #[strum(
        serialize = "map:vsyscall:size",
        message = "total size of mapped vsyscall region, see: vdso(7)"
    )]
    MapVsyscallSize,
    #[strum(
        serialize = "map:vvar:count",
        message = "number of mapped kernel variable, see: vdso(7)"
    )]
    MapVvarCount,
    #[strum(
        serialize = "map:vvar:size",
        message = "total size of mapped kernel variable, see: vdso(7)"
    )]
    MapVvarSize,
    #[strum(
        serialize = "map:other:count",
        message = "number of other mapped memory region"
    )]
    MapOtherCount,
    #[strum(
        serialize = "map:other:size",
        message = "total size of other mapped memory region"
    )]
    MapOtherSize,
    #[strum(serialize = "mem:rss", message = "resident set size")]
    MemRss,
    #[strum(serialize = "mem:vm", message = "virtual memory")]
    MemVm,
    #[strum(serialize = "mem:text", message = "text size (code)")]
    MemText,
    #[strum(serialize = "mem:data", message = "data + stack size")]
    MemData,
    #[strum(
        serialize = "time:elapsed",
        message = "elapsed time since process started"
    )]
    TimeElapsed,
    #[strum(
        serialize = "time:cpu",
        message = "elapsed time in kernel or user mode"
    )]
    TimeCpu,
    #[strum(serialize = "time:system", message = "elapsed time in kernel mode")]
    TimeSystem,
    #[strum(serialize = "time:user", message = "elapsed time in user mode")]
    TimeUser,
    #[strum(serialize = "thread:count", message = "number of threads")]
    ThreadCount,
}

impl MetricId {
    pub fn as_str(self) -> &'static str {
        self.into()
    }

    /// Return a string of less than SHORT_NAME_MAX_LEN characters.
    pub fn to_short_str(self) -> Option<&'static str> {
        match self {
            MetricId::FaultMinor => Some("flt:min"),
            MetricId::FaultMajor => Some("flt:maj"),
            MetricId::IoReadCall => Some("rd:call"),
            MetricId::IoReadTotal => Some("rd:total"),
            MetricId::IoReadStorage => Some("rd:store"),
            MetricId::IoWriteCall => Some("wr:call"),
            MetricId::IoWriteTotal => Some("wr:total"),
            MetricId::IoWriteStorage => Some("wr:store"),
            MetricId::MapAnonCount => Some("m:anon:cnt"),
            MetricId::MapHeapCount => Some("m:heap:cnt"),
            MetricId::MapFileCount => Some("m:file:cnt"),
            MetricId::MapStackCount => Some("m:stk:cnt"),
            MetricId::MapThreadStackCount => Some("m:tsck:cnt"),
            MetricId::MapVdsoCount => Some("m:vdso:cnt"),
            MetricId::MapVsysCount => Some("m:vsys:cnt"),
            MetricId::MapVsyscallCount => Some("m:vsc:cnt"),
            MetricId::MapVvarCount => Some("m:vv:cnt"),
            MetricId::MapOtherCount => Some("m:oth:cnt"),
            MetricId::MapAnonSize => Some("m:anon:sz"),
            MetricId::MapHeapSize => Some("m:heap:sz"),
            MetricId::MapFileSize => Some("m:file:sz"),
            MetricId::MapStackSize => Some("m:stk:sz"),
            MetricId::MapThreadStackSize => Some("m:tstk:sz"),
            MetricId::MapVdsoSize => Some("m:vdso:sz"),
            MetricId::MapVsysSize => Some("m:vsys:sz"),
            MetricId::MapVsyscallSize => Some("m:vsc:sz"),
            MetricId::MapVvarSize => Some("m:vv:sz"),
            MetricId::MapOtherSize => Some("m:oth:sz"),
            MetricId::TimeElapsed => Some("tm:elapsed"),
            MetricId::TimeCpu => Some("tm:cpu"),
            MetricId::TimeSystem => Some("tm:sys"),
            MetricId::TimeUser => Some("tm:user"),
            MetricId::ThreadCount => Some("thread:cnt"),
            _ => {
                let name: &'static str = self.into();
                if name.len() > SHORT_NAME_MAX_LEN {
                    panic!("{}: internal error, no short name", name)
                }
                Some(name)
            }
        }
    }

    /// The data type either counter (always increasing) or gauge (varying but positive).
    pub fn data_type(self) -> MetricDataType {
        match self {
            MetricId::FaultMinor | MetricId::FaultMajor => MetricDataType::Counter,
            MetricId::FdAll
            | MetricId::FdHigh
            | MetricId::FdFile
            | MetricId::FdSocket
            | MetricId::FdNet
            | MetricId::FdPipe
            | MetricId::FdAnon
            | MetricId::FdMemFile
            | MetricId::FdOther => MetricDataType::Gauge,
            MetricId::IoReadCall
            | MetricId::IoReadTotal
            | MetricId::IoReadStorage
            | MetricId::IoWriteCall
            | MetricId::IoWriteTotal
            | MetricId::IoWriteStorage => MetricDataType::Counter,
            MetricId::MapAnonSize
            | MetricId::MapAnonCount
            | MetricId::MapHeapSize
            | MetricId::MapHeapCount
            | MetricId::MapFileSize
            | MetricId::MapFileCount
            | MetricId::MapStackSize
            | MetricId::MapStackCount
            | MetricId::MapThreadStackSize
            | MetricId::MapThreadStackCount
            | MetricId::MapVdsoSize
            | MetricId::MapVdsoCount
            | MetricId::MapVsysSize
            | MetricId::MapVsysCount
            | MetricId::MapVsyscallSize
            | MetricId::MapVsyscallCount
            | MetricId::MapVvarSize
            | MetricId::MapVvarCount
            | MetricId::MapOtherSize
            | MetricId::MapOtherCount => MetricDataType::Gauge,
            MetricId::MemRss | MetricId::MemVm | MetricId::MemText | MetricId::MemData => {
                MetricDataType::Gauge
            }
            MetricId::TimeElapsed
            | MetricId::TimeCpu
            | MetricId::TimeSystem
            | MetricId::TimeUser => MetricDataType::Counter,
            MetricId::ThreadCount => MetricDataType::Gauge,
        }
    }
}

impl fmt::Display for MetricId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Metric with associated aggregations and a formatter function
pub struct FormattedMetric {
    pub id: MetricId,
    pub aggregations: AggregationSet,
    pub format: Formatter,
}

impl FormattedMetric {
    fn new(id: MetricId, aggregations: AggregationSet, format: Formatter) -> FormattedMetric {
        FormattedMetric {
            id,
            aggregations,
            format,
        }
    }

    /// Return true if it exists a limit associated to the metric for a given process
    ///
    /// A limit makes sense only for the raw value, not aggregations.
    pub fn has_limit(&self) -> bool {
        matches!(
            self.id,
            MetricId::FdAll
                | MetricId::MapStackSize
                | MetricId::MemRss
                | MetricId::MemVm
                | MetricId::ThreadCount
                | MetricId::TimeCpu
        ) && self.aggregations.has(Aggregation::None)
    }
}

/// Metric names parser
pub struct MetricNamesParser {
    human_format: bool,
}

impl MetricNamesParser {
    pub fn new(human_format: bool) -> MetricNamesParser {
        MetricNamesParser { human_format }
    }

    // Return the more readable format for a human
    fn get_human_format(id: MetricId) -> Formatter {
        match id {
            MetricId::IoReadCall
            | MetricId::IoReadTotal
            | MetricId::IoReadStorage
            | MetricId::IoWriteCall
            | MetricId::IoWriteTotal
            | MetricId::IoWriteStorage => format::size,
            MetricId::MapAnonSize
            | MetricId::MapHeapSize
            | MetricId::MapFileSize
            | MetricId::MapStackSize
            | MetricId::MapThreadStackSize
            | MetricId::MapVdsoSize
            | MetricId::MapVsyscallSize
            | MetricId::MapVvarSize
            | MetricId::MapOtherSize => format::size,
            MetricId::MemRss | MetricId::MemVm | MetricId::MemText | MetricId::MemData => {
                format::size
            }
            MetricId::TimeElapsed
            | MetricId::TimeCpu
            | MetricId::TimeSystem
            | MetricId::TimeUser => format::human_milliseconds,
            _ => format::identity,
        }
    }

    fn get_default_formatter(&self, id: MetricId) -> Formatter {
        if self.human_format {
            MetricNamesParser::get_human_format(id)
        } else {
            match id {
                MetricId::TimeElapsed
                | MetricId::TimeCpu
                | MetricId::TimeSystem
                | MetricId::TimeUser => format::seconds,
                _ => format::identity,
            }
        }
    }

    /// Return a list of metrics with aggregations and format
    pub fn parse(&mut self, names: &[String]) -> result::Result<Vec<FormattedMetric>, Error> {
        let mut metrics = Vec::new();
        let mut parsed_ids = HashSet::new();
        names
            .iter()
            .try_for_each(|name| match parse_metric_spec(name.as_str()) {
                Ok((metric_ids, aggs, fmt)) => {
                    if metric_ids.is_empty() {
                        return Err(Error::UnknownMetric(name.to_string()));
                    }
                    for id in metric_ids {
                        if parsed_ids.contains(&id) {
                            return Err(Error::DuplicateMetric(id.as_str().to_string()));
                        } else {
                            parsed_ids.insert(id);
                            metrics.push(FormattedMetric::new(
                                id,
                                aggs,
                                fmt.unwrap_or_else(|| self.get_default_formatter(id)),
                            ));
                        }
                    }
                    Ok(())
                }
                Err(_) => Err(Error::InvalidSyntax(format!("{}: invalid metric", name))),
            })?;
        Ok(metrics)
    }
}

#[cfg(test)]
mod tests {

    use std::str::FromStr;
    use strum::{EnumMessage, IntoEnumIterator};

    use super::{MetricDataType, MetricId, MetricNamesParser};

    fn vec_of_string(vstr: &[&str]) -> Vec<String> {
        vstr.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_metricid_to_str() {
        assert_eq!("mem:vm", MetricId::MemVm.as_str());
        let name: &'static str = MetricId::MemVm.into();
        assert_eq!("mem:vm", name);
    }

    #[test]
    fn test_metricid_from_str() {
        assert_eq!(MetricId::MemVm, MetricId::from_str("mem:vm").unwrap());
    }

    #[test]
    fn test_metricid_help() {
        assert_eq!("virtual memory", MetricId::MemVm.get_message().unwrap());
        for metric_id in MetricId::iter() {
            assert!(
                metric_id.get_message().is_some(),
                "{}: no message",
                metric_id.as_str()
            )
        }
    }

    #[test]
    fn test_metricid_short_str() {
        for metric_id in MetricId::iter() {
            match metric_id.to_short_str() {
                Some(name) => assert!(
                    name.len() <= super::SHORT_NAME_MAX_LEN,
                    "{}: short name is too long",
                    metric_id.as_str()
                ),
                None => panic!("{} has no short name", metric_id.as_str()),
            }
        }
    }

    #[test]
    fn test_parse_metric_names() {
        let metric_names = vec_of_string(&[
            "fault:minor",
            "fault:major/k",
            "io:read:call",
            "io:read:total/sz",
            "io:read:storage",
            "io:write:call",
            "io:write:total",
            "io:write:storage",
            "mem:rss/mi",
            "mem:vm/ti",
            "mem:text/m",
            "mem:data/g",
            "time:elapsed/du",
            "time:system",
            "time:user",
            "thread:count",
        ]);
        // Check few metrics
        let mut parser1 = MetricNamesParser::new(false);
        let metrics1 = parser1.parse(&metric_names[0..2]).unwrap();
        assert_eq!(2, metrics1.len());

        // Check all metrics
        let mut parser2 = MetricNamesParser::new(false);
        let metric_count = metric_names.len();
        let metrics2 = parser2.parse(&metric_names).unwrap();
        assert_eq!(metric_count, metrics2.len());
    }

    #[test]
    fn test_expand_metric_names() {
        // Check prefix
        let metric_names1 = vec_of_string(&["mem:*"]);
        let mut parser1 = MetricNamesParser::new(false);
        let metrics1 = parser1.parse(&metric_names1).unwrap();
        assert_eq!(4, metrics1.len());

        // Check suffix
        let metric_names2 = vec_of_string(&["*:storage"]);
        let mut parser2 = MetricNamesParser::new(false);
        let metrics2 = parser2.parse(&metric_names2).unwrap();
        assert_eq!(2, metrics2.len());

        // Check middle
        let metric_names3 = vec_of_string(&["io:*:total"]);
        let mut parser3 = MetricNamesParser::new(false);
        let metrics3 = parser3.parse(&metric_names3).unwrap();
        assert_eq!(2, metrics3.len());
    }

    #[test]
    fn test_expand_metric_names_errors() {
        for pattern in &["mem:*:*", "me*", "not:*"] {
            let metric_names = vec_of_string(&[pattern]);
            let mut parser = MetricNamesParser::new(false);
            assert!(
                parser.parse(&metric_names).is_err(),
                "pattern \"{}\" works unexpectedly",
                pattern
            );
        }
    }

    #[test]
    fn data_type() {
        assert!(matches!(
            MetricId::FaultMajor.data_type(),
            MetricDataType::Counter
        ));
        assert!(matches!(MetricId::FdAll.data_type(), MetricDataType::Gauge));
        assert!(matches!(
            MetricId::IoReadTotal.data_type(),
            MetricDataType::Counter
        ));
        assert!(matches!(
            MetricId::MapHeapSize.data_type(),
            MetricDataType::Gauge
        ));
        assert!(matches!(MetricId::MemVm.data_type(), MetricDataType::Gauge));
        assert!(matches!(
            MetricId::TimeCpu.data_type(),
            MetricDataType::Counter
        ));
        assert!(matches!(
            MetricId::ThreadCount.data_type(),
            MetricDataType::Gauge
        ));
    }
}
