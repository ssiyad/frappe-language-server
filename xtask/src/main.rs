//! Repository automation. Run as `cargo xtask <command>`.

mod drift;
mod smoke;
mod stress;

use std::process::ExitCode;

const HELP: &str = "\
cargo xtask <command>

Commands:
  codegen    --check [FRAPPE]: compare the vendored tables with what a frappe
             checkout reads, and fail on a name they lack
  smoke      Index a real bench and check its findings against the allowlist
  stub       Write the type stub of spec 24 for a bench: [BENCH] [--out DIR] [--all]
  stress     Drive the release binary like an editor over a real bench
             [--positions N] [--edits N] [--limit FILES] [--seed N] [--all]
  help       Show this message
";

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("codegen") if std::env::args().any(|argument| argument == "--check") => drift(),
        Some("codegen") => unimplemented("codegen", "docs/development.md#vendored-constants"),
        Some("stress") => stress(),
        Some("stub") => stub(),
        Some("smoke") => {
            let bench = bench();
            let write = std::env::args().any(|argument| argument == "--write");
            let all = std::env::args().any(|argument| argument == "--all");
            let absence = std::env::args().any(|argument| argument == "--absence");
            let allowlist = std::path::Path::new("tests/smoke-allowlist.toml").to_path_buf();

            match smoke::run(&bench, &allowlist, write, all, absence) {
                Ok(0) => ExitCode::SUCCESS,
                Ok(_) => ExitCode::FAILURE,
                Err(error) => {
                    eprintln!("xtask: smoke failed: {error}");
                    ExitCode::FAILURE
                }
            }
        }
        Some("help" | "--help" | "-h") | None => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("xtask: unknown command `{other}`\n\n{HELP}");
            ExitCode::from(2)
        }
    }
}

fn drift() -> ExitCode {
    // A checkout named on the command line, else the reference bench's.
    let frappe = std::env::args()
        .skip(2)
        .find(|argument| !argument.starts_with("--"))
        .map_or_else(|| bench().join("apps/frappe"), std::path::PathBuf::from);
    match drift::run(&frappe) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("xtask: codegen --check failed: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The bench named on the command line, else the reference bench.
fn bench() -> std::path::PathBuf {
    let arguments: Vec<String> = std::env::args().skip(2).collect();
    let mut positional = arguments.iter().enumerate().filter(|(index, argument)| {
        !argument.starts_with("--")
            && !(*index > 0 && VALUED.contains(&arguments[index - 1].as_str()))
    });
    positional
        .next()
        .map(|(_, bench)| std::path::PathBuf::from(bench))
        .or_else(|| std::env::var_os("FRAPPE_LSP_REFERENCE_BENCH").map(std::path::PathBuf::from))
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|home| std::path::PathBuf::from(home).join("sources/frappe-dev"))
        })
        .unwrap_or_default()
}

/// Flags that take a value, so the value is not read as the bench.
const VALUED: [&str; 5] = ["--positions", "--edits", "--limit", "--seed", "--out"];

fn stub() -> ExitCode {
    let bench = bench();
    let arguments: Vec<String> = std::env::args().collect();
    let out = arguments
        .iter()
        .position(|argument| argument == "--out")
        .and_then(|at| arguments.get(at + 1))
        .map_or_else(|| bench.join(".frappe-lsp/stubs"), std::path::PathBuf::from);
    let scope = if arguments.iter().any(|argument| argument == "--all") {
        frappe_index::stub::Scope::All
    } else {
        frappe_index::stub::Scope::Referenced
    };

    let started = std::time::Instant::now();
    let mut index = frappe_index::Index::build(&bench);
    index.scan_occurrences();
    match frappe_index::stub::generate(&index, scope) {
        Ok(stub) => match frappe_index::stub::write(&out, &stub) {
            Ok(_) => {
                println!(
                    "{} overloads for {} DocTypes in {}/frappe/model/document.pyi, {:?}",
                    stub.overloads,
                    stub.doctypes,
                    out.display(),
                    started.elapsed()
                );
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("xtask: could not write the stub: {error}");
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("xtask: no stub: {error}");
            ExitCode::FAILURE
        }
    }
}

fn number(flag: &str, default: u64) -> u64 {
    let arguments: Vec<String> = std::env::args().collect();
    arguments
        .iter()
        .position(|argument| argument == flag)
        .and_then(|index| arguments.get(index + 1))
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn stress() -> ExitCode {
    // The binary an editor runs, not the debug build the tests use.
    let built = std::process::Command::new(env!("CARGO"))
        .args([
            "build",
            "--release",
            "--quiet",
            "--package",
            "frappe-lsp",
            "--bin",
            "frappe-lsp",
        ])
        .status();
    if !built.is_ok_and(|status| status.success()) {
        eprintln!("xtask: could not build the release binary");
        return ExitCode::FAILURE;
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| "target".into(), std::path::PathBuf::from);
    let options = stress::Options {
        bench: bench(),
        binary: target.join("release/frappe-lsp"),
        log: target.join("stress.log"),
        positions: usize::try_from(number("--positions", 24)).unwrap_or(24),
        edits: usize::try_from(number("--edits", 4)).unwrap_or(4),
        limit: usize::try_from(number("--limit", 0)).unwrap_or(0),
        seed: number("--seed", 1),
        all: std::env::args().any(|argument| argument == "--all"),
    };
    println!(
        "stress: seed {}, log in {}",
        options.seed,
        options.log.display()
    );
    match stress::run(&options) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("xtask: stress failed: {error}");
            ExitCode::FAILURE
        }
    }
}

// Failing rather than no-op'ing: CI calls these, and a silent success would
// report generated constants as up to date when nothing ran.
fn unimplemented(command: &str, spec: &str) -> ExitCode {
    eprintln!("xtask: `{command}` is not implemented yet — see {spec}");
    ExitCode::FAILURE
}
