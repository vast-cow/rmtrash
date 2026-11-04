# Usage

[README](../README.md) · [Usage](usage.md) · [Storage](storage.md) · [Build](build.md)

## Examples

```sh
export RMTRASH_DIR="$HOME/my-trash"
rmtrash report.txt directory
rmtrash --list
rmtrash --restore 'trash://report.txt'
rmtrash --empty
rmtrash -f missing-file
rmtrash --trash-dir /path/to/other-trash --list
```

**Warning:** `--empty` permanently deletes the selected trash directory's stored entries and metadata.

## Command-line options

With no operation option, pass one or more files or directories to move them to
trash. Invoking the command without files or an operation reports an error.

| Option | Behavior |
| --- | --- |
| `--trash-dir PATH` | Select the trash directory for this invocation. |
| `-f`, `--force` | Suppress errors for missing inputs. Other failures are reported. |
| `--list` | List stored entries and their original paths. |
| `--restore URI` | Restore the entry identified by an rmtrash-local URI. |
| `--empty` | Permanently delete stored entries and metadata. |
| `-h`, `--help` | Show command-line help. |
| `-V`, `--version` | Show the version. |

`--list`, `--restore`, and `--empty` are mutually exclusive and cannot be
combined with file arguments or `--force`. `--trash-dir` applies to all operations.

## Trash directory selection

Storage is selected in this order: `--trash-dir PATH`, `RMTRASH_DIR`, `$XDG_DATA_HOME/Trash`, then `$HOME/.local/share/Trash`. An unset, empty, or relative `XDG_DATA_HOME` falls back to the HOME location. If no explicit location is given and neither a valid `XDG_DATA_HOME` nor a nonempty `HOME` is available, the command reports an error.

Mount points are not searched automatically, and the selected trash directory is
used for all operations. See [storage and limitations](storage.md) for filesystem
requirements and per-user trash directory checks.

## Listing, errors, and restoration

`--list` prints an rmtrash-local URI and original path separated by a tab. The URI identifies the stored filename and can be passed to `--restore`; it is not registered with GIO. The displayed original path is for humans and may be lossy for non-UTF-8 names. `--force` suppresses errors for missing inputs; other failures are reported. Restores create missing parents and refuse to overwrite a destination.

If an entry's metadata is missing or invalid, `--list` reports its URI and the error on standard error, continues listing valid entries, and exits unsuccessfully.

