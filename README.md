# rmtrash

`rmtrash` is a Linux CLI for managing a trash directory. It uses the user's default trash directory unless a custom location is selected.

## Build and install

Install a stable Rust toolchain, then run `cargo build --release`. Install with `cargo install --path .`.

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

When reading metadata from other implementations, relative original paths are resolved against the selected trash directory's parent. Paths containing `..` or escaping that parent through a symlink are rejected during restoration.

Storage is managed directly without GIO. Moving across filesystems is unsupported and returns an error without deleting the source. Atomic no-replace renames require Linux and filesystem support for `renameat2`. An interrupted operation may leave orphaned metadata; `--empty` removes both stored entries and metadata. Do not share a trash directory with untrusted users or mutate its directories concurrently with rmtrash. `--empty` permanently deletes its contents.

When explicitly selecting `.Trash/<uid>` or `.Trash-<uid>`, the UID must match the effective user and the per-user directory must be a real directory owned by that user. The shared `.Trash` must already exist, be a real directory, and have its sticky bit set. Newly created trash directories use mode 0700. Mount points are not searched automatically, and the selected trash directory is used for all operations.
