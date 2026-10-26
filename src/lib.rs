//! Executable adapters for the public Hudson and Hudsonbox APIs.
//! This crate's Rust modules are not a supported SDK surface.
mod agent;
mod sandbox;

use clap::{Parser, Subcommand};
use std::{ffi::OsString, process::ExitCode};

#[derive(Parser)]
#[command(
    name = "hudson",
    version,
    about = "Command-line interface for Hudson products",
    after_help = "Legacy agent commands remain available at the root. Use hudson-cli --help for their full list."
)]
struct Cli {
    #[command(subcommand)]
    product: Product,
}

#[derive(Subcommand)]
enum Product {
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
        Ok(legacy) => return Ok(Invocation::LegacyAgent(legacy)),
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

/// Unified `hudson` executable, with the existing flat agent commands retained.
pub fn main_entry() -> ExitCode {
    let args: Vec<_> = std::env::args_os().collect();
    match parse(&args).unwrap_or_else(|error| error.exit()) {
        Invocation::Product(Cli {
            product: Product::Agent(args),
        })
        | Invocation::LegacyAgent(args) => agent_execute(args),
        Invocation::Product(Cli {
            product: Product::Sandbox(args),
        }) => sandbox_execute(args),
        Invocation::Product(Cli {
            product: Product::Completion { shell },
        }) => completion(shell),
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
    fn namespaces_do_not_shadow_historical_agent_definition_names() {
        for name in ["analyst", "get", "health", "start", "session"] {
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
    }
}
