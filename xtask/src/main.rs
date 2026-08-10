//! Repository automation. Run as `cargo xtask <command>`.

mod smoke;

use std::process::ExitCode;

const HELP: &str = "\
cargo xtask <command>

Commands:
  codegen    Regenerate vendored Frappe constants from a frappe checkout
  smoke      Index a real bench (clones frappe + erpnext)
  help       Show this message
";

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    match command.as_deref() {
        Some("codegen") => unimplemented("codegen", "docs/development.md#vendored-constants"),
        Some("smoke") => {
            let bench = std::env::args()
                .skip(2)
                .find(|argument| !argument.starts_with("--"))
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("FRAPPE_LSP_REFERENCE_BENCH").map(std::path::PathBuf::from)
                })
                .or_else(|| {
                    std::env::var_os("HOME")
                        .map(|home| std::path::PathBuf::from(home).join("sources/frappe-dev"))
                })
                .unwrap_or_default();
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

// Failing rather than no-op'ing: CI calls these, and a silent success would
// report generated constants as up to date when nothing ran.
fn unimplemented(command: &str, spec: &str) -> ExitCode {
    eprintln!("xtask: `{command}` is not implemented yet — see {spec}");
    ExitCode::FAILURE
}
