//! Requester side — runs on the work node, publishes `wm.fleet.recall.query`
//! and waits for the result on `wm.fleet.recall.result.<req_id>`.
//!
//! Times out cleanly with a non-zero exit if no responder answers within the
//! configured timeout.

use anyhow::{Context, Result};
use futures_util::StreamExt as _;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::debug;
use uuid::Uuid;

use crate::protocol::{
    result_subject, PingRequest, PongResponse, QueryRequest, QueryResponse, SUBJECT_PING,
    SUBJECT_QUERY,
};

/// Run the `query` subcommand: publish a query request and print ranked hits.
///
/// Exits non-zero (via returned `Err`) if the timeout elapses without a response.
///
/// # Errors
/// Returns an error on NATS connection failure or on timeout.
#[allow(clippy::too_many_arguments)]
pub async fn run_query(
    nats_url: &str,
    text: &str,
    kind: Option<&str>,
    limit: usize,
    hybrid: bool,
    timeout_secs: u64,
) -> Result<()> {
    let client = async_nats::connect(nats_url)
        .await
        .with_context(|| format!("failed to connect to NATS at {nats_url}"))?;

    let req_id = Uuid::new_v4().to_string();
    let reply_subject = result_subject(&req_id);

    // Subscribe to reply subject BEFORE publishing to avoid a race.
    let mut reply_sub = client
        .subscribe(reply_subject.clone())
        .await
        .context("failed to subscribe to reply subject")?;

    let req = QueryRequest {
        req_id: req_id.clone(),
        node: hostname(),
        query: text.to_string(),
        kind: kind.map(str::to_string),
        limit,
        hybrid,
        verb: None,
    };

    let payload = serde_json::to_vec(&req).context("failed to serialize request")?;

    debug!(req_id, subject = SUBJECT_QUERY, "publishing query request");
    client
        .publish(SUBJECT_QUERY.to_string(), payload.into())
        .await
        .context("failed to publish query request")?;

    let timeout = Duration::from_secs(timeout_secs);
    let response: QueryResponse = tokio::time::timeout(timeout, async {
        loop {
            if let Some(msg) = reply_sub.next().await {
                match serde_json::from_slice::<QueryResponse>(&msg.payload) {
                    Ok(resp) if resp.req_id == req_id => return Ok(resp),
                    Ok(resp) => {
                        debug!(
                            got_req_id = %resp.req_id,
                            expected_req_id = %req_id,
                            "ignoring response for different req_id"
                        );
                    }
                    Err(e) => {
                        return Err(anyhow::anyhow!("failed to parse response: {e}"));
                    }
                }
            } else {
                return Err(anyhow::anyhow!("reply subscription closed unexpectedly"));
            }
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("timed out waiting for recall response after {timeout_secs}s"))??;

    if let Some(ref err) = response.error {
        #[allow(clippy::print_stderr)]
        {
            eprintln!("recall query error: {err}");
        }
        std::process::exit(1);
    }

    print_hits(&response);

    Ok(())
}

/// Run the `status` subcommand: ping the responder and report RTT.
///
/// # Errors
/// Returns an error on NATS connection failure or on timeout.
pub async fn run_status(nats_url: &str, timeout_secs: u64) -> Result<()> {
    let client = async_nats::connect(nats_url)
        .await
        .with_context(|| format!("failed to connect to NATS at {nats_url}"))?;

    let ping_id = Uuid::new_v4().to_string();
    let reply_subject = format!("wm.fleet.recall.pong.{ping_id}");

    let mut pong_sub = client
        .subscribe(reply_subject.clone())
        .await
        .context("failed to subscribe to pong subject")?;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock before epoch")?
        .as_millis()
        .try_into()
        .context("timestamp overflow")?;

    let ping_req = PingRequest {
        ping_id: ping_id.clone(),
        sent_at_ms: now_ms,
    };

    client
        .publish_with_reply(
            SUBJECT_PING.to_string(),
            reply_subject.clone(),
            serde_json::to_vec(&ping_req).context("serialize ping")?.into(),
        )
        .await
        .context("failed to publish ping")?;

    let timeout = Duration::from_secs(timeout_secs);
    match tokio::time::timeout(timeout, pong_sub.next()).await {
        Ok(Some(msg)) => {
            let pong_resp: PongResponse = serde_json::from_slice(&msg.payload)
                .context("failed to parse pong response")?;
            let rtt_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("system clock error")?
                .as_millis()
                .saturating_sub(u128::from(pong_resp.sent_at_ms));
            #[allow(clippy::print_stdout)]
            {
                println!(
                    "responder: ONLINE  node={node}  rtt={rtt_ms}ms",
                    node = pong_resp.node,
                );
            }
        }
        Ok(None) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("responder: OFFLINE (subscription closed)");
            }
            std::process::exit(1);
        }
        Err(_) => {
            #[allow(clippy::print_stderr)]
            {
                eprintln!("responder: OFFLINE (no response within {timeout_secs}s)");
            }
            std::process::exit(1);
        }
    }

    Ok(())
}

/// Print ranked hits in a format matching `recall query`'s columns.
///
/// Columns: SCORE  KIND  SUBJECT  SNIPPET
fn print_hits(response: &QueryResponse) {
    if response.hits.is_empty() {
        #[allow(clippy::print_stdout)]
        {
            println!("(no results)");
        }
        return;
    }
    // Header
    #[allow(clippy::print_stdout)]
    {
        println!("{:<6}  {:<12}  {:<40}  SNIPPET", "SCORE", "KIND", "SUBJECT");
        println!("{}", "-".repeat(80));
    }
    for hit in &response.hits {
        let subject = if hit.subject.chars().count() > 40 {
            let s: String = hit.subject.chars().take(39).collect();
            format!("{s}…")
        } else {
            hit.subject.clone()
        };
        #[allow(clippy::print_stdout)]
        {
            println!(
                "{score:<6.2}  {kind:<12}  {subject:<40}  {snippet}",
                score = hit.score,
                kind = hit.kind,
                subject = subject,
                snippet = hit.snippet,
            );
        }
    }
    if response.truncated {
        #[allow(clippy::print_stdout)]
        {
            println!("(results truncated by server-side limit cap)");
        }
    }
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .unwrap_or_else(|_| "unknown".to_string())
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{QueryResponse, RecallHit};

    /// Ensure print_hits doesn't panic on an empty result set.
    #[test]
    fn print_hits_empty() {
        let resp = QueryResponse {
            req_id: "x".to_string(),
            hits: vec![],
            truncated: false,
            error: None,
        };
        // No panic = pass.
        print_hits(&resp);
    }

    /// Ensure print_hits doesn't panic with a few hits.
    #[test]
    fn print_hits_with_results() {
        let resp = QueryResponse {
            req_id: "x".to_string(),
            hits: vec![RecallHit {
                id: "1".to_string(),
                kind: "semantic".to_string(),
                subject: "Test subject".to_string(),
                score: 0.95,
                snippet: "A brief snippet".to_string(),
            }],
            truncated: false,
            error: None,
        };
        print_hits(&resp);
    }
}
