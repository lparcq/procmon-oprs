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

use libc::pid_t;
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::{agg::Aggregation, collector::Collector};

use super::Exporter;

pub struct CsvExporter {
    separator: &'static str,
    dir: PathBuf,
    files: HashMap<pid_t, File>,
    header: Vec<String>,
}

impl CsvExporter {
    pub fn new<P>(dir: P) -> CsvExporter
    where
        P: AsRef<Path>,
    {
        CsvExporter {
            separator: ",",
            dir: dir.as_ref().to_path_buf(),
            files: HashMap::new(),
            header: Vec::new(),
        }
    }

    /// Write the end of CSV line
    fn write_line_rest<I, D>(out: &mut dyn Write, row: I, separator: &str) -> io::Result<()>
    where
        I: IntoIterator<Item = D>,
        D: Display,
    {
        for value in row.into_iter() {
            write!(out, "{}{}", separator, value)?;
        }
        write!(out, "\n")?;
        Ok(())
    }

    /// Write a CSV line
    fn write_line<I, D>(out: &mut dyn Write, row: I, separator: &str) -> io::Result<()>
    where
        I: IntoIterator<Item = D>,
        D: Display,
    {
        let mut iter = row.into_iter();
        if let Some(first) = iter.next() {
            write!(out, "{}", first)?;
            CsvExporter::write_line_rest(out, iter, separator)?;
        }
        Ok(())
    }

    /// Create a file and write the header
    fn create_file(&mut self, pid: pid_t, name: &str) -> io::Result<()> {
        let filename = self.dir.join(format!("{}_{}.csv", name, pid));
        let mut file = File::create(filename)?;
        CsvExporter::write_line(&mut file, self.header.iter(), self.separator)?;
        self.files.insert(pid, file);
        Ok(())
    }
}

impl Exporter for CsvExporter {
    fn open(&mut self, collector: &Collector) -> anyhow::Result<()> {
        let mut last_id = None;
        self.header.push(String::from("time"));
        collector.for_each_computed_metric(|id, ag| {
            if last_id.is_none() || last_id.unwrap() != id {
                last_id = Some(id);
                self.header.push(id.to_str().to_string());
            } else {
                let name = format!(
                    "{} ({})",
                    id.to_str(),
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

    fn export(&mut self, collector: &Collector) -> anyhow::Result<()> {
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;

        let mut pids: HashSet<pid_t> = self.files.keys().map(|pid| *pid).collect();
        for proc in collector.lines() {
            let pid = proc.get_pid();
            if !pids.remove(&pid) {
                self.create_file(pid, proc.get_name())?;
            }
            let samples = proc.samples().map(|sample| sample.values()).flatten();
            if let Some(ref mut file) = self.files.get_mut(&pid) {
                // Necessarily true
                write!(file, "{:.3}", now.as_secs_f64())?;
                CsvExporter::write_line_rest(file, samples, self.separator)?;
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

    use std::fmt::Display;
    use std::io::{self, BufRead, Seek};

    use super::CsvExporter;

    fn write_csv_line<D: Display>(values: &[D]) -> io::Result<String> {
        let mut buf = io::Cursor::new(Vec::<u8>::new());
        CsvExporter::write_line(&mut buf, values.iter(), ",")?;
        buf.seek(io::SeekFrom::Start(0)).unwrap();
        let mut line = String::new();
        buf.read_line(&mut line)?;
        Ok(line)
    }

    #[test]
    fn write_csv_line_of_string() -> io::Result<()> {
        let line = write_csv_line(&["abc", "def"])?;
        assert_eq!("abc,def\n", line);
        Ok(())
    }

    #[test]
    fn write_csv_line_of_integer() -> io::Result<()> {
        let line = write_csv_line(&[123, 456])?;
        assert_eq!("123,456\n", line);
        Ok(())
    }
}
