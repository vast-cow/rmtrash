# rmtrash

`rmtrash` is a Linux CLI for managing a trash directory. It uses the user's default trash directory unless a custom location is selected.

## Installation

Install a stable Rust toolchain, then run from the project directory:

```sh
cargo install --path .
```

To build without installing, run `cargo build --release`.

## Quick start

```sh
rmtrash report.txt directory
rmtrash --list
rmtrash --restore 'trash://report.txt'
rmtrash --trash-dir /path/to/other-trash --list
```

Use the URI printed by `--list` when restoring. By default, storage is
`$XDG_DATA_HOME/Trash` when `XDG_DATA_HOME` is absolute, otherwise
`$HOME/.local/share/Trash`. Override it with `--trash-dir` or `RMTRASH_DIR`.

`rmtrash --empty` permanently deletes stored entries and metadata in the selected
trash directory. Cross-filesystem moves are unsupported.

## Documentation

- [Usage](docs/usage.md): options, examples, directory selection, errors, and restoration.
- [Storage](docs/storage.md): metadata, filesystem limitations, and security considerations.
- [Build](docs/build.md): release configuration, binary sizes, and recorded validation and benchmarks.
