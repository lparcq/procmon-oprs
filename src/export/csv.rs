// Oprs -- process monitor for Linux
// Copyright (C) 2020-2024  Laurent Pelecq
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

use libc::pid_t;
use memchr::memchr;
use std::{
    borrow::Cow,
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::{self, Seek, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use crate::{
    cfg::{ExportSettings, ExportType},
    process::{Aggregation, Collector, FormattedMetric, ProcessIdentity},
};

use super::{Exporter, SliceIter};

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("csv: missing count")]
    MissingCount,
}

trait ToStr {
    fn to_str(&self) -> Cow<str>;
}

impl ToStr for &String {
    fn to_str(&self) -> Cow<str> {
        Cow::Borrowed(self)
    }
}

impl ToStr for &u64 {
    fn to_str(&self) -> Cow<str> {
        Cow::Owned(format!("{self}"))
    }
}

/// Print a line of CSV
struct CsvLineOutput<'a> {
    out: &'a mut dyn Write,
    separator: char,
}

impl<'a> CsvLineOutput<'a> {
    fn new(out: &'a mut dyn Write, separator: char) -> Self {
        Self { out, separator }
    }

    /// Quote value if required. Assume there is no quote in the value.
    fn write_value(&mut self, value: &str) -> io::Result<()> {
        match memchr(self.separator as u8, value.as_bytes()) {
            None => write!(self.out, "{value}"),
            _ => write!(self.out, "\"{value}\""),
        }
    }

    /// Write the end of CSV line
    fn write_line_rest<I, D>(&mut self, row: I) -> io::Result<()>
    where
        I: IntoIterator<Item = D>,
        D: ToStr,
    {
        for value in row.into_iter() {
            write!(self.out, "{}", self.separator)?;
            self.write_value(&value.to_str())?;
        }
        writeln!(self.out)?;
        Ok(())
    }

    /// Write a CSV line
    fn write_line<I, D>(&mut self, row: I) -> io::Result<()>
    where
        I: IntoIterator<Item = D>,
        D: ToStr,
    {
        let mut iter = row.into_iter();
        if let Some(first) = iter.next() {
            self.write_value(&first.to_str())?;
            self.write_line_rest(iter)?;
        }
        Ok(())
    }
}

pub struct CsvExporter {
    separator: char,
    extension: &'static str,
    dir: PathBuf,
    count: Option<usize>,
    size: Option<u64>,
    files: HashMap<pid_t, File>,
    header: Vec<String>,
}

impl CsvExporter {
    pub fn new(settings: &ExportSettings) -> anyhow::Result<CsvExporter> {
        let (separator, extension) = match settings.kind {
            ExportType::Csv => (',', "csv"),
            ExportType::Tsv => ('\t', "tsv"),
            _ => panic!("internal error: kind should be csv or tsv"),
        };
        let count = if settings.size.is_some() {
            Some(settings.count.ok_or(Error::MissingCount)?)
        } else {
            None
        };
        Ok(CsvExporter {
            separator,
            extension,
            dir: settings.dir.clone(),
            count,
            size: settings.size,
            files: HashMap::new(),
            header: Vec::new(),
        })
    }

    /// Create a file and write the header
    fn create_file(&mut self, pid: pid_t, name: &str) -> io::Result<()> {
        let filename = self
            .dir
            .join(format!("{}_{}.{}", name, pid, self.extension));
        if filename.exists() {
            self.shift_file(&filename, 0)?;
        }
        let mut file = File::create(filename)?;
        let mut lout = CsvLineOutput::new(&mut file, self.separator);
        lout.write_line(self.header.iter())?;
        self.files.insert(pid, file);
        Ok(())
    }

    fn shifted_name<P>(filename: P, rank: usize) -> PathBuf
    where
        P: AsRef<Path>,
    {
        let mut name = filename.as_ref().as_os_str().to_os_string();
        let ext = format!(".{rank}");
        name.push(ext.as_str());
        PathBuf::from(name)
    }

