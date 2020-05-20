Oprs
====

Oprs is a process monitor for Linux. The name is an abbreviation for Observe Process ReSources.

It's based on [procfs](https://crates.io/crates/procfs).

Features
--------

* Monitor memory, execution time, input/output, page fault.
* Optional minimum and maximum.
* Select processes by PID, PID file or name.
* Display in plain text or a terminal UI.

Basic usage
-----------

To get the memory size, elapsed time and page fault of a process by PID (firefox), a process by pid
file (lvmetad) and a process by name (bash), run:

    oprs --human -p 12813 -f /run/lvmetad.pid -n bash -m mem:vm mem:rss+max time:elapsed fault:major

Without argument, the command prints the available metrics.

License
-------

Copyright (c) 2020 Laurent Pelecq

`oprs` is distributed under the terms of the GNU General Public License version 3.

See the [LICENSE GPL3](LICENSE) for details.
