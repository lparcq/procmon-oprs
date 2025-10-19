# Changelog

All notable changes to this project will be documented in this file.

## [0.42.0] - 2025-10-19

### 🚀 Features

- Better process name for interpreters like python.

## [0.41.0] - 2025-10-04

### 🚀 Features

- Upgrade procfs to version 0.18 with new file descriptor target.
- Display the script/jar name if the process is java, python or perl.

### 🚜 Refactor

- Modifie exe_name pour inclure le nom du jar ou du script entre parenthèses pour les processus Java, Python et Perl

### ⚙️ Miscellaneous Tasks

- Remove unused type Unbounded
- Release procmon-oprs version 0.41.0

## [0.40.0] - 2025-08-25

### 🐛 Bug Fixes

- Rename --regex as --regexp

### 🚜 Refactor

- Fix warnings

### ⚙️ Miscellaneous Tasks

- Release procmon-oprs version 0.40.0

## [0.39.1] - 2025-08-03

### 🐛 Bug Fixes

- Revert to main menu if process dies in details
- Do not clear marks with arrows, only search

### ⚙️ Miscellaneous Tasks

- Fix clippy warnings
- Update terminal-colorsaurus to version 1
- Update xdg to version 3
- Release procmon-oprs version 0.39.1

## [0.39.0] - 2025-06-15

### 🚀 Features

- New key bindings with function keys.
- Promote header in the help for better rendering.

### 🐛 Bug Fixes

- Escape in details pane.
- Correctly set the filter
- Exiting the limits revert to the details.
- Rewrite help with new key bindings.
- Correct selection of the parent process in the tree.

### 🚜 Refactor

- Implement hierarchical menus.

### ⚙️ Miscellaneous Tasks

- Update rstest to 0.25
- Update edition to 2024
- Document configuration types.
- Fix clippy warnings
- Release procmon-oprs version 0.39.0

## [0.38.0] - 2025-03-16

### 🚀 Features

- Display maps in process details.
- Add a filter for processes owned by the current user (myself)

### 🐛 Bug Fixes

- Correctly show permissions on files.
- Help for filter 'myself'

### ⚙️ Miscellaneous Tasks

- Release procmon-oprs version 0.38.0

## [0.37.1] - 2025-03-02

### 🐛 Bug Fixes

- Error messages in console breaks the UI.

### ⚙️ Miscellaneous Tasks

- Fix README to mention the new format 'units'
- Release procmon-oprs version 0.37.1

## [0.37.0] - 2025-02-23

### 🚀 Features

- Add feature tui to optionally disable terminal support
- [**breaking**] Change --format human to units.

### 🐛 Bug Fixes

- Add units back in non tui mode.

### ⚙️ Miscellaneous Tasks

- Num-traits is used only for feature tui.
- Release procmon-oprs version 0.37.0

## [0.36.0] - 2025-02-16

### 🚀 Features

- Display file descriptors opened by a process.

### 🐛 Bug Fixes

- Upgrade to strum 0.27.
- Change headers in table files.
- Replace termbg to save size.

### ⚙️ Miscellaneous Tasks

- Release procmon-oprs version 0.36.0

## [0.35.1] - 2025-02-15

### 🐛 Bug Fixes

- Selected line was always erased.
- Correctly scroll horizontally if columns exceed the screen width.

### ⚙️ Miscellaneous Tasks

- Update nom and rand
- Release procmon-oprs version 0.35.1

## [0.35.0] - 2025-02-14

### 🐛 Bug Fixes

- Let the last column of a table fill the remaining space.
- In environment, display the scrollbar only on the value.
- In main pane, Ctrl-C clears the search first.
- Scroll environment table in all directions.
- Indentation of processes when scrolling.
- Go to first match when searching.

### 🚜 Refactor

- Make terminal RefCell to avoid mutable borrow just for it.
- Manage the motion in BigTableWidget.

### ⚙️ Miscellaneous Tasks

- Fix tests and clippy warnings.
- Remove file that shouldn't have been commited
- Release procmon-oprs version 0.35.0

## [0.34.0] - 2025-01-26

### 🚀 Features

- Add a scrollbar in the help
- Add scrollbars to the main table.
- Remove the limits in the process tree
- Display the limits in the process details.
- Display process environment

### 🐛 Bug Fixes

- Fix the help message in interactive mode and the README
- Scrolling in process environment

### 🚜 Refactor

- Add support for different pane for a single process.

### ⚙️ Miscellaneous Tasks

- Fix clippy warnings
- Release procmon-oprs version 0.34.0

## [0.33.0] - 2025-01-12

### 🚀 Features

- Display the process working directory in the details

### 🐛 Bug Fixes

- Scroll the details by blocks
- Scroll down one block at a time in details

### ⚙️ Miscellaneous Tasks

- Release procmon-oprs version 0.33.0

## [0.32.0] - 2025-01-11

### 🚀 Features

- Add option --root to select the root PID.
- Select the root of the tree interactively.

### ⚙️ Miscellaneous Tasks

- Release procmon-oprs version 0.32.0

## [0.31.1] - 2025-01-11

### 🐛 Bug Fixes

- Starting an incremental search select the first item.
- Process crash when displaying limits.

### 🚜 Refactor

- Fix clippy warnings

### ⚙️ Miscellaneous Tasks

- Update itertools and rstest
- Release procmon-oprs version 0.31.1

## [0.31.0] - 2025-01-05

### 🚀 Features

- Always display the system metrics on first line.

### 🐛 Bug Fixes

- Don't allow to select the first line

### ⚙️ Miscellaneous Tasks

- Release procmon-oprs version 0.31.0

## [0.30.0] - 2025-01-04

### 🐛 Bug Fixes

- Typo in help message
- Check if the keymap is correct when displaying the main pane
- Wrong keymap name for details.
- Truncate process name if it is larger than half screen line.

### 🚜 Refactor

- Create custom widgets to manage the layout.
- Fix clippy warnings and minor changes.

### ⚙️ Miscellaneous Tasks

- Changelog for release 0.30.0
- Release procmon-oprs version 0.30.0

## [0.29.0] - 2024-12-22

### 🚀 Features

- Order processes by PID under their parent
- Add option --glob to match shell-like patterns on process names

### 🐛 Bug Fixes

- Incremental search blocked on the wrong keymap

### ⚙️ Miscellaneous Tasks

- Changelog for new release
- Release procmon-oprs version 0.29.0

## [0.28.0] - 2024-12-21

### 🚀 Features

- In details, go to parent process with key 'p'

### ⚙️ Miscellaneous Tasks

- Support for git cliff and cargo release.
- Wrong pre-release replacements.
- Add changelog.
- Release procmon-oprs version 0.28.0

## [r0.6.0] - 2020-06-19

<!-- generated by git-cliff -->
