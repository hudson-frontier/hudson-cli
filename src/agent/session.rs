//! Interactive operator session. Project display grants no tool or filesystem authority.
use super::{commands::Session, follow};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::io::{self, BufRead, IsTerminal, Read, Write};

type Error = Box<dyn std::error::Error>;

#[derive(Debug)]
struct OutputFailure(io::Error);

impl std::fmt::Display for OutputFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "cannot write terminal output; inspect the outcome before retrying a mutation",
        )
    }
}

impl std::error::Error for OutputFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

/// Keep untrusted display fields on one line, including ANSI and bidi controls.
fn safe(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            if c.is_control()
                || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                c.escape_unicode().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

fn id(text: &str) -> Result<String, Error> {
    Ok(uuid::Uuid::parse_str(text)
        .map_err(|_| "expected a run or operation UUID")?
        .to_string())
}

fn request(client: &Client, base: &str, path: &str, body: Option<Value>) -> Result<Value, Error> {
    let builder = if let Some(body) = body {
        if serde_json::to_vec(&body)?.len() > 64 * 1024 {
            return Err("input exceeds 64 KiB".into());
        }
        client.post(format!("{base}{path}")).json(&body)
    } else {
        client.get(format!("{base}{path}"))
    };
    // Mutations are sent exactly once. Ambiguous errors must be reconciled explicitly.
    let response = builder.send().map_err(|_| "API unavailable; mutation outcome may be unknown. Reuse the printed request key with `start` or `reply`; inspect existing runs before other retries")?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Hudson returned {status}").into());
    }
    let mut bytes = Vec::new();
    response.take(8 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("response exceeds 8 MiB".into());
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn display(output: &mut impl Write, record: Value) -> io::Result<()> {
    if record["type"] == "event" {
        let event = &record["event"];
        writeln!(
            output,
            "[{}] {} {}",
            record["after"],
            safe(event["event_type"].as_str().unwrap_or("event")),
            safe(&event["payload"].to_string())
        )?;
    } else {
        let run = &record["run"];
        writeln!(
            output,
            "Status: {}",
            safe(run["status"].as_str().unwrap_or("unknown"))
        )?;
        if !run["wait"].is_null() {
            writeln!(output, "Waiting: {}", safe(&run["wait"].to_string()))?;
        }
        if !run["result"].is_null() {
            writeln!(output, "Result: {}", safe(&run["result"].to_string()))?;
        }
        if let Some(reason) = run["reason"].as_str() {
            writeln!(output, "Reason: {}", safe(reason))?;
        }
    }
    output.flush()
}

const HELP: &str = "Type a task to create a new run. Each task is independent.\n/open UUID | /follow | /status | /reply QUESTION_ID answer\n/approve OPERATION_UUID | /deny OPERATION_UUID | /resume | /cancel | /help | /quit\nCtrl-C exits the client; it does not cancel the run. Use /cancel explicitly.";

fn current(run: &Option<String>) -> Result<&str, Error> {
    run.as_deref()
        .ok_or_else(|| "no active run; submit a task or /open UUID".into())
}

fn action(
    client: &Client,
    base: &str,
    session: &Session,
    line: &str,
    active: &mut Option<String>,
    output: &mut impl Write,
) -> Result<bool, Error> {
    let (command, argument) = line.split_once(' ').unwrap_or((line, ""));
    match command {
        "/quit" => return Ok(false),
        "/help" => writeln!(output, "{HELP}").map_err(OutputFailure)?,
        "/open" => {
            let run = id(argument.trim())?;
            request(client, base, &format!("/runs/{run}"), None)?;
            *active = Some(run);
        }
        "/status" => {
            let value = request(client, base, &format!("/runs/{}", current(active)?), None)?;
            display(output, json!({"type":"run", "run":value})).map_err(OutputFailure)?;
        }
        "/follow" => {}
        "/resume" | "/cancel" => {
            request(
                client,
                base,
                &format!("/runs/{}/{}", current(active)?, &command[1..]),
                Some(json!({})),
            )?;
        }
        "/approve" | "/deny" => {
            let operation = id(argument.trim())?;
            // Existing API checks ownership; a session cannot expand authority.
            request(
                client,
                base,
                &format!("/operations/{operation}/approval"),
                Some(json!({"approved":command == "/approve"})),
            )?;
        }
        "/reply" => {
            let (question, answer) = argument
                .split_once(' ')
                .ok_or("usage: /reply QUESTION_ID answer")?;
            if answer.trim().is_empty() {
                return Err("answer cannot be empty".into());
            }
            let question: u64 = question
                .parse()
                .map_err(|_| "question ID must be an unsigned integer")?;
            let run = current(active)?;
            let key = uuid::Uuid::new_v4().to_string();
            writeln!(
                output,
                "Reply request key: {key} (retain for an ambiguous retry)"
            )
            .map_err(OutputFailure)?;
            output.flush().map_err(OutputFailure)?;
            request(
                client,
                base,
                &format!("/runs/{run}/input"),
                Some(json!({"input":answer,"question_id":question,"request_key":key})),
            )?;
        }
        _ if line.starts_with('/') => return Err("unknown command; use /help".into()),
        _ => {
            let key = uuid::Uuid::new_v4().to_string();
            let mut body = json!({"input":line,"request_key":key});
            if let Some(agent) = &session.agent {
                body["agent_ref"] = json!({"id":super::client::definition_name(agent)?,"version":session.agent_version.ok_or("agent version required")?});
            }
            writeln!(
                output,
                "Submission request key: {key} (retain for an ambiguous retry)"
            )
            .map_err(OutputFailure)?;
            output.flush().map_err(OutputFailure)?;
            let response = request(client, base, "/runs", Some(body))?;
            let run = id(response["run_id"]
                .as_str()
                .ok_or("submission response has no run ID")?)?;
            writeln!(output, "Run: {run}").map_err(OutputFailure)?;
            *active = Some(run);
        }
    }
    Ok(true)
}

pub fn run(client: &Client, base: &str, session: Session) -> Result<(), Error> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("interactive session requires a terminal; use start/follow for scripts".into());
    }
    if let Some(agent) = &session.agent {
        super::client::definition_name(agent)?;
    }
    let mut output = io::stdout().lock();
    writeln!(output, "Hudson • interactive local API session\nProject: {}\nCurrent directory is display context only; no files are uploaded and tools remain host-configured.", safe(&std::env::current_dir()?.display().to_string()))?;
    writeln!(
        output,
        "Connect to an existing hudson-server; /help lists controls.\n{HELP}"
    )?;
    let mut active = None;
    let mut cursor = 0;
    let input = io::stdin();
    let mut input = input.lock();
    loop {
        write!(output, "hudson> ")?;
        output.flush()?;
        // Bound allocation even for paste/input containing no newline.
        let mut bytes = Vec::new();
        let count = input
            .by_ref()
            .take(64 * 1024 + 1)
            .read_until(b'\n', &mut bytes)?;
        if count == 0 {
            break;
        }
        if bytes.len() > 64 * 1024 {
            return Err("terminal input exceeds 64 KiB".into());
        }
        let line = String::from_utf8(bytes)?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let previous = active.clone();
        match action(client, base, &session, line, &mut active, &mut output) {
            Ok(false) => break,
            Err(error) => {
                if error.downcast_ref::<OutputFailure>().is_some() {
                    return Err(error);
                }
                eprintln!("{}", safe(&error.to_string()));
                continue;
            }
            Ok(true) => {}
        }
        if active != previous {
            cursor = 0;
        }
        if line == "/status" || line == "/help" {
            continue;
        }
        if let Some(run) = &active {
            // Callback tracks emitted cursor even when a later read fails.
            let mut output_error = None;
            let result = follow::observe(
                client,
                base,
                run,
                follow::Options {
                    after: cursor,
                    page_size: 100,
                    poll_ms: 500,
                    max_retries: 3,
                    report_retries: true,
                },
                true,
                |record| {
                    display(&mut output, record.clone()).map_err(|error| {
                        output_error = Some(error);
                        "cannot write terminal output".to_owned()
                    })?;
                    if let Some(after) = record["after"].as_u64() {
                        cursor = after;
                    }
                    Ok(())
                },
            );
            if let Some(error) = output_error {
                return Err(OutputFailure(error).into());
            }
            if let Err(reason) = result {
                eprintln!("{}; /follow retries observation", safe(&reason.to_string()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    struct BrokenOutput {
        on_flush: bool,
    }

    impl Write for BrokenOutput {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if self.on_flush {
                Ok(bytes.len())
            } else {
                Err(io::ErrorKind::BrokenPipe.into())
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Err(io::ErrorKind::BrokenPipe.into())
        }
    }

    #[test]
    fn terminal_records_propagate_write_and_flush_failures() {
        for on_flush in [false, true] {
            for record in [
                json!({"type":"event","after":1,"event":{"event_type":"progress","payload":{}}}),
                json!({"type":"run","run":{"status":"completed","result":{"answer":"done"},"reason":"finished"}}),
            ] {
                assert_eq!(
                    display(&mut BrokenOutput { on_flush }, record)
                        .unwrap_err()
                        .kind(),
                    io::ErrorKind::BrokenPipe
                );
            }
        }
    }

    #[test]
    fn tasks_and_replies_require_a_written_and_flushed_request_key() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let client = Client::builder()
            .no_proxy()
            .timeout(std::time::Duration::from_millis(100))
            .build()
            .unwrap();
        for on_flush in [false, true] {
            for line in ["new task", "/reply 7 answer"] {
                let mut active = Some("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_owned());
                let error = action(
                    &client,
                    &base,
                    &Session::default(),
                    line,
                    &mut active,
                    &mut BrokenOutput { on_flush },
                )
                .unwrap_err();
                assert_eq!(
                    error.downcast_ref::<OutputFailure>().unwrap().0.kind(),
                    io::ErrorKind::BrokenPipe
                );
                assert_eq!(
                    listener.accept().unwrap_err().kind(),
                    io::ErrorKind::WouldBlock
                );
            }
        }
    }

    #[test]
    fn escapes_terminal_injection_and_bidi_but_retains_readable_text() {
        assert_eq!(safe("hello\n\tworld"), "hello\\u{a}\\u{9}world");
        assert_eq!(
            safe("\x1b]52;c;secret\x07\r\u{202e}x"),
            "\\u{1b}]52;c;secret\\u{7}\\u{d}\\u{202e}x"
        );
    }
    #[test]
    fn command_entrypoints_preserve_subcommands_and_require_pinned_agent_version() {
        use super::super::commands::{Args, Command};
        use clap::Parser;
        assert!(Args::try_parse_from(["hudson"]).unwrap().command.is_none());
        assert!(matches!(
            Args::try_parse_from(["hudson", "health"]).unwrap().command,
            Some(Command::Health)
        ));
        assert!(Args::try_parse_from(["hudson", "session", "--agent", "analyst"]).is_err());
        assert!(Args::try_parse_from([
            "hudson",
            "session",
            "--agent",
            "analyst",
            "--agent-version",
            "1"
        ])
        .is_ok());
    }
    #[test]
    fn validates_real_uuids_and_missing_session() {
        assert!(id("-").is_err());
        assert!(current(&None).is_err());
        assert_eq!(
            id("AAAAAAAA-AAAA-AAAA-AAAA-AAAAAAAAAAAA").unwrap(),
            "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
        );
    }
}
