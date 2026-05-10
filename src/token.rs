use std::{
    io::{self, Write},
    iter, mem,
    path::PathBuf,
    str,
};

use anyhow::{Result, anyhow};

pub enum Token {
    Arg(String),
    Pipe,
    Redirect {
        type_: RedirectType,
        path: PathBuf,
        append: bool,
    },
}

pub enum RedirectType {
    Stdout,
    Stderr,
    Both,
}

pub fn tokenize(input: &str) -> Result<Vec<Token>> {
    let mut input_char_iter = input.trim().chars().peekable();
    let mut tokens = Vec::new();
    let mut cur_token = String::new();

    while let Some(&ch) = input_char_iter.peek() {
        match ch {
            '\'' | '"' => {
                input_char_iter.next();
                let (mut quoted, closed) = parse_quoted_string(&mut input_char_iter, ch);
                if !closed {
                    collect_additional_quoted(&mut quoted, ch)?;
                }
                cur_token.push_str(&quoted);
            }
            '\\' => {
                input_char_iter.next();
                if let Some(escaped) = input_char_iter.next() {
                    cur_token.push(escaped);
                }
            }
            ' ' => {
                input_char_iter.next();
                if !cur_token.is_empty() {
                    tokens.push(Token::Arg(mem::take(&mut cur_token)));
                }
            }
            '>' => {
                input_char_iter.next();
                let redirect_token = handle_redirection(&mut input_char_iter, &cur_token)?;
                tokens.push(redirect_token);
                cur_token.clear();
            }
            '|' => {
                input_char_iter.next();
                if !cur_token.is_empty() {
                    tokens.push(Token::Arg(mem::take(&mut cur_token)));
                }
                tokens.push(Token::Pipe);
                if input_char_iter.peek().is_none() {
                    cur_token.push_str(&collect_next_arg()?);
                }
            }
            _ => cur_token.push(input_char_iter.next().unwrap()),
        }
    }

    if !cur_token.is_empty() {
        tokens.push(Token::Arg(cur_token));
    }

    Ok(tokens)
}

fn parse_quoted_string(iter: &mut impl Iterator<Item = char>, quote: char) -> (String, bool) {
    let mut chunk = String::new();
    while let Some(mut ch) = iter.next() {
        if ch == quote {
            return (chunk, true);
        }
        if ch == '\\'
            && quote == '"'
            && let Some(next_ch) = iter.next()
        {
            if !matches!(next_ch, '$' | '`' | '"' | '\\' | '\n') {
                chunk.push('\\');
            }
            ch = next_ch;
        }
        chunk.push(ch);
    }
    (chunk, false)
}

fn collect_additional_quoted(arg: &mut String, quote: char) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let prefix = if quote == '\'' { "d" } else { "" };
    let mut line = String::new();
    loop {
        print!("{prefix}quote> ");
        stdout.flush()?;
        stdin.read_line(&mut line)?;
        let (chunk, closed) = parse_quoted_string(&mut line.trim().chars().peekable(), quote);
        arg.push_str(&chunk);
        if closed {
            break;
        }
        line.clear();
    }
    Ok(())
}

fn collect_next_arg() -> Result<String> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut line = String::new();
    loop {
        print!("> ");
        stdout.flush()?;
        stdin.read_line(&mut line)?;
        let arg = line.trim();
        if arg.is_empty() {
            line.clear();
        } else {
            return Ok(arg.to_string());
        }
    }
}

