use clap::{ArgGroup, Parser};
use rtrash::Trash;
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(version, about = "Manage an independent trash directory", group(ArgGroup::new("operation").args(["list", "restore", "empty"]).multiple(false)))]
struct Args {
    #[arg(long, env = "RTRASH_DIR")]
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
fn main() -> ExitCode {
    let args = Args::parse();
    let Some(dir) = args.trash_dir else {
        eprintln!("rtrash: specify --trash-dir or RTRASH_DIR");
        return ExitCode::FAILURE;
    };
    if !args.list && args.restore.is_none() && !args.empty && args.files.is_empty() {
        eprintln!("rtrash: specify files or an operation");
        return ExitCode::FAILURE;
    }
    let trash = match Trash::open(&dir) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("rtrash: {e}");
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
                if !args.force {
                    eprintln!("rtrash: {}: {e}", p.display());
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
            eprintln!("rtrash: {e}");
            ExitCode::FAILURE
        }
    }
}
