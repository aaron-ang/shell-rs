# Project Conventions

## Validation after every change

After every code edit, run:

```sh
cargo check
cargo fmt
cargo clippy --tests -- -W clippy::pedantic
cargo test
```

Fix all warnings before reporting work complete.

## Unit tests

Every new feature or user-facing behavior change must ship with unit tests in the same change. Test at the layer that exercises the user-facing contract — don't duplicate the same assertion at multiple layers.
