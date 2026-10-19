use std::process::ExitCode;

use frappe_lsp::guard::Guarded;
use frappe_lsp::server::{Backend, SERVER_NAME, VERSION};
use frappe_lsp::{cli, logging};
use tower_lsp_server::{LspService, Server};

fn main() -> ExitCode {
    bound_malloc_arenas();
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(serve()),
        Err(error) => {
            eprintln!("frappe-lsp: could not start the async runtime: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Run once more with two glibc malloc arenas, unless the user chose a number.
///
/// tree-sitter allocates through the C library, and glibc gives every thread
/// its own arena and seldom returns what is freed in one. Parsing a whole bench
/// on every core left 660 MB resident for 60 MB of live index; two arenas keep
/// about 400 MB ([31](../../../docs/specs/31-performance.md)). The variable is
/// read when the process starts, so setting it means starting again, and
/// `exec` keeps the PID and the stdio the editor is talking to.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn bound_malloc_arenas() {
    use std::os::unix::process::CommandExt;

    const ARENAS: &str = "MALLOC_ARENA_MAX";
    if std::env::var_os(ARENAS).is_some() {
        return;
    }
    let Ok(binary) = std::env::current_exe() else {
        return;
    };
    let error = std::process::Command::new(binary)
        .args(std::env::args_os().skip(1))
        .env(ARENAS, "2")
        .exec();
    // Only reached when `exec` failed; the server still works, with more memory.
    eprintln!("frappe-lsp: could not restart with bounded malloc arenas: {error}");
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn bound_malloc_arenas() {}

async fn serve() -> ExitCode {
    let args = match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Invocation::Serve(args)) => args,
        Ok(cli::Invocation::Help) => {
            print!("{}", cli::HELP);
            return ExitCode::SUCCESS;
        }
        Ok(cli::Invocation::Version) => {
            println!("{SERVER_NAME} {VERSION}");
            return ExitCode::SUCCESS;
        }
        Err(message) => {
            eprintln!("frappe-lsp: {message}\n\n{}", cli::HELP);
            return ExitCode::from(2);
        }
    };

    logging::init(args.log_level.as_deref());
    tracing::info!(version = VERSION, "starting");

    // The custom methods of spec 30. `indexStatus` in particular is what makes
    // a bug report actionable without a reproduction.
    let (service, socket) = LspService::build(Backend::new)
        .custom_method("frappe-lsp/indexStatus", Backend::index_status)
        .custom_method("frappe-lsp/reindex", Backend::reindex)
        .custom_method("frappe-lsp/refreshBridge", Backend::refresh_bridge)
        .finish();
    // A panic in one request answers that request with an error; unguarded,
    // it would end the session.
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(Guarded(service))
        .await;

    ExitCode::SUCCESS
}
