//! Executable adapters for the public Hudson and Hudsonbox APIs.
//! This crate's Rust modules are not a supported SDK surface.
mod agent;
#[cfg(unix)]
mod local;
mod local_args;
mod sandbox;

use clap::{Parser, Subcommand};
use std::{ffi::OsString, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "hudson",
    version,
    about = "Command-line interface for Hudson products",
    args_conflicts_with_subcommands = true,
    after_help = "Without a command, Hudson opens the local terminal agent on Unix. Legacy API commands remain available at the root; hudson-cli --help lists them."
)]
struct Cli {
    #[command(flatten)]
    local: local_args::LocalArgs,
    #[command(subcommand)]
    product: Option<Product>,
}

#[derive(Subcommand)]
enum Product {
    /// Run the local terminal agent in this workspace (Unix; use WSL on Windows).
    Local(local_args::LocalArgs),
    /// Connect to the Hudson agent API.
    Agent(agent::commands::Args),
    /// Manage Hudsonbox sandboxes through the project HTTPS API.
    Sandbox(sandbox::Cli),
    /// Print a shell completion script without connecting to a service.
    Completion {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

enum Invocation {
    Product(Cli),
    LegacyAgent(agent::commands::Args),
}

fn parse(args: &[OsString]) -> Result<Invocation, clap::Error> {
    // A valid historical definition lookup wins even when the definition name
    // matches a new agent operation (for example `hudson agent get 7`).
    let legacy_error = match agent::commands::Args::try_parse_from(args) {
        Ok(legacy) if args.len() > 1 => return Ok(Invocation::LegacyAgent(legacy)),
        Ok(_) => return Cli::try_parse_from(args).map(Invocation::Product),
        Err(error) => error,
    };
    let informational = |error: &clap::Error| {
        matches!(
            error.kind(),
            clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
        )
    };
    match Cli::try_parse_from(args) {
        Ok(cli) => Ok(Invocation::Product(cli)),
        // Prefer product help when both parsers recognize it, but retain help
        // for legacy flat commands such as `hudson start --help`.
        Err(error) if informational(&legacy_error) && !informational(&error) => Err(legacy_error),
        Err(error) => Err(error),
    }
}

fn error_cause<'a, T: std::error::Error + 'static>(
    error: &'a (dyn std::error::Error + 'static),
) -> Option<&'a T> {
    let mut current = Some(error);
    while let Some(error) = current {
        if let Some(cause) = error.downcast_ref::<T>() {
            return Some(cause);
        }
        current = error.source();
    }
    None
}

fn agent_execute(args: agent::commands::Args) -> ExitCode {
    let json = args.json;
    match agent::client::execute(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let (kind, status, message) = if let Some(http) =
                error_cause::<agent::client::HttpFailure>(error.as_ref())
            {
                ("http", Some(http.0.as_u16()), error.to_string())
            } else if error_cause::<agent::client::ProtocolFailure>(error.as_ref()).is_some() {
                ("protocol", None, error.to_string())
            } else if error_cause::<agent::client::TransportFailure>(error.as_ref()).is_some() {
                ("transport", None, error.to_string())
            } else if let Some(request) = error_cause::<reqwest::Error>(error.as_ref()) {
                let (kind, message) = if request.is_timeout() {
                    ("timeout", "Agent API request timed out. Inspect the outcome before retrying a mutation.")
                } else if request.is_decode() {
                    ("protocol", "Agent API returned an invalid JSON response.")
                } else if request.is_builder() {
                    ("configuration", "Cannot configure the agent API request. Check the URL and connection options.")
                } else {
                    (
                        "transport",
                        "Agent API request failed. Inspect the outcome before retrying a mutation.",
                    )
                };
                (kind, None, message.to_owned())
            } else {
                ("client", None, error.to_string())
            };
            if json {
                eprintln!(
                    "{}",
                    serde_json::json!({"error":kind,"http_status":status,"message":message})
                );
            } else {
                eprintln!("{message}");
            }
            ExitCode::FAILURE
        }
    }
}

fn sandbox_execute(args: sandbox::Cli) -> ExitCode {
    match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime.block_on(sandbox::execute(args)),
        Err(_) => {
            eprintln!("cannot start the sandbox client runtime");
            ExitCode::FAILURE
        }
    }
}

