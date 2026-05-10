use std::{
    env, fs,
    io::{self, Write},
    process,
};

use anyhow::Result;
use strum::IntoEnumIterator;
use termion::{
    clear, cursor,
    event::Key,
    input::TermRead,
    raw::{IntoRawMode, RawTerminal},
};

use crate::command::Builtin;
use crate::pipeline::Pipeline;
use crate::shell::Shell;

const BELL: &str = "\x07";

struct Completion {
    prefix: String,
    matches: Vec<String>,
}

impl Completion {
    fn new(prefix: String, matches: Vec<String>) -> Self {
        Self { prefix, matches }
    }
}

enum ProcessResult {
    Continue(bool),
    Exit,
}

pub struct Terminal {
    /// Input buffer for the command line
    input: Vec<char>,
    /// Current position of the cursor in the input string
    cursor_pos: usize,
    /// Raw terminal output for direct terminal manipulation
    stdout: RawTerminal<io::Stdout>,
    /// Shared shell state (history, jobs)
    shell: Shell,
    /// Current index in the command history
    history_index: usize,
    /// Last command entered before navigating history
    last_input: String,
    /// Tab completion state
    completion: Option<Completion>,
}

impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.shell.history.save();
    }
}

impl Terminal {
    pub fn new() -> Result<Self> {
        let shell = Shell::new();
        let history_len = shell.history.len();
        let term = Self {
            input: Vec::new(),
            cursor_pos: 0,
            stdout: io::stdout().into_raw_mode()?,
            shell,
            history_index: history_len,
            last_input: String::new(),
            completion: None,
        };
        Ok(term)
    }

    pub fn start(&mut self) -> Result<()> {
        loop {
            self.draw_input()?;
            match self.process_input() {
                Ok(ProcessResult::Continue(should_execute)) => {
                    if should_execute {
                        self.run()?;
                    }
                    self.reap_jobs()?;
                }
                Ok(ProcessResult::Exit) => {
                    println!();
                    return Ok(());
                }
                Err(e) => eprintln!("{e}"),
            }
            self.reset_input();
        }
    }

    fn reap_jobs(&mut self) -> Result<()> {
        self.stdout.suspend_raw_mode()?;
        self.shell.jobs.reap(&mut io::stdout())?;
        self.stdout.activate_raw_mode()?;
        Ok(())
    }

    fn reset_input(&mut self) {
        self.input.clear();
        self.cursor_pos = 0;
    }

    fn draw_input(&mut self) -> Result<()> {
        write!(self.stdout, "\r{}$ ", clear::CurrentLine)?;
        for c in &self.input {
            write!(self.stdout, "{c}")?;
        }
        // Reposition cursor to just after the inserted char
        let n = self.input.len().saturating_sub(self.cursor_pos);
        if n > 0 {
            #[allow(clippy::cast_possible_truncation)]
            write!(self.stdout, "{}", cursor::Left(n as u16))?;
        }
        self.stdout.flush()?;
        Ok(())
    }

    fn process_input(&mut self) -> Result<ProcessResult> {
        for key in io::stdin().keys().filter_map(Result::ok) {
            match key {
                Key::Char('\n') => {
                    writeln!(self.stdout, "\r")?;
                    self.append_history();
                    return Ok(ProcessResult::Continue(!self.input.is_empty()));
                }
                Key::Char('\t') => self.handle_tab()?,
                Key::Ctrl('c') => {
                    writeln!(self.stdout, "\r")?;
                    return Ok(ProcessResult::Continue(false));
                }
                Key::Ctrl('d') => {
                    if self.input.is_empty() {
                        self.stdout.suspend_raw_mode()?;
                        return Ok(ProcessResult::Exit);
                    }
                    self.show_completions()?;
                }
                Key::Backspace => self.backspace()?,
                Key::Left => self.move_cursor_left()?,
                Key::Right => self.move_cursor_right()?,
                Key::Up => self.get_previous_command()?,
                Key::Down => self.get_next_command()?,
                Key::Char(c) => self.insert_char(c)?,
                _ => (),
            }
            self.stdout.flush()?;
        }
        Ok(ProcessResult::Continue(true))
    }

    fn backspace(&mut self) -> Result<()> {
        if self.cursor_pos > 0 {
            self.input.remove(self.cursor_pos - 1);
            self.cursor_pos -= 1;
            self.draw_input()
        } else {
            write!(self.stdout, "{BELL}")?;
            Ok(())
        }
    }

    fn move_cursor_left(&mut self) -> Result<()> {
        if self.cursor_pos > 0 {
            self.cursor_pos -= 1;
            write!(self.stdout, "{}", cursor::Left(1))?;
        }
        Ok(())
    }

