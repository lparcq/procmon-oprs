Oprs
====

![Rust](https://github.com/lparcq/procmon-oprs/workflows/Rust/badge.svg)
[![Crates.io](https://img.shields.io/crates/v/procmon-oprs.svg)](https://crates.io/crates/procmon-oprs)
![Crates.io](https://img.shields.io/crates/l/procmon-oprs)

Oprs is a process monitor for Linux. The name is an abbreviation for Observe Process ReSources.

It's based on [procfs](https://crates.io/crates/procfs).

Features
--------

* Monitor memory, execution time, input/output, page fault, file descriptors, mapped memory regions.
* Optional minimum and maximum.
* Select processes by PID, PID file or name.
* Display in plain text or a terminal UI.

Example
-------

To get the memory size, elapsed time and page fault of a process by PID (firefox), a process by pid
file (pulseaudio) and a process by name (bash), run:

    oprs -F human -p 6913 -f pulseaudio.pid -n dhclient mem:vm mem:rss+max time:elapsed fault:major

![Screenshot](doc/screenshot.jpeg)

Without argument, the command prints the available metrics.

By default, the raw figure is printed unless -raw is added: mem:rss-raw+min+max. 

Usage
-----

Without argument, the command prints the list of available metrics.

Limited patterns are allowed for metrics: by prefix mem:*, suffix *:call, both io:*:count.

A metric may be followed by a unit. For example: mem:vm/gi

Available units:
- ki  kibi
- mi  mebi
- gi  gibi
- ti  tebi
- k   kilo
- m   mega
- g   giga
- t   tera
- sz  the best unit in k, m, g or t.
- du  format duration as hour, minutes, seconds.

Metrics can be also aggregated using +min and/or +max. For example mem:vm+max/gi prints the virtual
memory size and the peak size. To get only the max, use: mem:vm-raw+max. To get all: mem:vm+min+max.

For some metrics, min or max is meaningless.

Export options:
- csv: comma-separated values, one file per process in the export directory.
- rrd: Round Robin Database.

CPU usage
---------

Unlike other tools, the CPU usage of a process displayed by time:cpu+ratio is the percentage of the
total CPU time. A process using all cores of a 4-cores system would be at 100%, not 400%.

The CPU usage is (stime + utime) / ((user - guest) + (nice - guest_nice) + system + idle + iowait + irq + softirq)
where stime and utime comes from /proc/PID/stat and user, … from /proc/stat.

Export
------

In CSV export, the first column is the number of seconds since the [Unix Epoch](https://en.wikipedia.org/wiki/Unix_time).

[RRDtool](https://oss.oetiker.ch/rrdtool/) creates one file per process. Only raw values are written in the database.
The number of rows is set with option --export-count.

Configuration
-------------

Configuration file name is `settings.ini`. It's located according to
the [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir-spec/latest/).

Example `~/.config/oprs/settings.ini`:

    [display]
    mode = term
    every = 10
    format = human
    theme = light

    [export]
    kind = csv
    dir = /tmp
    size = 10m
    count = 5

    [logging]
    file = /var/log/oprs.log
    level = info

    [targets]
    system = yes
    myself = yes

License
-------

Copyright (c) 2020 Laurent Pelecq

`oprs` is distributed under the terms of the GNU General Public License version 3.

See the [LICENSE GPL3](LICENSE) for details.
