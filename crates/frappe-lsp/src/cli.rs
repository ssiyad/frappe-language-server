//! Command line. Spec 30 defines `--log-level`; everything else the editor
//! passes is either ignored or an error.

pub const HELP: &str = "\
frappe-lsp — language server for Frappe Framework apps

Usage: frappe-lsp [options]

The server speaks LSP over stdio; there is nothing to run interactively.

Options:
  --log-level <level>  error | warn | info | debug | trace, or a tracing filter
                       (overrides FRAPPE_LSP_LOG; default: info)
  --stdio              Accepted and ignored; stdio is the only transport
  -V, --version        Print version and exit
  -h, --help           Print this message and exit
";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Args {
    pub log_level: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Invocation {
    Serve(Args),
    Help,
    Version,
}

pub fn parse<I>(arguments: I) -> Result<Invocation, String>
where
    I: IntoIterator<Item = String>,
{
    let mut args = Args::default();
    let mut arguments = arguments.into_iter();

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(Invocation::Help),
            "-V" | "--version" => return Ok(Invocation::Version),
            "--stdio" => {}
            "--log-level" => {
                let value = arguments
                    .next()
                    .ok_or_else(|| "--log-level needs a value".to_string())?;
                args.log_level = Some(value);
            }
            other => match other.strip_prefix("--log-level=") {
                Some(value) => args.log_level = Some(value.to_string()),
                None => return Err(format!("unknown argument `{other}`")),
            },
        }
    }

    Ok(Invocation::Serve(args))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(arguments: &[&str]) -> Result<Invocation, String> {
        parse(arguments.iter().map(|s| (*s).to_string()))
    }

    #[test]
    fn log_level_takes_either_form() {
        let expected = Ok(Invocation::Serve(Args {
            log_level: Some("debug".to_string()),
        }));

        assert_eq!(parse_str(&["--log-level", "debug"]), expected);
        assert_eq!(parse_str(&["--log-level=debug"]), expected);
    }

    #[test]
    fn stdio_is_accepted() {
        assert_eq!(
            parse_str(&["--stdio"]),
            Ok(Invocation::Serve(Args::default()))
        );
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        assert!(parse_str(&["--index-everything"]).is_err());
        assert!(parse_str(&["--log-level"]).is_err());
    }
}
