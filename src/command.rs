use std::{
    env,
    fmt::Display,
    io::{self, Write},
    mem,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process, str,
};

use anyhow::Result;
use strum::{Display, EnumIter, EnumString};

use crate::shell::Shell;

#[derive(EnumIter, EnumString, Display)]
#[strum(ascii_case_insensitive)]
pub enum Builtin {
    Cd,
    Complete,
    Declare,
    Echo,
    Exit,
    History,
    Jobs,
    Pwd,
    Type,
}

pub struct Command {
    name: String,
    args: Vec<String>,
    output: Box<dyn Write>,
    err: Box<dyn Write>,
    redirected: bool,
    shell: Shell,
}

impl Command {
    pub fn new(shell: Shell) -> Self {
        Self {
            name: String::new(),
            args: Vec::new(),
            output: Box::new(io::stdout()),
            err: Box::new(io::stderr()),
            redirected: false,
            shell,
        }
    }

    pub fn push_arg(&mut self, current_arg: &str) {
        if self.name.is_empty() {
            self.name = current_arg.to_string();
        } else {
            self.args.push(current_arg.to_string());
        }
    }

    pub fn set_output(&mut self, output: impl Write + 'static) {
        self.output = Box::new(output);
        self.redirected = true;
    }

    pub fn set_err(&mut self, err: impl Write + 'static) {
        self.err = Box::new(err);
        self.redirected = true;
    }

    pub fn is_empty(&self) -> bool {
        self.name.is_empty()
    }

    pub fn is_builtin(&self) -> bool {
        Builtin::try_from(self.name.as_str()).is_ok()
    }

    pub fn pop_background_token(&mut self) -> bool {
        if self.args.last().is_some_and(|a| a == "&") {
            self.args.pop();
            true
        } else {
            false
        }
    }

    pub fn new_process(&self) -> process::Command {
        let mut cmd = process::Command::new(&self.name);
        cmd.args(&self.args);
        cmd
    }

    pub fn execute(&mut self) -> Result<()> {
        match Builtin::try_from(self.name.as_str()) {
            Ok(builtin) => match builtin {
                Builtin::Cd => self.handle_cd(),
                Builtin::Complete => self.handle_complete(),
                Builtin::Declare => self.handle_declare(),
                Builtin::Echo => self.handle_echo(),
                Builtin::Exit => self.handle_exit(),
                Builtin::History => self.handle_history(),
                Builtin::Jobs => self.shell.jobs.print(&mut self.output),
                Builtin::Pwd => self.print_out(env::current_dir()?.display()),
                Builtin::Type => self.handle_type(),
            },
            Err(_) => self.execute_external_command(),
        }
    }

    pub fn execute_to_output(&mut self, out: impl Write + 'static) -> Result<()> {
        let orig_out = mem::replace(&mut self.output, Box::new(out));
        self.redirected = true;
        self.execute()?;
        self.output = orig_out;
        self.redirected = false;
        Ok(())
    }

    fn handle_cd(&mut self) -> Result<()> {
        let target = match self.args.first().map(String::as_str) {
            Some("~") | None => env::var("HOME").unwrap_or_else(|_| "/".to_string()),
            Some(path) => path.to_string(),
        };
        if env::set_current_dir(&target).is_err() {
            self.print_err(format!("cd: {target}: No such file or directory"))?;
        }
        Ok(())
    }

    fn handle_echo(&mut self) -> Result<()> {
        let arg_str = self.args.join(" ");
        self.print_out(arg_str)
    }

    fn handle_exit(&self) -> ! {
        let status = self
            .args
            .first()
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        let _ = self.shell.history.save();
        process::exit(status);
    }

    fn handle_history(&mut self) -> Result<()> {
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        match args.as_slice() {
            [] => self.shell.history.print(&mut self.output, None),
            ["-c"] => {
                self.shell.history.clear();
                Ok(())
            }
            [opt @ ("-r" | "-w" | "-a"), rest @ ..] => {
                let file = rest.first().map(PathBuf::from).unwrap_or_default();
                match *opt {
                    "-r" => self.shell.history.append_from_file(file),
                    "-w" => self.shell.history.write_to_file(file)?,
                    "-a" => self.shell.history.append_to_file(file)?,
                    _ => unreachable!(),
                }
                Ok(())
            }
            [flag, ..] if flag.starts_with('-') => {
                self.print_err(format!("history: {flag}: invalid option"))
            }
            [arg, ..] => {
                if let Ok(n) = arg.parse::<usize>() {
                    self.shell.history.print(&mut self.output, Some(n))
                } else {
                    self.print_err(format!("history: {arg}: numeric argument required"))
                }
            }
        }
    }

    fn handle_declare(&mut self) -> Result<()> {
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        match args.as_slice() {
            ["-p", name] => match self.shell.variables.get(name) {
                Some(value) => self.print_out(format!("declare -- {name}=\"{value}\"")),
                None => self.print_err(format!("declare: {name}: not found")),
            },
            [spec] => {
                let (name, value) = spec.split_once('=').unwrap_or((spec, ""));
                if is_valid_identifier(name) {
                    self.shell
                        .variables
                        .set(name.to_string(), value.to_string());
                    Ok(())
                } else {
                    self.print_err(format!("declare: `{spec}': not a valid identifier"))
                }
            }
            _ => Ok(()),
        }
    }

