# shell-rs

A Unix-like shell implementation in Rust, built for the [Build Your Own Shell](https://app.codecrafters.io/courses/shell/overview) challenge on Codecrafters.

[![progress-banner](https://backend.codecrafters.io/progress/shell/ebc85b9a-41a5-43f1-a04e-0b5dac9119a6)](https://app.codecrafters.io/users/codecrafters-bot?r=2qF)

---

## Features

| Area            | Capabilities                                                                                       |
| --------------- | -------------------------------------------------------------------------------------------------- |
| **Commands**    | Built-ins (`cd`, `echo`, `exit`, `history`, `pwd`, `type`) and external programs from `PATH`       |
| **Editing**     | Arrow keys (←/→), backspace, insert at cursor; full line redraw keeps display in sync              |
| **History**     | Persistent history via `HISTFILE`; ↑/↓ to navigate; `history` built-in with `-c`, `-r`, `-w`, `-a` |
| **Completion**  | Tab completion for executables and paths; LCP when multiple matches; list on second Tab            |
| **Pipelines**   | Chain commands with `\|` (pipe)                                                                    |
| **Redirection** | `>`, `>>`, `2>`, `2>>`, `&>`, `&>>` for stdout/stderr                                              |
| **Signals**     | Ctrl+C clears line; Ctrl+D exits if line empty, else shows completions                             |

- **Executable resolution**: Only files with execute permission (or Windows equivalents) are considered when resolving commands from `PATH`.
- **Longest common prefix**: When several completions share a prefix longer than the current token, the shell completes to that prefix; trailing `/` or space is added only for a single match.

---

## Build & Run

```bash
# Release build
cargo build --release

# Run the shell
./target/release/shell-rs
```

Or use the project script (builds then runs):

```bash
./your_program.sh
```

---

## Project Layout

| Path              | Purpose                                                   |
| ----------------- | --------------------------------------------------------- |
| `src/main.rs`     | Entry point, REPL loop                                    |
| `src/state.rs`    | Terminal state, input, history navigation, tab completion |
| `src/command.rs`  | Built-in dispatch and external command execution          |
| `src/pipeline.rs` | Pipeline parsing and execution                            |
| `src/token.rs`    | Tokenizer for the command line                            |
| `src/history.rs`  | History storage, load/save, append                        |

---

## Codecrafters

This project is part of the Codecrafters shell challenge. If you’re viewing this on GitHub, you can try the challenge at [codecrafters.io](https://codecrafters.io).