    /// Shift all files keeping only the last ones
    fn shift_file<P>(&self, filename: P, rank: usize) -> io::Result<()>
    where
        P: AsRef<Path>,
    {
        if let Some(count) = self.count {
            if rank + 1 < count {
                let source = if rank == 0 {
                    filename.as_ref().to_path_buf()
                } else {
                    CsvExporter::shifted_name(filename.as_ref(), rank)
                };
                let destination = CsvExporter::shifted_name(filename.as_ref(), rank + 1);
                if destination.exists() {
                    self.shift_file(filename, rank + 1)?;
                }
                fs::rename(source, destination)?;
            }
        }
        Ok(())
    }
}

impl Exporter for CsvExporter {
    fn open(&mut self, metrics: SliceIter<FormattedMetric>) -> anyhow::Result<()> {
        let mut last_id = None;
        self.header.push(String::from("time"));
        Collector::for_each_computed_metric(metrics, |id, ag| {
            if last_id.is_none() || last_id.unwrap() != id {
                last_id = Some(id);
                self.header.push(id.as_str().to_string());
            } else {
                let name = format!(
                    "{} ({})",
                    id.as_str(),
                    match ag {
                        Aggregation::None => "none", // never used
                        Aggregation::Min => "min",
                        Aggregation::Max => "max",
                        Aggregation::Ratio => "%",
                    }
                );
                self.header.push(name);
            }
        });
        Ok(())
    }

    fn close(&mut self) -> anyhow::Result<()> {
        for (_, file) in self.files.drain() {
            file.sync_all()?;
        }
        Ok(())
    }

    fn export(&mut self, collector: &Collector, timestamp: &Duration) -> anyhow::Result<()> {
        let mut pids: HashSet<pid_t> = self.files.keys().copied().collect();
        for pstat in collector.lines() {
            let pid = pstat.pid();
            if !pids.remove(&pid) {
                self.create_file(pid, pstat.name())?;
            }
            let samples = pstat.samples().flat_map(|sample| sample.values());
            if let Some(ref mut file) = self.files.get_mut(&pid) {
                // Necessarily true
                write!(file, "{:.3}", timestamp.as_secs_f64())?;
                let mut lout = CsvLineOutput::new(file, self.separator);
                lout.write_line_rest(samples)?;
                if let Some(size) = self.size {
                    let written = file.seek(io::SeekFrom::End(0))?;
                    if written >= size {
                        pids.insert(pid); // file will be closed
                    }
                }
            }
        }
        for pid in pids {
            self.files.remove(&pid);
        }
        Ok(())
    }
}

#[cfg(test)]
mod test {

    use std::borrow::Cow;
    use std::fmt::Display;
    use std::io::{self, BufRead, Seek};

    use super::{CsvLineOutput, ToStr};

    impl ToStr for &str {
        fn to_str(&self) -> Cow<str> {
            Cow::Owned(self.to_string())
        }
    }

    fn write_csv_line<I, D>(values: I) -> io::Result<String>
    where
        I: IntoIterator<Item = D>,
        D: Display + ToStr,
    {
        let mut buf = io::Cursor::new(Vec::<u8>::new());
        let mut lout = CsvLineOutput::new(&mut buf, ',');
        lout.write_line(values.into_iter())?;
        buf.rewind()?;
        let mut line = String::new();
        buf.read_line(&mut line)?;
        Ok(line)
    }

    #[test]
    fn write_csv_line_of_string() -> io::Result<()> {
        let values = ["abc", "def"];
        let line = write_csv_line(values.iter().copied())?;
        assert_eq!("abc,def\n", line);
        Ok(())
    }

    #[test]
    fn write_csv_line_of_integer() -> io::Result<()> {
        let values = [123u64, 456u64];
        let line = write_csv_line(values.iter())?;
        assert_eq!("123,456\n", line);
        Ok(())
    }

    #[test]
    fn write_quoted_csv() -> io::Result<()> {
        let values = ["123,4", "567,5"];
        let line = write_csv_line(values.iter().copied())?;
        assert_eq!("\"123,4\",\"567,5\"\n", line);
        Ok(())
    }
}
