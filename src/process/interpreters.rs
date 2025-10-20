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

/// Path basename
fn basename<S: AsRef<str>>(path: S) -> String {
    let path = path.as_ref();
    match path.rsplit_once('/') {
        Some((_, name)) => name.to_string(),
        None => path.to_string(),
    }
}

/// Get information on program that execute code.
///
/// Programs like java, perl and python execute files with a known extension or
/// as modules.
struct Interpreter {
    name: String,
    extension: &'static str,
    mod_option: Option<&'static str>,
}

impl Interpreter {
    fn new(name: String, extension: &'static str, mod_option: Option<&'static str>) -> Interpreter {
        Interpreter {
            name,
            mod_option,
            extension,
        }
    }

    pub fn guess(path: &str) -> Option<Interpreter> {
        let name = basename(path);
        if name == "java" {
            Some(Interpreter::new(name, ".jar", Some("-jar")))
        } else if name.starts_with("perl") {
            Some(Interpreter::new(name, ".pl", Some("-M")))
        } else if name.starts_with("python") {
            Some(Interpreter::new(name, ".py", Some("-m")))
        } else if name.ends_with("sh") {
            Some(Interpreter::new(name, ".sh", None))
        } else {
            None
        }
    }

    fn friendly_name(&self, cmdline: &[String]) -> Option<String> {
        let prog_name = self.name.as_str();
        let mut next_arg = false;
        for arg in cmdline.iter().skip(1) {
            match arg.strip_suffix(self.extension) {
                Some(path) => {
                    let name = basename(path);
                    return Some(format!("{prog_name}({name})"));
                }
                None if next_arg => return Some(format!("{prog_name}({arg})")),
                None => {
                    if let Some(option) = self.mod_option {
                        match arg.strip_prefix(option) {
                            Some("") => next_arg = true,
                            Some(name) => return Some(format!("{prog_name}({name})")),
                            None => (),
                        }
                    }
                }
            }
        }
        Some(prog_name.to_string())
    }
}

pub fn friendly_name(cmdline: &[String]) -> Option<String> {
    cmdline.first().and_then(|prog_name| {
        Interpreter::guess(prog_name).and_then(|intr| intr.friendly_name(cmdline))
    })
}

#[cfg(test)]
mod tests {

    use super::friendly_name;

    macro_rules! strings {
        ($($x:expr),*) => (&[$($x.to_string()),*]);
    }

    macro_rules! assert_eq_some_string {
        ($expected:expr, $result:expr) => {
            assert_eq!(Some($expected.to_string()), $result)
        };
    }

    #[test]
    fn test_no_interpreter() {
        assert!(friendly_name(strings!["/bin/head", "-1", "file.txt"]).is_none());
    }

    #[test]
    fn test_java_interpreter() {
        let prog = "/path/to/prog.jar";

        // Name for interpreter without script
        let r1 = friendly_name(strings!["/usr/local/bin/java", "-V"]);
        assert_eq_some_string!("java", r1);

        // Name from jar alone
        let r2 = friendly_name(strings!["/usr/bin/java", "-jar", prog]);
        assert_eq_some_string!("java(prog)", r2);

        // Name from jar with arguments
        let r3 = friendly_name(strings!["/bin/java", "-Dx=y", "-jar", prog, "arg"]);
        assert_eq_some_string!("java(prog)", r3);
    }

    #[test]
    fn test_perl_interpreter() {
        let prog = "/path/to/prog.pl";

        // Name from exe if there is no command line
        let r1 = friendly_name(strings!["/usr/local/bin/perl"]);
        assert_eq_some_string!("perl", r1);

        // Name from script by extension
        let r2 = friendly_name(strings!["/usr/bin/perl", prog]);
        assert_eq_some_string!("perl(prog)", r2);

        // Name for module
        let r3 = friendly_name(strings!["/bin/perl", "-Dtls", prog, "arg"]);
        assert_eq_some_string!("perl(prog)", r3);
    }

    #[test]
    fn test_python_interpreter() {
        let prog = "/path/to/prog.py";

        // Name from exe if there is no command line.
        let r1 = friendly_name(strings!["/usr/local/bin/python", "-h"]);
        assert_eq_some_string!("python", r1);

        // Name from script by extension
        let r2 = friendly_name(strings!["/usr/bin/python", "-v", prog]);
        assert_eq_some_string!("python(prog)", r2);

        // Name for python with a module
        let r3 = friendly_name(strings!["/bin/python", "-m", "http.server", "arg"]);
        assert_eq_some_string!("python(http.server)", r3);
    }
}
