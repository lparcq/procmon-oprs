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

use std::io;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use termion::input::TermRead;

pub use termion::{
    event::{Event, Key},
    is_tty,
};

type InputResult = io::Result<Event>;

type InputOptionalResult = io::Result<Option<Event>>;

pub struct EventChannel {
    chin: mpsc::Receiver<InputResult>,
}

impl EventChannel {
    pub fn new() -> EventChannel {
        let (chout, chin) = mpsc::channel();
        thread::spawn(move || {
            for res in io::stdin().events() {
                if chout.send(res).is_err() {
                    break;
                }
            }
        });
        EventChannel { chin }
    }

    fn disconnected() -> io::Error {
        io::Error::new(io::ErrorKind::ConnectionAborted, "channel disconnected")
    }

    pub fn receive_timeout(&self, timeout: Duration) -> InputOptionalResult {
        match self.chin.recv_timeout(timeout) {
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(_) => Err(EventChannel::disconnected()),
            Ok(res) => res.map(Some),
        }
    }
}
