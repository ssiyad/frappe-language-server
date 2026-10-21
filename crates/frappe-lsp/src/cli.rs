//! Command line. Spec 30 defines `--log-level`; everything else the editor
//! passes is either ignored or an error. The subcommands are spec 26's: the
//! server's answers for a program with a shell rather than an LSP client.

pub const HELP: &str = "\
frappe-lsp — language server for Frappe Framework apps

Usage: frappe-lsp [options]
       frappe-lsp check [--json] [<path>...]
       frappe-lsp describe [--json] <DocType>
       frappe-lsp references [--json] <DocType> [<fieldname>]

With no command, the server speaks LSP over stdio.

Commands:
  check       Report what is wrong in files or directories (default: .),
              one finding per line as path:line:column. Exits 1 on findings
  describe    Print a DocType's schema, after Custom Fields and Property Setters
  references  Print every place that names a DocType, or one of its fields

Each command finds the bench above its first path, or above the current
directory, and reuses the server's index cache.

Options:
  --json               Print the answer as JSON
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
    Command(Command),
    Help,
    Version,
}

/// A one-shot answer — [26](../../../docs/specs/26-command-line.md).
#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Check {
        paths: Vec<String>,
        json: bool,
    },
    Describe {
        doctype: String,
        json: bool,
    },
    References {
        doctype: String,
        fieldname: Option<String>,
        json: bool,
    },
}

pub fn parse<I>(arguments: I) -> Result<Invocation, String>
where
    I: IntoIterator<Item = String>,
{
    let mut arguments = arguments.into_iter().peekable();
    match arguments.peek().map(String::as_str) {
        Some("check" | "describe" | "references") => {
            let name = arguments.next().unwrap_or_default();
            command(&name, arguments)
        }
        _ => serve(arguments),
    }
}

fn serve(arguments: impl Iterator<Item = String>) -> Result<Invocation, String> {
    let mut args = Args::default();
    let mut arguments = arguments;

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

fn command(name: &str, arguments: impl Iterator<Item = String>) -> Result<Invocation, String> {
    let mut json = false;
    let mut operands = Vec::new();
    for argument in arguments {
        match argument.as_str() {
            "-h" | "--help" => return Ok(Invocation::Help),
            "--json" => json = true,
            flag if flag.starts_with('-') && flag != "-" => {
                return Err(format!("unknown option `{flag}` for `{name}`"));
            }
            _ => operands.push(argument),
        }
    }

    let command = match name {
        "check" => Command::Check {
            paths: operands,
            json,
        },
        "describe" => match operands.as_slice() {
            [doctype] => Command::Describe {
                doctype: doctype.clone(),
                json,
            },
            _ => return Err("`describe` takes one DocType name".to_string()),
        },
        _ => match operands.as_slice() {
            [doctype] => Command::References {
                doctype: doctype.clone(),
                fieldname: None,
                json,
            },
            [doctype, fieldname] => Command::References {
                doctype: doctype.clone(),
                fieldname: Some(fieldname.clone()),
                json,
            },
            _ => return Err("`references` takes a DocType name and an optional field".to_string()),
        },
    };
    Ok(Invocation::Command(command))
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
    fn check_takes_paths_and_json_in_any_order() {
        assert_eq!(
            parse_str(&["check", "apps/press", "--json", "hooks.py"]),
            Ok(Invocation::Command(Command::Check {
                paths: vec!["apps/press".to_string(), "hooks.py".to_string()],
                json: true,
            }))
        );
        assert_eq!(
            parse_str(&["check"]),
            Ok(Invocation::Command(Command::Check {
                paths: Vec::new(),
                json: false,
            }))
        );
    }

    #[test]
    fn a_doctype_name_with_spaces_is_one_operand() {
        assert_eq!(
            parse_str(&["references", "Site Update", "update_job"]),
            Ok(Invocation::Command(Command::References {
                doctype: "Site Update".to_string(),
                fieldname: Some("update_job".to_string()),
                json: false,
            }))
        );
        assert_eq!(
            parse_str(&["describe", "--json", "Site Update"]),
            Ok(Invocation::Command(Command::Describe {
                doctype: "Site Update".to_string(),
                json: true,
            }))
        );
    }

    #[test]
    fn a_command_rejects_what_it_cannot_use() {
        assert!(parse_str(&["describe"]).is_err());
        assert!(parse_str(&["describe", "Site", "Update"]).is_err());
        assert!(parse_str(&["references", "Site", "status", "extra"]).is_err());
        assert!(parse_str(&["check", "--fix"]).is_err());
    }

    #[test]
    fn unknown_arguments_are_rejected() {
        assert!(parse_str(&["--index-everything"]).is_err());
        assert!(parse_str(&["--log-level"]).is_err());
    }
}