    fn move_cursor_right(&mut self) -> Result<()> {
        if self.cursor_pos < self.input.len() {
            self.cursor_pos += 1;
            write!(self.stdout, "{}", cursor::Right(1))?;
        }
        Ok(())
    }

    fn append_history(&mut self) {
        let command = self.input.iter().collect::<String>();
        self.shell.history.add(command);
        self.history_index = self.shell.history.len();
    }

    fn get_previous_command(&mut self) -> Result<()> {
        // Can't go back if we're at the beginning of history or history is empty
        let history_len = self.shell.history.len();
        if history_len == 0 || self.history_index == 0 {
            write!(self.stdout, "{BELL}")?;
            return Ok(());
        }
        // Save current input before moving to previous command
        let input = self.input.iter().collect::<String>();
        if self.history_index == history_len {
            self.last_input = input;
        } else {
            self.shell.history.set(self.history_index, input);
        }
        // Move to previous command
        self.history_index -= 1;
        if let Some(cmd) = self.shell.history.get(self.history_index) {
            self.input = cmd.chars().collect();
            self.cursor_pos = self.input.len();
        }
        self.draw_input()
    }

    fn get_next_command(&mut self) -> Result<()> {
        // Check if we're already at or beyond the end of history
        let history_len = self.shell.history.len();
        if self.history_index >= history_len {
            write!(self.stdout, "{BELL}")?;
            return Ok(());
        }
        // Save current input to the history
        self.shell
            .history
            .set(self.history_index, self.input.iter().collect());
        self.history_index += 1;
        // Set input: either from stored_input (if at end) or from history
        if self.history_index == history_len {
            self.input = self.last_input.chars().collect();
        } else if let Some(cmd) = self.shell.history.get(self.history_index) {
            self.input = cmd.chars().collect();
        }
        // Update cursor position and redraw
        self.cursor_pos = self.input.len();
        self.draw_input()
    }

    fn insert_char(&mut self, c: char) -> Result<()> {
        self.input.insert(self.cursor_pos, c);
        self.cursor_pos += 1;
        self.draw_input()
    }

    fn handle_tab(&mut self) -> Result<()> {
        let prefix = self.input[..self.cursor_pos].iter().collect::<String>();
        let comp_line: String = self.input.iter().collect();
        let comp_point: usize = self.input[..self.cursor_pos]
            .iter()
            .map(|c| c.len_utf8())
            .sum();
        let completion_token = prefix.split(' ').next_back().unwrap_or("");
        let matches = match &self.completion {
            Some(c) if c.prefix == prefix => c.matches.clone(),
            _ => {
                if prefix == "." || prefix == ".." {
                    vec![prefix.clone() + "/"]
                } else {
                    self.get_matches(&prefix, &comp_line, comp_point)
                }
            }
        };
        match matches.len() {
            0 => write!(self.stdout, "{BELL}")?,
            1 => {
                // Single match: use full name; trailing / for dirs, trailing space for files
                let mut completion = matches[0].clone();
                if !completion.ends_with('/') {
                    completion += " ";
                }
                self.update_input(&completion)?;
            }
            _ => {
                // Multiple matches: complete to LCP if it extends the current token
                let common_prefix = longest_common_prefix(&matches);
                if common_prefix.len() > completion_token.len() {
                    self.update_input(&common_prefix)?;
                } else {
                    // First TAB: ring bell only.
                    // Second TAB (cached): ring bell and show list.
                    let was_cached = self.completion.as_ref().is_some_and(|s| s.prefix == prefix);
                    self.completion = Some(Completion::new(prefix.clone(), matches.clone()));
                    write!(self.stdout, "{BELL}")?;
                    if was_cached {
                        self.display_matches(&matches)?;
                    }
                }
            }
        }
        Ok(())
    }

    fn get_matches(&self, prefix: &str, comp_line: &str, comp_point: usize) -> Vec<String> {
        let tokens: Vec<&str> = prefix.split(' ').collect();
        let last_token = tokens.last().copied().unwrap_or("");
        if let Some(first) = tokens.first().filter(|_| tokens.len() > 1)
            && let Some(script) = self.shell.completions.get(first)
        {
            let prev = tokens
                .get(tokens.len().saturating_sub(2))
                .copied()
                .unwrap_or("");
            if let Some(lines) =
                run_completer(&script, first, last_token, prev, comp_line, comp_point)
            {
                return lines;
            }
        }
        // Complete as files if there's a path separator or it's not the first word
        if last_token.contains('/') || prefix.contains(' ') {
            find_matching_files(last_token)
        } else {
            find_matching_executables(last_token)
        }
    }

