# Project Conventions

## Validation after every change

After every code edit, run all three:

```sh
cargo check
cargo fmt
cargo clippy -- -W clippy::pedantic
```

Fix all warnings before reporting work complete.
