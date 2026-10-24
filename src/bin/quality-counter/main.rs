//! Repository quality counter: the physical-line limit and the Rust source
//! population.
//!
//! One implementation feeds the hard gate (`--hard`), the Ratchet rule and the
//! report: a file counts when it has more than `--max` physical lines, where
//! blank lines and comments count and a trailing newline adds no phantom line.
//!
//! Maintained sources are every `*.rs` file in the repository, including
//! `build.rs`, root files, test modules and new untracked files. The generated
//! build/runtime directories `target/`, `output/` and `.git` are not maintained
//! sources, but only when they sit directly at the scan root. A nested
//! directory with one of those names is ordinary source-tree structure and is
//! scanned; a name collision must not hide a maintained file.

mod scan;

use std::process::ExitCode;

const DEFAULT_MAX: u64 = 500;

const USAGE: &str = "\
usage:
  quality-counter oversized [--root DIR] [--max N] [--format count|items|json] [--hard]
  quality-counter rust-files [--root DIR] [--format count|items|json]

The hard gate exits 1 when a maintained Rust file exceeds the limit. Without
--hard the command reports the count and exits 0; an empty selection is always
an error.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Count,
    Items,
    Json,
}

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Problem::Usage(message)) => {
            eprintln!("quality-counter: {message}");
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
        Err(Problem::Failure(message)) => {
            eprintln!("quality-counter: {message}");
            ExitCode::FAILURE
        }
        Err(Problem::Empty(message)) => {
            eprintln!("quality-counter: {message}");
            ExitCode::from(3)
        }
    }
}

enum Problem {
    Usage(String),
    Failure(String),
    Empty(String),
}

fn run(args: Vec<String>) -> Result<(), Problem> {
    let command = args
        .first()
        .ok_or_else(|| Problem::Usage(USAGE.to_string()))?;
    let mut root = std::path::PathBuf::from(".");
    let mut max = DEFAULT_MAX;
    let mut format = Format::Count;
    let mut hard = false;

    let mut index = 1;
    while index < args.len() {
        let argument = &args[index];
        match argument.as_str() {
            "--root" => {
                index += 1;
                root = std::path::PathBuf::from(
                    args.get(index)
                        .ok_or_else(|| Problem::Usage("--root needs a directory".to_string()))?,
                );
            }
            "--max" => {
                index += 1;
                let value = args
                    .get(index)
                    .ok_or_else(|| Problem::Usage("--max needs a number".to_string()))?;
                max = value
                    .parse()
                    .map_err(|_| Problem::Usage(format!("invalid --max value {value:?}")))?;
            }
            "--format" => {
                index += 1;
                format = match args.get(index).map(String::as_str) {
                    Some("count") => Format::Count,
                    Some("items") => Format::Items,
                    Some("json") => Format::Json,
                    Some(other) => {
                        return Err(Problem::Usage(format!(
                            "unknown --format {other:?}; expected count, items or json"
                        )));
                    }
                    None => return Err(Problem::Usage("--format needs a value".to_string())),
                };
            }
            "--hard" => hard = true,
            _ => return Err(Problem::Usage(format!("unknown argument {argument:?}"))),
        }
        index += 1;
    }

    let mut files = scan::rust_files(&root).map_err(Problem::Failure)?;
    if files.is_empty() {
        return Err(Problem::Empty(format!(
            "no maintained Rust files under {}; an empty selection is never a pass",
            root.display()
        )));
    }
    files.sort();

    match command.as_str() {
        "rust-files" => {
            if hard {
                return Err(Problem::Usage(
                    "--hard applies to oversized, not rust-files".to_string(),
                ));
            }
            let total = files.len() as u64;
            emit(format, total, &files, &root);
            Ok(())
        }
        "oversized" => {
            let mut oversized = Vec::new();
            for path in files {
                let lines = scan::physical_lines(&path).map_err(Problem::Failure)?;
                if lines > max {
                    if hard {
                        eprintln!(
                            "quality-counter: {} has {lines} physical lines (limit {max})",
                            path.display()
                        );
                    }
                    oversized.push(path);
                }
            }
            if hard && !oversized.is_empty() {
                emit(format, oversized.len() as u64, &oversized, &root);
                return Err(Problem::Failure(format!(
                    "{} file(s) exceed the hard limit of {max} physical lines",
                    oversized.len()
                )));
            }
            emit(format, oversized.len() as u64, &oversized, &root);
            Ok(())
        }
        other => Err(Problem::Usage(format!("unknown command {other:?}"))),
    }
}

fn emit(format: Format, count: u64, files: &[std::path::PathBuf], root: &std::path::Path) {
    let items = || {
        files
            .iter()
            .map(|path| scan::relative_name(root, path))
            .collect::<Vec<_>>()
    };
    match format {
        Format::Count => println!("{count}"),
        Format::Items => println!("{}", json_array(&items())),
        Format::Json => println!("{{\"count\":{count},\"items\":{}}}", json_array(&items())),
    }
}

fn json_array(items: &[String]) -> String {
    let mut out = String::from("[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push('"');
        for character in item.chars() {
            match character {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                control if control.is_control() => {
                    out.push_str(&format!("\\u{:04x}", control as u32));
                }
                other => out.push(other),
            }
        }
        out.push('"');
    }
    out.push(']');
    out
}