    fn update_input(&mut self, completion: &str) -> Result<()> {
        // Find the start of the current token being completed
        let token_start = self.input[..self.cursor_pos]
            .iter()
            .rposition(|c| c.is_whitespace())
            .map_or(0, |pos| pos + 1);
        // For file paths, we only want to replace after the last slash
        let insertion_start = self.input[token_start..self.cursor_pos]
            .iter()
            .rposition(|&c| c == '/')
            .map_or(token_start, |pos| token_start + pos + 1);
        // Create the replacement text, adding a space for completed words
        let completion_chars: Vec<char> = completion.chars().collect();
        // Replace the partial text with the completion
        self.input
            .splice(insertion_start..self.cursor_pos, completion_chars.clone());
        self.cursor_pos = insertion_start + completion_chars.len();
        // Redraw the input with the completion
        self.draw_input()
    }

    fn display_matches(&mut self, matches: &[String]) -> Result<()> {
        writeln!(self.stdout, "\r")?;
        writeln!(self.stdout, "{}", matches.join("  "))?;
        self.draw_input()
    }

    fn show_completions(&mut self) -> Result<()> {
        let prefix = self.input[..self.cursor_pos].iter().collect::<String>();
        let matches = find_matching_executables(&prefix);
        if matches.is_empty() {
            write!(self.stdout, "{BELL}")?;
            return Ok(());
        }
        self.display_matches(&matches)
    }

    fn run(&self) -> Result<()> {
        self.stdout.suspend_raw_mode()?;
        let input = self.input.iter().collect::<String>();
        match Pipeline::new(&input, self.shell.clone()) {
            Ok(mut pipeline) => pipeline.execute()?,
            Err(e) => {
                eprintln!("{e}");
            }
        }
        self.stdout.activate_raw_mode()?;
        Ok(())
    }
}

fn find_matching_executables(prefix: &str) -> Vec<String> {
    let mut matches = Vec::new();
    // Builtin commands
    matches.extend(
        Builtin::iter()
            .map(|b| b.to_string().to_lowercase())
            .filter(|b| b.starts_with(prefix)),
    );
    // Executables in PATH
    if let Some(path) = env::var_os("PATH") {
        for dir in env::split_paths(&path) {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.filter_map(Result::ok) {
                    if let Some(name) = entry.file_name().to_str()
                        && name.starts_with(prefix)
                    {
                        matches.push(name.to_string());
                    }
                }
            }
        }
    }
    matches.sort();
    matches.dedup();
    matches
}

fn find_matching_files(prefix: &str) -> Vec<String> {
    if prefix == "." || prefix == ".." {
        return vec![format!("{}/", prefix)];
    }

    let mut matches = Vec::new();
    let (dir, stem) = if let Some(pos) = prefix.rfind('/') {
        prefix.split_at(pos + 1)
    } else {
        (".", prefix)
    };
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.filter_map(Result::ok) {
            if let Some(name) = entry.file_name().to_str()
                && name.starts_with(stem)
            {
                let mut candidate = name.to_string();
                // Add trailing slash if it's a directory
                if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                    candidate.push('/');
                }
                matches.push(candidate);
            }
        }
    }
    matches.sort();
    matches
}

fn run_completer(
    script: &str,
    cmd: &str,
    current: &str,
    prev: &str,
    comp_line: &str,
    comp_point: usize,
) -> Option<Vec<String>> {
    let output = process::Command::new(script)
        .args([cmd, current, prev])
        .env("COMP_LINE", comp_line)
        .env("COMP_POINT", comp_point.to_string())
        .output()
        .ok()?;
    let mut lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(String::from)
        .collect();
    lines.sort();
    Some(lines)
}

fn longest_common_prefix(strings: &[String]) -> String {
    if strings.is_empty() {
        return String::new();
    }
    let first = &strings[0];
    let last = &strings[strings.len() - 1];
    first
        .chars()
        .zip(last.chars())
        .take_while(|(a, b)| a == b)
        .map(|(a, _)| a)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcp_common_prefix() {
        let input = vec!["apple".into(), "application".into(), "apply".into()];
        assert_eq!(longest_common_prefix(&input), "appl");
    }

    #[test]
    fn lcp_identical_strings() {
        let input = vec!["same".into(), "same".into()];
        assert_eq!(longest_common_prefix(&input), "same");
    }

    #[test]
    fn lcp_no_common_prefix() {
        let input = vec!["abc".into(), "xyz".into()];
        assert_eq!(longest_common_prefix(&input), "");
    }

    #[test]
    fn lcp_empty_input() {
        let input: Vec<String> = vec![];
        assert_eq!(longest_common_prefix(&input), "");
    }

    #[test]
    fn lcp_single_string() {
        let input = vec!["only".into()];
        assert_eq!(longest_common_prefix(&input), "only");
    }
}
