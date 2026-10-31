use clap::{ArgGroup, Parser};
use rmtrash::Trash;
use std::{env, path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(version, about = "Manage a trash directory", group(ArgGroup::new("operation").args(["list", "restore", "empty"]).multiple(false)))]
struct Args {
    /// Override the default $XDG_DATA_HOME/Trash or $HOME/.local/share/Trash
    #[arg(long, env = "RMTRASH_DIR")]
    trash_dir: Option<PathBuf>,
    #[arg(short, long, conflicts_with_all = ["list", "restore", "empty"])]
    force: bool,
    #[arg(long, conflicts_with = "files")]
    list: bool,
    #[arg(long, value_name = "URI", conflicts_with = "files")]
    restore: Option<String>,
    #[arg(long, conflicts_with = "files")]
    empty: bool,
    #[arg(value_name = "FILE")]
    files: Vec<PathBuf>,
}
fn default_trash_dir() -> Option<PathBuf> {
    env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .map(|path| path.join("Trash"))
        .or_else(|| {
            env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".local/share/Trash"))
        })
}
fn main() -> ExitCode {
    let args = Args::parse();
    let Some(dir) = args.trash_dir.or_else(default_trash_dir) else {
        eprintln!("rmtrash: cannot determine trash directory; set HOME or XDG_DATA_HOME, or specify --trash-dir or RMTRASH_DIR");
        return ExitCode::FAILURE;
    };
    if !args.list && args.restore.is_none() && !args.empty && args.files.is_empty() {
        eprintln!("rmtrash: specify files or an operation");
        return ExitCode::FAILURE;
    }
    let trash = match Trash::open(&dir) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("rmtrash: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = if args.list {
        trash.list().map(|items| {
            for (uri, path) in items {
                println!("{}\t{}", uri, path.display());
            }
        })
    } else if let Some(uri) = args.restore {
        trash.restore(&uri)
    } else if args.empty {
        trash.empty()
    } else {
        let mut failed = false;
        for p in args.files {
            if let Err(e) = trash.put(&p) {
                let missing = e.kind() == std::io::ErrorKind::NotFound
                    && matches!(std::fs::symlink_metadata(&p), Err(ref error) if error.kind() == std::io::ErrorKind::NotFound);
                if !args.force || !missing {
                    eprintln!("rmtrash: {}: {e}", p.display());
                    failed = true;
                }
            }
        }
        if failed {
            Err(std::io::Error::other("some entries could not be trashed"))
        } else {
            Ok(())
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("rmtrash: {e}");
            ExitCode::FAILURE
        }
    }
}
