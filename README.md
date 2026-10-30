# rtrash

`rtrash` is a Linux CLI for an independent, user-selected trash directory. It never selects the system trash automatically.

## Build and install

Install a stable Rust toolchain, then run `cargo build --release`. Install with `cargo install --path .`.

## Usage

Select storage using `--trash-dir PATH` or `RTRASH_DIR`; the option takes precedence.

```sh
export RTRASH_DIR="$HOME/my-trash"
rtrash report.txt directory
rtrash --list
rtrash --restore 'trash://report.txt'
rtrash --empty
rtrash -f missing-file
rtrash --trash-dir /path/to/other-trash --list
```

`--list` prints an rtrash-local URI and original path separated by a tab. The URI identifies the stored filename and can be passed to `--restore`; it is not registered with GIO. The displayed original path is for humans and may be lossy for non-UTF-8 names. `--force` suppresses errors for inputs that cannot be trashed. Restores create missing parents and refuse to overwrite a destination.

## Storage and limitations

The selected directory contains `files/` and `info/`. Each entry has a matching `.trashinfo` file containing a percent-encoded absolute original path and local deletion time, following the freedesktop.org Trash Specification. Name collisions receive numeric suffixes. Symlinks are stored as links.

Unlike `gio trash`, storage is explicitly selected and independent of GIO. Moving across filesystems is unsupported and returns an error without deleting the source. Atomic no-replace renames require Linux and filesystem support for `renameat2`. An interrupted operation may leave orphaned metadata; `--empty` removes both stored entries and metadata. Do not share a trash directory with untrusted users or mutate its directories concurrently with rtrash. `--empty` permanently deletes its contents.
