# Help

Keys F1 to F7 opens sub-menus. The action in the sub-menus are most of them time accessible directly as shortcuts from the main menu. The menu is more a quick reminder. When a key is only accessible from its menu, it is followed by a dagger †.

Revert to the parent menu with the escape key.

The main screen displays the list of processes with the metrics specified in the command line. The states are

- (R) Running
- (S) Sleeping in an interruptible wait
- (D) Waiting in uninterruptible disk sleep
- (Z) Zombie
- (T) Stopped (on a signal)
- (t) Tracing stop
- (X) Dead
- (K) Wakekill
- (W) Waking
- (P) Parked
- (I) Idle

## F1: Help menu

- This page can be opened with '?'.
- The about dialog is opened with 'a'†.
- Quit the application with 'q'.

## F2: Edition

The refresh rate is displayed in the status bar. It can be accelerated with '+' and slowed down with '-'.

## F3: Navigate

- Up and down: move the cursor up and down.
- Page up and down: scroll the cursor by pages.
- Control-Home: go to first line.
- Control-End: go to last line.
- Shift-Tab and Tab: scroll the columns left or right.
- Home: go to first column.
- End: go to last column.

## F4: Searching

- Start an incremental search with '/'.
  . Hit enter to validate the search string.
  . Hit Ctrl-c to clear the search.
  . Use Ctrl-n and Ctrl-N to select the next or previous match.
  . Enter more characters to refine the search.
- Once the search pattern has been validated, move to the next match with 'n'
  and the previous match with 'N'.
- Hit Ctrl-c to clear the search.

## F5: Selection

### Highlighted line

One line is highlighted when hitting the up or down arrows.

Go to the parent process with 'p' in the main menu.

Hit 'Enter' to see the details. See section "Details" below.

### Marking

The space bar toggles the mark on:
1. the matched lines if there is a search,
2. the line under the cursor otherwise.

When no search is enabled, move to the next and previous match with 'n' and 'N'. Otherwise the matches from the search have precedence above the marks.

Hit Ctrl-c to clear the marks.

### Scope

The list of processes can be narrowed by marking them and hitting 's'. The processes are displayed as a flat list.

Hitting 's' again reverts to the tree mode.

The root process of the tree can be defined with 'r' on the selected process. Revert to the full tree with 'R'. The program can be started at a given root with option '--root' on the command line.

### Details

This pane displays informations about a process. Other details are accessible with the following keys.

- 'e': environment variables.
- 'f': opened file descriptors.
- 'l': process limits.
- 'm': memory maps.

## F6: Filters

- none: show userland and kernel processes
- user: show only userland processes (default)
- active: show userland processes that have consumed some CPU in the last 5 cycles.
- myself: show processes owned by the current user.
