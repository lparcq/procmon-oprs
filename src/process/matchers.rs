// Oprs -- process monitor for Linux
// Copyright (C) 2024 Laurent Pelecq
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

use globset::{Glob, GlobSetBuilder};

use super::Forest;

/// Return all processes with a name matching one of the patterns.
pub fn glob(patterns: &[String]) -> anyhow::Result<HashSet<String>> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        builder.add(Glob::new(pattern)?);
    }
    let gset = builder.build()?;
    let forest = {
        let mut forest = Forest::new();
        forest.refresh()?;
        forest
    };
    Ok(HashSet::from_iter(forest.filter_collect(|pinfo| {
        if gset.is_match(pinfo.name())
            || pinfo
                .process()
                .cmdline()
                .ok()
                .map_or(false, |cmdline| match cmdline.first() {
                    Some(path) => gset.is_match(path),
                    None => false,
                })
        {
            Some(pinfo.name().to_string())
        } else {
            None
        }
    })))
}
