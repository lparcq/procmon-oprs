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

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// Catch SIGINT and SIGTERM.
pub struct SignalHandler {
    caught: Arc<AtomicBool>,
}

impl SignalHandler {
    pub fn new() -> Result<SignalHandler, ctrlc::Error> {
        let caught = Arc::new(AtomicBool::new(false));
        let moved_caught = caught.clone();
        ctrlc::set_handler(move || {
            moved_caught.store(true, Ordering::SeqCst);
        })?;
        Ok(SignalHandler { caught })
    }

    pub fn caught(&self) -> bool {
        self.caught.load(Ordering::SeqCst)
    }
}
