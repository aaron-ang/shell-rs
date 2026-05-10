use std::{
    env,
    fmt::Display,
    io::{self, Write},
    mem,
    path::PathBuf,
    process, str,
};

use anyhow::Result;
use is_executable::IsExecutable;
use strum::{Display, EnumIter, EnumString};

use crate::shell::Shell;

#[derive(EnumIter, EnumString, Display)]
#[strum(ascii_case_insensitive)]
pub enum Builtin {
    Cd,
    Complete,
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
        let mut args = self.args.iter().map(String::as_str);
        match args.next() {
            None => self.shell.history.print(&mut self.output, None),
            Some("-c") => {
                self.shell.history.clear();
                Ok(())
            }
            Some(opt @ ("-r" | "-w" | "-a")) => {
                let file = args.next().map(PathBuf::from).unwrap_or_default();
                match opt {
                    "-r" => self.shell.history.append_from_file(file),
                    "-w" => self.shell.history.write_to_file(file)?,
                    "-a" => self.shell.history.append_to_file(file)?,
                    _ => unreachable!(),
                }
                Ok(())
            }
            Some(flag) if flag.starts_with('-') => {
                self.print_err(format!("history: {flag}: invalid option"))
            }
            Some(arg) => {
                if let Ok(n) = arg.parse::<usize>() {
                    self.shell.history.print(&mut self.output, Some(n))
                } else {
                    self.print_err(format!("history: {arg}: numeric argument required"))
                }
            }
        }
    }

    fn handle_complete(&mut self) -> Result<()> {
        let mut args = self.args.iter().map(String::as_str);
        match args.next() {
            Some("-C") => {
                if let (Some(path), Some(cmd)) = (args.next(), args.next()) {
                    self.shell
                        .completions
                        .register(cmd.to_string(), path.to_string());
                }
                Ok(())
            }
            Some("-r") => {
                if let Some(cmd) = args.next() {
                    self.shell.completions.remove(cmd);
                }
                Ok(())
            }
            Some("-p") => {
                if let Some(cmd) = args.next() {
                    match self.shell.completions.get(cmd) {
                        Some(path) => self.print_out(format!("complete -C '{path}' {cmd}"))?,
                        None => {
                            self.print_err(format!(
                                "complete: {cmd}: no completion specification"
                            ))?;
                        }
                    }
                }
                Ok(())
            }
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
                full_path.is_executable().then_some(full_path)
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

#[cfg(test)]
mod tests {
    use super::*;
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
}
