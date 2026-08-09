use std::process::ExitCode;

use frappe_lsp::server::{Backend, SERVER_NAME, VERSION};
use frappe_lsp::{cli, logging};
use tower_lsp_server::{LspService, Server};

#[tokio::main]
async fn main() -> ExitCode {
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

    let (service, socket) = LspService::new(Backend::new);
    Server::new(tokio::io::stdin(), tokio::io::stdout(), socket)
        .serve(service)
        .await;

    ExitCode::SUCCESS
}