fn handle_redirection(iter: &mut iter::Peekable<str::Chars>, fd_arg: &str) -> Result<Token> {
    // Check if we have >> (append) or just > (overwrite)
    let append = matches!(iter.peek(), Some('>'));
    if append {
        iter.next(); // consume second '>'
    }
    let path: PathBuf = iter
        .by_ref()
        .skip_while(|c| c.is_whitespace())
        .collect::<String>()
        .into();
    if path.as_os_str().is_empty() {
        return Err(anyhow!("No file specified for redirection"));
    }
    let redirect_type = match fd_arg {
        "2" => RedirectType::Stderr, // stderr
        "&" => RedirectType::Both,   // both stdout and stderr
        _ => RedirectType::Stdout,
    };
    Ok(Token::Redirect {
        type_: redirect_type,
        path,
        append,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arg_values(tokens: &[Token]) -> Vec<&str> {
        tokens
            .iter()
            .filter_map(|t| match t {
                Token::Arg(s) => Some(s.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn simple_args() {
        let tokens = tokenize("echo hello world").unwrap();
        assert_eq!(arg_values(&tokens), vec!["echo", "hello", "world"]);
    }

    #[test]
    fn empty_input() {
        let tokens = tokenize("").unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn whitespace_only() {
        let tokens = tokenize("   ").unwrap();
        assert!(tokens.is_empty());
    }

    #[test]
    fn single_quoted_string() {
        let tokens = tokenize("echo 'hello world'").unwrap();
        assert_eq!(arg_values(&tokens), vec!["echo", "hello world"]);
    }

    #[test]
    fn double_quoted_string() {
        let tokens = tokenize(r#"echo "hello world""#).unwrap();
        assert_eq!(arg_values(&tokens), vec!["echo", "hello world"]);
    }

    #[test]
    fn backslash_escape() {
        let tokens = tokenize(r"echo hello\ world").unwrap();
        assert_eq!(arg_values(&tokens), vec!["echo", "hello world"]);
    }

    #[test]
    fn pipe_token() {
        let tokens = tokenize("echo hi | cat").unwrap();
        assert_eq!(tokens.len(), 4);
        assert!(matches!(tokens[2], Token::Pipe));
        assert_eq!(arg_values(&tokens), vec!["echo", "hi", "cat"]);
    }

    #[test]
    fn redirect_stdout() {
        let tokens = tokenize("echo hi > out.txt").unwrap();
        assert_eq!(arg_values(&tokens), vec!["echo", "hi"]);
        assert!(matches!(
            &tokens[2],
            Token::Redirect {
                type_: RedirectType::Stdout,
                append: false,
                ..
            }
        ));
        if let Token::Redirect { path, .. } = &tokens[2] {
            assert_eq!(path, &PathBuf::from("out.txt"));
        }
    }

    #[test]
    fn redirect_append() {
        let tokens = tokenize("echo hi >> out.txt").unwrap();
        assert!(matches!(
            &tokens[2],
            Token::Redirect {
                type_: RedirectType::Stdout,
                append: true,
                ..
            }
        ));
    }

    #[test]
    fn redirect_stderr() {
        let tokens = tokenize("cmd 2> err.txt").unwrap();
        assert!(matches!(
            &tokens[1],
            Token::Redirect {
                type_: RedirectType::Stderr,
                append: false,
                ..
            }
        ));
    }

    #[test]
    fn redirect_both() {
        let tokens = tokenize("cmd &> all.txt").unwrap();
        assert!(matches!(
            &tokens[1],
            Token::Redirect {
                type_: RedirectType::Both,
                ..
            }
        ));
    }

    #[test]
    fn multiple_spaces_between_args() {
        let tokens = tokenize("echo   hello   world").unwrap();
        assert_eq!(arg_values(&tokens), vec!["echo", "hello", "world"]);
    }

    #[test]
    fn adjacent_quotes() {
        let tokens = tokenize(r#"echo "hello"'world'"#).unwrap();
        assert_eq!(arg_values(&tokens), vec!["echo", "helloworld"]);
    }

    #[test]
    fn double_quote_preserves_backslash_for_non_special() {
        let tokens = tokenize(r#"echo "hello\nworld""#).unwrap();
        assert_eq!(arg_values(&tokens), vec!["echo", r"hello\nworld"]);
    }

    #[test]
    fn double_quote_escapes_special_chars() {
        let tokens = tokenize(r#"echo "hello\\world""#).unwrap();
        assert_eq!(arg_values(&tokens), vec!["echo", r"hello\world"]);
    }

    #[test]
    fn parse_quoted_string_single() {
        let input = "hello world'rest";
        let (result, closed) = parse_quoted_string(&mut input.chars(), '\'');
        assert!(closed);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn parse_quoted_string_unclosed() {
        let input = "hello world";
        let (result, closed) = parse_quoted_string(&mut input.chars(), '\'');
        assert!(!closed);
        assert_eq!(result, "hello world");
    }
}
