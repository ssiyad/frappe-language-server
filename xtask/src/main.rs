//! Repository automation. Run as `cargo xtask <command>`.

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
        Some("smoke") => unimplemented("smoke", "docs/specs/32-testing.md"),
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
