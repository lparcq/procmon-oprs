// Oprs -- process monitor for Linux
// Copyright (C) 2025  Laurent Pelecq
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

use std::io;
use std::path::Path;

const PATH_PREFIX: &str = "content:";

pub struct File {
    content: String,
    pos: usize,
}

impl File {
    pub fn open<P>(path: P) -> io::Result<File>
    where
        P: AsRef<Path>,
    {
        let content = path
            .as_ref()
            .to_str()
            .ok_or_else(|| io::Error::other("wrong string in test"))?;
        if !content.starts_with(PATH_PREFIX) {
            Err(io::Error::other("invalid test path"))
        } else {
            let content: String = content.chars().skip(PATH_PREFIX.len()).collect();
            Ok(File { content, pos: 0 })
        }
    }
}

impl io::Read for File {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let rest_len = self.content.len() - self.pos;
        if rest_len == 0 {
            Ok(0)
        } else {
            let content = &self.content.as_bytes()[self.pos..];
            if buf.len() <= rest_len {
                let buflen = buf.len();
                buf.copy_from_slice(&content[..buflen]);
                self.pos += buflen;
                Ok(buflen)
            } else {
                buf[..rest_len].copy_from_slice(content);
                self.pos += rest_len;
                Ok(rest_len)
            }
        }
    }
}

#[cfg(test)]
mod tests {

    use std::io::Read;

    use super::File;

    #[test]
    fn test_file_open() {
        assert!(File::open("content:ok").is_ok());
        assert!(File::open("nok").is_err());
    }

    #[test]
    fn test_read_small() {
        let mut buf: [u8; 10] = [0; 10];
        let mut file = File::open("content:ok").unwrap();
        let mut nread = file.read(&mut buf).unwrap();
        assert_eq!(2, nread);
        assert_eq!([b'o', b'k'], buf[0..2]);
        nread = file.read(&mut buf).unwrap();
        assert_eq!(0, nread);
    }

    #[test]
    fn test_read_equal() {
        let mut buf: [u8; 5] = [0; 5];
        let mut file = File::open("content:seven").unwrap();
        let mut nread = file.read(&mut buf).unwrap();
        assert_eq!(5, nread);
        assert_eq!([b's', b'e', b'v', b'e', b'n'], buf);
        nread = file.read(&mut buf).unwrap();
        assert_eq!(0, nread);
    }

    #[test]
    fn test_read_big() {
        let mut buf: [u8; 2] = [0; 2];
        let mut file = File::open("content:one").unwrap();
        let mut nread = file.read(&mut buf).unwrap();
        assert_eq!(2, nread);
        assert_eq!([b'o', b'n'], buf);
        nread = file.read(&mut buf).unwrap();
        assert_eq!(1, nread);
        assert_eq!([b'e', b'n'], buf);
        nread = file.read(&mut buf).unwrap();
        assert_eq!(0, nread);
    }
}
