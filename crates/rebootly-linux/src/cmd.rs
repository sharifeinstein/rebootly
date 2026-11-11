//! Running the system tools (lsblk, mount, cryptsetup, dislocker) behind a
//! trait, so the unlock and mount logic is tested with a fake.
//!
//! Arguments are never put in error messages: dislocker takes the recovery
//! key as an argument.

use std::io::Write;
use std::process::{Command, Stdio};

/// What a finished command left behind.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Output {
    /// Exit code; `None` when killed by a signal.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == Some(0)
    }
}

pub trait Runner: Send + Sync {
    /// Run `prog args…`, writing `stdin` to it (then closing it). An `Err`
    /// means the program couldn't be started (usually: not installed).
    fn run(&self, prog: &str, args: &[&str], stdin: Option<&[u8]>) -> std::io::Result<Output>;

    /// The program is installed (on `PATH`).
    fn has(&self, prog: &str) -> bool;
}

/// The real system.
#[derive(Debug, Default, Clone, Copy)]
pub struct System;

impl Runner for System {
    fn run(&self, prog: &str, args: &[&str], stdin: Option<&[u8]>) -> std::io::Result<Output> {
        let mut child = Command::new(prog)
            .args(args)
            .env("LC_ALL", "C") // English messages, parsed in a few places
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        if let (Some(data), Some(mut pipe)) = (stdin, child.stdin.take()) {
            pipe.write_all(data)?;
        } // pipe dropped here: the program sees end of input
        let out = child.wait_with_output()?;
        Ok(Output {
            code: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }

    fn has(&self, prog: &str) -> bool {
        let Some(path) = std::env::var_os("PATH") else {
            return false;
        };
        std::env::split_paths(&path)
            .chain(["/sbin".into(), "/usr/sbin".into()])
            .any(|d| d.join(prog).is_file())
    }
}

/// The last non-empty line of a tool's error output, for messages.
pub fn last_line(s: &str) -> String {
    s.lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_owned()
}

#[cfg(test)]
pub mod fake {
    //! A scripted runner: each call is matched against the expectations in
    //! order; the calls are recorded for checks.

    use std::sync::Mutex;

    use super::{Output, Runner};

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Call {
        pub prog: String,
        pub args: Vec<String>,
        pub stdin: Option<Vec<u8>>,
    }

    impl Call {
        pub fn line(&self) -> String {
            format!("{} {}", self.prog, self.args.join(" "))
        }
    }

    #[derive(Default)]
    pub struct Fake {
        /// (prefix of `prog args…`, exit code, stdout, stderr)
        pub script: Mutex<Vec<(String, i32, String, String)>>,
        pub calls: Mutex<Vec<Call>>,
        pub missing: Vec<&'static str>,
    }

    impl Fake {
        pub fn expect(&self, prefix: &str, code: i32, stdout: &str, stderr: &str) -> &Self {
            self.script.lock().unwrap().push((
                prefix.into(),
                code,
                stdout.into(),
                stderr.into(),
            ));
            self
        }

        pub fn lines(&self) -> Vec<String> {
            self.calls.lock().unwrap().iter().map(Call::line).collect()
        }
    }

    impl Runner for Fake {
        fn run(
            &self,
            prog: &str,
            args: &[&str],
            stdin: Option<&[u8]>,
        ) -> std::io::Result<Output> {
            let call = Call {
                prog: prog.into(),
                args: args.iter().map(|s| s.to_string()).collect(),
                stdin: stdin.map(<[u8]>::to_vec),
            };
            let line = call.line();
            self.calls.lock().unwrap().push(call);
            let mut script = self.script.lock().unwrap();
            let i = script
                .iter()
                .position(|(p, ..)| line.starts_with(p.as_str()))
                .unwrap_or_else(|| panic!("unexpected command: {line}"));
            let (_, code, stdout, stderr) = script.remove(i);
            Ok(Output {
                code: Some(code),
                stdout,
                stderr,
            })
        }

        fn has(&self, prog: &str) -> bool {
            !self.missing.contains(&prog)
        }
    }
}
