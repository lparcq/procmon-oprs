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

use strum_macros::EnumString;
use supports_color::Stream;

pub use self::input::{is_tty, Event, EventChannel, Key};

pub mod charset;

mod input;

#[derive(Clone, Copy, Debug, EnumString, PartialEq, Eq)]
pub enum BuiltinTheme {
    #[strum(serialize = "light")]
    Light,
    #[strum(serialize = "dark")]
    Dark,
    #[strum(serialize = "light16")]
    Light16,
    #[strum(serialize = "dark16")]
    Dark16,
}

impl BuiltinTheme {
    /// Guess the theme
    pub fn guess() -> Option<BuiltinTheme> {
        let timeout = std::time::Duration::from_millis(100);
        match termbg::theme(timeout) {
            Err(err) => {
                log::info!("cannot guess theme: {err:?}");
                None
            }
            Ok(theme) => match (theme, supports_color::on(Stream::Stdout)) {
                (termbg::Theme::Dark, Some(support)) if support.has_16m || support.has_256 => {
                    Some(BuiltinTheme::Dark)
                }
                (termbg::Theme::Light, Some(support)) if support.has_16m || support.has_256 => {
                    Some(BuiltinTheme::Light)
                }
                (termbg::Theme::Dark, Some(support)) if support.has_basic => {
                    Some(BuiltinTheme::Dark16)
                }
                (termbg::Theme::Light, Some(support)) if support.has_basic => {
                    Some(BuiltinTheme::Light16)
                }
                _ => None,
            },
        }
    }
}
