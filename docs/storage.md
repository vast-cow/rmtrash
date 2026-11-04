# Storage and limitations

[README](../README.md) · [Usage](usage.md) · [Storage](storage.md) · [Build](build.md)

## Layout and metadata

The selected directory contains `files/` and `info/`. Each entry has a matching `.trashinfo` file containing a percent-encoded absolute original path and local deletion time, following the freedesktop.org Trash Specification. Name collisions receive numeric suffixes. Symlinks are stored as links.

## Directory size cache

For trashed directories, `directorysizes` caches allocated disk space and the corresponding metadata modification time. It is rebuilt after directory trashing, restoration, and emptying; entries that cannot be scanned are omitted. Cache updates are best effort, so they do not change the result of a completed trash operation.

## Metadata compatibility

When reading metadata from other implementations, relative original paths are resolved against the selected trash directory's parent. Paths containing `..` or escaping that parent through a symlink are rejected during restoration.

## Filesystem requirements and security

Storage is managed directly without GIO. Moving across filesystems is unsupported and returns an error without deleting the source. Atomic no-replace renames require Linux and filesystem support for `renameat2`. An interrupted operation may leave orphaned metadata; `--empty` removes both stored entries and metadata. Do not share a trash directory with untrusted users or mutate its directories concurrently with rmtrash. `--empty` permanently deletes its contents.

## Per-user trash directories

When explicitly selecting `.Trash/<uid>` or `.Trash-<uid>`, the UID must match the effective user and the per-user directory must be a real directory owned by that user. The shared `.Trash` must already exist, be a real directory, and have its sticky bit set. Newly created trash directories use mode 0700. Mount points are not searched automatically, and the selected trash directory is used for all operations.

## Lustre compatibility

Trashing and restoration first use `renameat2(RENAME_NOREPLACE)`. If it returns
`EINVAL`, rmtrash checks the destination parent with `statfs`. Only a confirmed
Lustre filesystem (magic `0x0BD00BD0`) enables a fallback: `symlink_metadata`
checks for an existing destination, including dangling symlinks, then `rename`
moves the source if the destination is absent. Existing entries return
`AlreadyExists`, so trash filename collisions still receive numeric suffixes.
Other lookup or rename errors are propagated. Failed filesystem detection retains
the original `EINVAL`; other filesystems and syscall errors never use the fallback.
See [Lustre LU-12272](https://jira.whamcloud.com/browse/LU-12272).

This fallback does **not** provide atomic no-replace semantics. There is a TOCTOU
race between the existence check and `rename`: a destination created in that
interval can be overwritten. Avoid concurrent changes to destination paths on
Lustre. Failed moves retain the existing metadata cleanup behavior.
