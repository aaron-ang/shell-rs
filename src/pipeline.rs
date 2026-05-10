use std::{fs, path::PathBuf};

use anyhow::{Result, anyhow};
use os_pipe::pipe;

use crate::command::Command;
use crate::shell::Shell;
use crate::token::{RedirectType, Token, tokenize};

pub struct Pipeline {
    commands: Vec<Command>,
    shell: Shell,
    background: bool,
    input: String,
}

impl Pipeline {
    pub fn new(input: &str, shell: Shell) -> Result<Self> {
        let mut pipeline = Self {
            commands: Vec::new(),
            shell,
            background: false,
            input: input.to_string(),
        };
        let tokens = tokenize(input)?;
        pipeline.parse_tokens(tokens)?;
        Ok(pipeline)
    }

    fn parse_tokens(&mut self, tokens: Vec<Token>) -> Result<()> {
        let mut cmd = Command::new(self.shell.clone());
        for token in tokens {
            match token {
                Token::Arg(arg) => cmd.push_arg(&arg),
                Token::Pipe => {
                    if cmd.is_empty() {
                        return Err(anyhow!("Empty command before pipe"));
                    }
                    self.commands.push(cmd);
                    cmd = Command::new(self.shell.clone());
                }
                Token::Redirect {
                    type_,
                    path,
                    append,
                } => {
                    let redirect_file = create_file(&path, append)?;
                    match type_ {
                        RedirectType::Stdout => cmd.set_output(redirect_file),
                        RedirectType::Stderr => cmd.set_err(redirect_file),
                        RedirectType::Both => {
                            let err = redirect_file.try_clone()?;
                            cmd.set_output(redirect_file);
                            cmd.set_err(err);
                        }
                    }
                }
            }
        }
        if cmd.pop_background_token() {
            self.background = true;
        }
        if !cmd.is_empty() {
            self.commands.push(cmd);
        }
        Ok(())
    }

    pub fn execute(&mut self) -> Result<()> {
        if self.commands.is_empty() {
            return Ok(());
        }
        if self.background {
            return self.execute_background();
        }
        if self.commands.len() == 1 {
            return self.commands[0].execute();
        }

        let last_idx = self.commands.len() - 1;
        let mut children = Vec::new();
        let mut prev_pipe = None;

        for (i, cmd) in self.commands.iter_mut().enumerate() {
            let is_last = i == last_idx;
            // Prepare a pipe for this stage if not last
            let (next_reader, next_writer) = if is_last {
                (None, None)
            } else {
                let (r, w) = pipe()?;
                (Some(r), Some(w))
            };
            if cmd.is_builtin() {
                // Builtin: execute directly, redirect stdout if needed
                if let Some(out) = next_writer {
                    // temporarily swap output to pipe
                    cmd.execute_to_output(out)?;
                } else {
                    // last builtin
                    cmd.execute()?;
                }
            } else {
                // External: spawn child process
                let mut p = cmd.new_process();
                if let Some(reader) = prev_pipe.take() {
                    p.stdin(reader);
                }
                if let Some(out) = next_writer {
                    p.stdout(out);
                }
                children.push(p.spawn()?);
            }
            prev_pipe = next_reader;
        }

        for mut child in children {
            child.wait()?;
        }

        Ok(())
    }

    fn execute_background(&mut self) -> Result<()> {
        if let Some(cmd) = self.commands.first() {
            let child = cmd.new_process().spawn()?;
            let pid = child.id();
            let command = self.input.trim().trim_end_matches('&').trim().to_string();
            let number = self.shell.jobs.add(child, command);
            println!("[{number}] {pid}");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pipeline(input: &str) -> Pipeline {
        Pipeline::new(input, Shell::new()).unwrap()
    }

    #[test]
    fn single_command() {
        let p = pipeline("echo hello world");
        assert_eq!(p.commands.len(), 1);
        assert!(!p.background);
    }

    #[test]
    fn background_flag_set() {
        let p = pipeline("sleep 500 &");
        assert!(p.background);
        assert_eq!(p.commands.len(), 1);
    }

    #[test]
    fn ampersand_not_last_is_not_background() {
        let p = pipeline("echo & hello");
        assert!(!p.background);
    }

    #[test]
    fn pipe_creates_multiple_commands() {
        let p = pipeline("echo hi | cat");
        assert_eq!(p.commands.len(), 2);
        assert!(!p.background);
    }

    #[test]
    fn empty_input() {
        let p = pipeline("");
        assert_eq!(p.commands.len(), 0);
        assert!(!p.background);
    }

    #[test]
    fn bare_ampersand_only() {
        let p = pipeline("&");
        assert!(!p.background);
        assert_eq!(p.commands.len(), 1);
    }
}

fn create_file(path: &PathBuf, append: bool) -> Result<fs::File> {
    let mut opts = fs::OpenOptions::new();
    opts.write(true)
        .create(true)
        .truncate(!append)
        .append(append);
    Ok(opts.open(path)?)
}
