# Command help

## Movements

- Up and down: move the cursor up and down.
- Page up and down: scroll the cursor by pages.
- Control-Home: go to first line.
- Control-End: go to last line.
- Left and Right: move the columns left or right.
- Home: go to first column.
- End: go to last column.

## Searching

- Start an incremental search with '/'.
  . Hit enter to validate the search string.
  . Hit Ctrl-c to clear the search.
- Move to the next match with 'n' and the previous match with 'N'.
- Move the cursor to clear the search.

## Marking

The cursor appears when hitting the up or down arrows.

The space bar toggles the mark on:
1. the matched lines if there is a search,
2. the line under the cursor otherwise.

When no search is enabled, move to the next and previous match with 'n' and 'N'.

Hit Ctrl-c to clear the marks.

## Scope

The list of processes can be narrowed by marking them and hitting 's'. The processes
are displayed as a flat list.

Hitting 's' again reverts to the tree mode.

The root process of the tree can be defined with 'r' on the selected process. Revert
to the full tree with 'R'. The command can be started at a given root with option
'--root' on the command line.

## Filters

- none: show userland and kernel processes
- user: show only userland processes (default)
- active: show userland processes that have consumed some CPU in the last 5 cycles.

## State

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
