//! Cursor-based observation only: following never resumes, approves, or cancels a run.
use super::client::{HttpFailure, ProtocolFailure, TransportFailure};
use reqwest::blocking::Client;
use serde_json::{json, Value};
use std::{io::Write, time::Duration};

type Error = Box<dyn std::error::Error>;

#[derive(Debug)]
struct ObservationFailure {
    source: Error,
    after: u64,
}

impl std::fmt::Display for ObservationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}; reconnect with --after {}",
            self.source, self.after
        )
    }
}

impl std::error::Error for ObservationFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.source.as_ref())
    }
}

pub struct Options {
    pub after: u64,
    pub page_size: u64,
    pub poll_ms: u64,
    pub max_retries: u32,
    pub report_retries: bool,
}

enum FetchError {
    Retryable(Error),
    Fatal(Error),
}

fn fetch(client: &Client, url: &str) -> Result<Value, FetchError> {
    use std::io::Read;
    let response = client.get(url).send().map_err(|_| {
        FetchError::Retryable(TransportFailure("follow request unavailable").into())
    })?;
    let status = response.status();
    if status.is_server_error() || status.as_u16() == 429 {
        return Err(FetchError::Retryable(HttpFailure(status).into()));
    }
    if !status.is_success() {
        // Do not print an error body or URL that could echo authentication data.
        return Err(FetchError::Fatal(HttpFailure(status).into()));
    }
    let mut bytes = Vec::new();
    response
        .take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| {
            FetchError::Retryable(
                TransportFailure("follow response could not be read completely").into(),
            )
        })?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(FetchError::Fatal(
            ProtocolFailure("follow response exceeds 8 MiB").into(),
        ));
    }
    serde_json::from_slice(&bytes)
        .map_err(|_| FetchError::Fatal(ProtocolFailure("invalid JSON in follow response").into()))
}

fn fetch_with_retry(
    client: &Client,
    url: &str,
    options: &Options,
    after: u64,
) -> Result<Value, Error> {
    for attempt in 0..=options.max_retries {
        match fetch(client, url) {
            Ok(value) => return Ok(value),
            Err(FetchError::Fatal(reason)) => return Err(reason),
            Err(FetchError::Retryable(_)) if attempt < options.max_retries => {
                let delay = options.poll_ms.saturating_mul(1_u64 << attempt).min(30_000);
                if options.report_retries {
                    eprintln!("follow: retrying unavailable server in {delay} ms; after={after}");
                }
                std::thread::sleep(Duration::from_millis(delay));
            }
            Err(FetchError::Retryable(error)) => return Err(error),
        }
    }
    unreachable!("bounded retry loop always returns")
}

fn page(value: Value, after: u64, limit: u64, run_id: &str) -> Result<Vec<Value>, &'static str> {
    let events = value.as_array().ok_or("event page must be an array")?;
    if events.len() as u64 > limit {
        return Err("event page exceeds requested limit");
    }
    let mut previous = after;
    for event in events {
        let sequence = event["sequence"]
            .as_u64()
            .ok_or("event sequence must be an unsigned integer")?;
        if sequence <= previous || event["run_id"].as_str() != Some(run_id) {
            return Err("event page has invalid ordering or run identity");
        }
        previous = sequence;
    }
    Ok(events.clone())
}

fn emit(output: &mut impl Write, record: Value) -> Result<(), String> {
    serde_json::to_writer(&mut *output, &record).map_err(|_| "cannot write follow output")?;
    writeln!(output).map_err(|_| "cannot write follow output")?;
    output
        .flush()
        .map_err(|_| "cannot flush follow output".into())
}

pub fn run(client: &Client, base: &str, run_id: &str, options: Options) -> Result<(), Error> {
    let mut output = std::io::stdout().lock();
    observe(client, base, run_id, options, false, |record| {
        emit(&mut output, record)
    })
    .map(|_| ())
}

/// Shared bounded observer; interactive sessions yield at waits without taking action.
pub fn observe(
    client: &Client,
    base: &str,
    run_id: &str,
    options: Options,
    stop_on_wait: bool,
    mut output: impl FnMut(Value) -> Result<(), String>,
) -> Result<u64, Error> {
    let mut after = options.after;
    let mut previous_run = Value::Null;
    let result = (|| -> Result<u64, Error> {
        loop {
            // Observe status BEFORE draining: terminal status guarantees its
            // committed final events exist before the following event requests.
            let run = fetch_with_retry(client, &format!("{base}/runs/{run_id}"), &options, after)?;
            if run["id"]
                .as_str()
                .is_none_or(|id| !id.eq_ignore_ascii_case(run_id))
            {
                return Err(ProtocolFailure("run response has invalid identity").into());
            }
            let status = run["status"]
                .as_str()
                .ok_or(ProtocolFailure("run status is missing"))?;
            if !matches!(
                status,
                "queued"
                    | "running"
                    | "waiting"
                    | "cancelling"
                    | "completed"
                    | "failed"
                    | "cancelled"
            ) {
                return Err(ProtocolFailure("unknown run status").into());
            }
            loop {
                let value = fetch_with_retry(
                    client,
                    &format!(
                        "{base}/runs/{run_id}/events?after={after}&limit={}",
                        options.page_size
                    ),
                    &options,
                    after,
                )?;
                let events =
                    page(value, after, options.page_size, run_id).map_err(ProtocolFailure)?;
                if events.is_empty() {
                    break;
                }
                for event in events {
                    let sequence = event["sequence"].as_u64().expect("validated sequence");
                    output(json!({"type":"event", "after":sequence, "event":event}))?;
                    after = sequence;
                }
            }
            if run != previous_run {
                output(json!({"type":"run", "after":after, "run":run}))?;
                previous_run = run.clone();
            }
            match status {
                "completed" => return Ok(after),
                "waiting" if stop_on_wait => return Ok(after),
                "failed" | "cancelled" => return Err(format!("run {status}").into()),
                _ => std::thread::sleep(Duration::from_millis(options.poll_ms)),
            }
        }
    })();
    result.map_err(|source| -> Error { Box::new(ObservationFailure { source, after }) })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_nonmonotonic_and_foreign_pages_before_output() {
        for value in [
            json!([{"sequence":2,"run_id":"a"},{"sequence":2,"run_id":"a"}]),
            json!([{"sequence":1,"run_id":"a"}]),
            json!([{"sequence":2,"run_id":"other"}]),
            json!([{"sequence":-1,"run_id":"a"}]),
            json!({"error":"not a page"}),
        ] {
            assert!(page(value, 1, 2, "a").is_err());
        }
        assert!(page(
            json!([{"sequence":2,"run_id":"a"},{"sequence":3,"run_id":"a"}]),
            1,
            1,
            "a"
        )
        .is_err());
        assert_eq!(
            page(json!([]), u64::MAX, 2, "a").unwrap(),
            Vec::<Value>::new()
        );
    }
}
