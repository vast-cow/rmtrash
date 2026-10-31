# rmtrash

`rmtrash` is a Linux CLI for managing a trash directory. It uses the user's default trash directory unless a custom location is selected.

## Build and install

Install a stable Rust toolchain, then run `cargo build --release`. Install with `cargo install --path .`.

The release profile favors a small executable: size optimization (`z`), fat LTO,
one codegen unit, aborting panics, and stripped symbols. LTO and a single codegen
unit increase build time. Unexpected panics terminate immediately without stack
unwinding or running destructors; normal filesystem and CLI errors still use the
existing error handling. Stripping limits symbol-based debugging. Development
builds retain the default settings.

Size comparison using Rust 1.85.1 on `x86_64-unknown-linux-gnu`, the same
`Cargo.lock`, and `cargo build --release --locked`:

| Release configuration | Executable bytes |
| --- | ---: |
| Original Cargo defaults | 1,440,952 |
| Defaults, with symbols stripped | 1,165,824 |
| `s`, fat LTO, one codegen unit, abort, stripped | 756,224 |
| `z`, fat LTO, one codegen unit, abort, stripped (selected) | 739,840 |
| `3`, fat LTO, one codegen unit, abort, stripped | 883,200 |
| `z`, thin LTO, one codegen unit, abort, stripped | 883,200 |
| `s`, thin LTO, one codegen unit, abort, stripped | 887,296 |
| `s`, fat LTO, one codegen unit, unwind, stripped | 813,568 |
| `s`, fat LTO, 16 codegen units, abort, stripped | 776,704 |

The selected configuration reduces the executable by 701,112 bytes (48.7%).
Sizes vary with the toolchain and target. To compare alternatives locally,
override individual settings, for example
`CARGO_PROFILE_RELEASE_OPT_LEVEL=s cargo build --release --locked`, and measure
with `stat -c %s target/release/rmtrash`.

Dependency features are retained to preserve behavior: Clap's defaults provide
help, usage, colors, error context, and suggestions; its `derive` and `env`
features support the CLI implementation and `RMTRASH_DIR`. Chrono already
disables default features and enables `clock` for local deletion timestamps.

Validation: all 12 integration tests passed with both `cargo test --locked` and
`cargo test --release --locked`. The actual release executable also matched the
baseline's output and exit status in eight CLI cases and passed a
trash/restore/force/empty smoke check. In 60 alternating, warm-cache runs listing
500 entries, median elapsed time was 2.474 ms before and 2.619 ms after (about
6% slower, or 0.145 ms). This local measurement is not a general performance
guarantee.

## Usage

Storage is selected in this order: `--trash-dir PATH`, `RMTRASH_DIR`, `$XDG_DATA_HOME/Trash`, then `$HOME/.local/share/Trash`. An unset, empty, or relative `XDG_DATA_HOME` falls back to the HOME location. If no explicit location is given and neither a valid `XDG_DATA_HOME` nor a nonempty `HOME` is available, the command reports an error.

```sh
export RMTRASH_DIR="$HOME/my-trash"
rmtrash report.txt directory
rmtrash --list
rmtrash --restore 'trash://report.txt'
rmtrash --empty
rmtrash -f missing-file
rmtrash --trash-dir /path/to/other-trash --list
```

`--list` prints an rmtrash-local URI and original path separated by a tab. The URI identifies the stored filename and can be passed to `--restore`; it is not registered with GIO. The displayed original path is for humans and may be lossy for non-UTF-8 names. `--force` suppresses errors for missing inputs; other failures are reported. Restores create missing parents and refuse to overwrite a destination.

If an entry's metadata is missing or invalid, `--list` reports its URI and the error on standard error, continues listing valid entries, and exits unsuccessfully.

## Storage and limitations

The selected directory contains `files/` and `info/`. Each entry has a matching `.trashinfo` file containing a percent-encoded absolute original path and local deletion time, following the freedesktop.org Trash Specification. Name collisions receive numeric suffixes. Symlinks are stored as links.

For trashed directories, `directorysizes` caches allocated disk space and the corresponding metadata modification time. It is rebuilt after directory trashing, restoration, and emptying; entries that cannot be scanned are omitted. Cache updates are best effort, so they do not change the result of a completed trash operation.

When reading metadata from other implementations, relative original paths are resolved against the selected trash directory's parent. Paths containing `..` or escaping that parent through a symlink are rejected during restoration.

Storage is managed directly without GIO. Moving across filesystems is unsupported and returns an error without deleting the source. Atomic no-replace renames require Linux and filesystem support for `renameat2`. An interrupted operation may leave orphaned metadata; `--empty` removes both stored entries and metadata. Do not share a trash directory with untrusted users or mutate its directories concurrently with rmtrash. `--empty` permanently deletes its contents.

When explicitly selecting `.Trash/<uid>` or `.Trash-<uid>`, the UID must match the effective user and the per-user directory must be a real directory owned by that user. The shared `.Trash` must already exist, be a real directory, and have its sticky bit set. Newly created trash directories use mode 0700. Mount points are not searched automatically, and the selected trash directory is used for all operations.