    fn handle_complete(&mut self) -> Result<()> {
        let args: Vec<&str> = self.args.iter().map(String::as_str).collect();
        match args.as_slice() {
            ["-C", path, cmd] => {
                self.shell
                    .completions
                    .register((*cmd).to_string(), (*path).to_string());
                Ok(())
            }
            ["-r", cmd] => {
                self.shell.completions.remove(cmd);
                Ok(())
            }
            ["-p", cmd] => match self.shell.completions.get(cmd) {
                Some(path) => self.print_out(format!("complete -C '{path}' {cmd}")),
                None => self.print_err(format!("complete: {cmd}: no completion specification")),
            },
            _ => Ok(()),
        }
    }

    fn handle_type(&mut self) -> Result<()> {
        if let Some(cmd) = self.args.first() {
            match Builtin::try_from(cmd.as_str()) {
                Ok(_) => self.print_out(format!("{cmd} is a shell builtin"))?,
                Err(_) => {
                    if let Some(path) = Self::full_path(cmd) {
                        self.print_out(format!("{cmd} is {}", path.display()))?;
                    } else {
                        self.print_out(format!("{cmd}: not found"))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn execute_external_command(&mut self) -> Result<()> {
        if self.exists() {
            let mut cmd = process::Command::new(&self.name);
            cmd.args(&self.args);
            if self.redirected {
                match cmd.output() {
                    Ok(output) => {
                        self.output.write_all(&output.stdout)?;
                        self.err.write_all(&output.stderr)?;
                        Ok(())
                    }
                    Err(e) => self.print_err(e),
                }
            } else {
                match cmd.status() {
                    Ok(_) => Ok(()),
                    Err(e) => self.print_err(e),
                }
            }
        } else {
            self.print_err(format!("{}: command not found", self.name))
        }
    }

    fn full_path(cmd: &str) -> Option<PathBuf> {
        env::var("PATH").ok().and_then(|path_str| {
            env::split_paths(&path_str).find_map(|path| {
                let full_path = path.join(cmd);
                is_executable(&full_path).then_some(full_path)
            })
        })
    }

    fn exists(&self) -> bool {
        Self::full_path(&self.name).is_some()
    }

    fn print_out(&mut self, msg: impl Display) -> Result<()> {
        writeln!(self.output, "{msg}")?;
        Ok(())
    }

    fn print_err(&mut self, msg: impl Display) -> Result<()> {
        writeln!(self.err, "{msg}")?;
        Ok(())
    }
}

/// User|group|other execute bits (POSIX `S_IXUSR | S_IXGRP | S_IXOTH`).
const ANY_EXEC: u32 = 0o111;

fn is_executable(path: &Path) -> bool {
    path.metadata()
        .is_ok_and(|m| m.is_file() && m.permissions().mode() & ANY_EXEC != 0)
}

fn is_valid_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {
            chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn cmd(args: &[&str]) -> Command {
        let mut c = Command::new(Shell::new());
        for a in args {
            c.push_arg(a);
        }
        c
    }

    #[test]
    fn push_arg_sets_name_then_args() {
        let c = cmd(&["echo", "hello", "world"]);
        assert_eq!(c.name, "echo");
        assert_eq!(c.args, &["hello", "world"]);
    }

    #[test]
    fn is_empty_when_no_args() {
        let c = Command::new(Shell::new());
        assert!(c.is_empty());
    }

    #[test]
    fn is_builtin_matches() {
        let c = cmd(&["echo"]);
        assert!(c.is_builtin());
    }

    #[test]
    fn is_builtin_rejects_unknown() {
        let c = cmd(&["foobar"]);
        assert!(!c.is_builtin());
    }

    #[test]
    fn pop_background_token_removes_trailing_ampersand() {
        let mut c = cmd(&["sleep", "500", "&"]);
        assert!(c.pop_background_token());
        assert_eq!(c.name, "sleep");
        assert_eq!(c.args, &["500"]);
    }

    #[test]
    fn pop_background_token_no_ampersand() {
        let mut c = cmd(&["sleep", "500"]);
        assert!(!c.pop_background_token());
        assert_eq!(c.args, &["500"]);
    }

    #[test]
    fn pop_background_token_ampersand_not_last() {
        let mut c = cmd(&["echo", "&", "hello"]);
        assert!(!c.pop_background_token());
        assert_eq!(c.args, &["&", "hello"]);
    }

    #[derive(Clone, Default)]
    struct CaptureBuf(Arc<Mutex<Vec<u8>>>);
    impl Write for CaptureBuf {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl CaptureBuf {
        fn as_string(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    fn complete_cmd(shell: Shell, args: &[&str]) -> (Command, CaptureBuf, CaptureBuf) {
        let mut c = Command::new(shell);
        c.push_arg("complete");
        for a in args {
            c.push_arg(a);
        }
        let out = CaptureBuf::default();
        let err = CaptureBuf::default();
        c.set_output(out.clone());
        c.set_err(err.clone());
        (c, out, err)
    }

    #[test]
    fn complete_p_missing_prints_error_to_stderr() {
        let (mut c, out, err) = complete_cmd(Shell::new(), &["-p", "git"]);
        c.execute().unwrap();
        assert_eq!(out.as_string(), "");
        assert_eq!(
            err.as_string(),
            "complete: git: no completion specification\n"
        );
    }

    #[test]
    fn complete_c_then_p_prints_registered_spec() {
        let shell = Shell::new();
        let (mut reg, _, _) = complete_cmd(shell.clone(), &["-C", "/path/to/git", "git"]);
        reg.execute().unwrap();
        let (mut q, out, err) = complete_cmd(shell, &["-p", "git"]);
        q.execute().unwrap();
        assert_eq!(out.as_string(), "complete -C '/path/to/git' git\n");
        assert_eq!(err.as_string(), "");
    }

    #[test]
    fn complete_r_removes_registration() {
        let shell = Shell::new();
        shell.completions.register("git".into(), "/path".into());
        let (mut rm, _, _) = complete_cmd(shell.clone(), &["-r", "git"]);
        rm.execute().unwrap();
        assert_eq!(shell.completions.get("git"), None);
    }

    #[test]
    fn complete_r_absent_is_silent() {
        let shell = Shell::new();
        let (mut rm, out, err) = complete_cmd(shell, &["-r", "git"]);
        rm.execute().unwrap();
        assert_eq!(out.as_string(), "");
        assert_eq!(err.as_string(), "");
    }

    fn declare_cmd(shell: Shell, args: &[&str]) -> (Command, CaptureBuf, CaptureBuf) {
        let mut c = Command::new(shell);
        c.push_arg("declare");
        for a in args {
            c.push_arg(a);
        }
        let out = CaptureBuf::default();
        let err = CaptureBuf::default();
        c.set_output(out.clone());
        c.set_err(err.clone());
        (c, out, err)
    }

    #[test]
    fn declare_p_missing_prints_error_to_stderr() {
        let (mut c, out, err) = declare_cmd(Shell::new(), &["-p", "missing"]);
        c.execute().unwrap();
        assert_eq!(out.as_string(), "");
        assert_eq!(err.as_string(), "declare: missing: not found\n");
    }

    #[test]
    fn declare_set_then_p_prints_value() {
        let shell = Shell::new();
        let (mut set, _, _) = declare_cmd(shell.clone(), &["foo=bar"]);
        set.execute().unwrap();
        let (mut q, out, err) = declare_cmd(shell, &["-p", "foo"]);
        q.execute().unwrap();
        assert_eq!(out.as_string(), "declare -- foo=\"bar\"\n");
        assert_eq!(err.as_string(), "");
    }

    #[test]
    fn declare_rejects_digit_start_identifier() {
        let shell = Shell::new();
        let (mut c, out, err) = declare_cmd(shell.clone(), &["23=x"]);
        c.execute().unwrap();
        assert_eq!(out.as_string(), "");
        assert_eq!(err.as_string(), "declare: `23=x': not a valid identifier\n");
        assert_eq!(shell.variables.get("23"), None);
    }

    #[test]
    fn declare_accepts_underscore_start_identifier() {
        let shell = Shell::new();
        let (mut set, _, _) = declare_cmd(shell.clone(), &["_FOO=bar"]);
        set.execute().unwrap();
        let (mut q, out, _) = declare_cmd(shell, &["-p", "_FOO"]);
        q.execute().unwrap();
        assert_eq!(out.as_string(), "declare -- _FOO=\"bar\"\n");
    }

    #[test]
    fn is_valid_identifier_rules() {
        assert!(is_valid_identifier("foo"));
        assert!(is_valid_identifier("_foo"));
        assert!(is_valid_identifier("FOO_BAR_2"));
        assert!(!is_valid_identifier(""));
        assert!(!is_valid_identifier("2foo"));
        assert!(!is_valid_identifier("foo-bar"));
        assert!(!is_valid_identifier("foo.bar"));
    }

    #[test]
    fn declare_overwrites_existing_value() {
        let shell = Shell::new();
        for v in ["foo=bar", "foo=bar2"] {
            let (mut c, _, _) = declare_cmd(shell.clone(), &[v]);
            c.execute().unwrap();
        }
        let (mut q, out, _) = declare_cmd(shell, &["-p", "foo"]);
        q.execute().unwrap();
        assert_eq!(out.as_string(), "declare -- foo=\"bar2\"\n");
    }

    #[test]
    fn type_declare_is_shell_builtin() {
        let mut c = Command::new(Shell::new());
        c.push_arg("type");
        c.push_arg("declare");
        let out = CaptureBuf::default();
        c.set_output(out.clone());
        c.execute().unwrap();
        assert_eq!(out.as_string(), "declare is a shell builtin\n");
    }
}