fn local_execute(args: local_args::LocalArgs) -> ExitCode {
    #[cfg(unix)]
    {
        match local::run(args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{}", agent::safe(&error.to_string()));
                ExitCode::FAILURE
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = args;
        eprintln!("The local terminal agent requires Unix. Run Hudson in WSL on Windows; native agent API and offline sandbox commands remain available.");
        ExitCode::FAILURE
    }
}

/// Unified `hudson` executable, with the existing flat agent commands retained.
pub fn main_entry() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    #[cfg(unix)]
    if let Some(error) = local::dispatch_internal(args.get(1..).unwrap_or_default()) {
        eprintln!(
            "{}",
            agent::safe(&format!("cannot start local command: {error}"))
        );
        return ExitCode::FAILURE;
    }
    match parse(&args).unwrap_or_else(|error| error.exit()) {
        Invocation::Product(Cli {
            product: Some(Product::Agent(args)),
            ..
        })
        | Invocation::LegacyAgent(args) => agent_execute(args),
        Invocation::Product(Cli {
            product: Some(Product::Sandbox(args)),
            ..
        }) => sandbox_execute(args),
        Invocation::Product(Cli {
            product: Some(Product::Completion { shell }),
            ..
        }) => completion(shell),
        Invocation::Product(Cli {
            product: Some(Product::Local(args)),
            ..
        }) => local_execute(args),
        Invocation::Product(Cli {
            product: None,
            local,
        }) => local_execute(local),
    }
}

fn completion(shell: clap_complete::Shell) -> ExitCode {
    use clap::CommandFactory;
    use clap_complete::Generator;
    let mut command = Cli::command().bin_name("hudson");
    command.build();
    match shell.try_generate(&command, &mut std::io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => {
            eprintln!("cannot write shell completion script");
            ExitCode::FAILURE
        }
    }
}

/// Compatibility entrypoint for existing `hudson-cli` scripts.
pub fn legacy_agent_entry() -> ExitCode {
    agent_execute(agent::commands::Args::parse())
}

/// Compatibility entrypoint for existing `hudsonbox` scripts.
pub fn legacy_sandbox_entry() -> ExitCode {
    sandbox_execute(sandbox::Cli::parse())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_defaults_and_explicit_api_sessions_remain_distinct() {
        let args = ["hudson"].map(OsString::from);
        assert!(matches!(
            parse(&args),
            Ok(Invocation::Product(Cli { product: None, .. }))
        ));
        let args = ["hudson", "local", "--permission", "read-only"].map(OsString::from);
        assert!(matches!(
            parse(&args),
            Ok(Invocation::Product(Cli {
                product: Some(Product::Local(local_args::LocalArgs {
                    permission: Some(local_args::Permission::ReadOnly),
                    ..
                })),
                ..
            }))
        ));
        let args = ["hudson", "--url", "http://127.0.0.1:4318"].map(OsString::from);
        assert!(matches!(parse(&args), Ok(Invocation::LegacyAgent(_))));
        let args = ["hudson", "--json"].map(OsString::from);
        assert!(matches!(parse(&args), Ok(Invocation::LegacyAgent(_))));
        let args = ["hudson", "agent"].map(OsString::from);
        assert!(matches!(
            parse(&args),
            Ok(Invocation::Product(Cli {
                product: Some(Product::Agent(_)),
                ..
            }))
        ));
    }

    #[test]
    fn namespaces_do_not_shadow_historical_agent_definition_names() {
        for name in ["analyst", "get", "health", "start", "session", "schedule"] {
            let args = ["hudson", "agent", name, "7"].map(OsString::from);
            assert!(matches!(
                parse(&args),
                Ok(Invocation::LegacyAgent(agent::commands::Args {
                    command: Some(agent::commands::Command::Agent { .. }),
                    ..
                }))
            ));
        }
        let args = [
            "hudson",
            "agent",
            "get",
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        ]
        .map(OsString::from);
        assert!(matches!(parse(&args), Ok(Invocation::Product(_))));
        let args = [
            "hudson",
            "agent",
            "schedule",
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa",
        ]
        .map(OsString::from);
        assert!(matches!(
            parse(&args),
            Ok(Invocation::Product(Cli {
                product: Product::Agent(agent::commands::Args {
                    command: Some(agent::commands::Command::Schedule { .. }),
                    ..
                }),
            }))
        ));
    }
}
