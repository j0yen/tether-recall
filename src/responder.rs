//! Responder side — runs on the laptop, subscribes to `wm.fleet.recall.query`,
//! invokes the local `recall` CLI as a subprocess (no shell), and publishes
//! results on `wm.fleet.recall.result.<req_id>`.
//!
//! # Safety contract
//! - The responder only ever invokes `recall` with read-only operations.
//! - Mutating verbs in the request are rejected before any subprocess call.
//! - Subprocess arguments are built programmatically (no shell, no interpolation).

use anyhow::{Context, Result};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command as TokioCommand;
use tracing::{error, info, warn};

use crate::protocol::{
    is_mutating_verb, result_subject, PingRequest, PongResponse, QueryRequest, QueryResponse,
    RecallHit, SUBJECT_PING, SUBJECT_QUERY,
};

/// Maximum subprocess execution time.
const SUBPROCESS_TIMEOUT: Duration = Duration::from_secs(30);

/// Run the responder daemon. Connects to NATS and serves recall queries.
///
/// # Errors
/// Returns an error if the NATS connection fails or if the event loop crashes.
pub async fn run_serve(
    nats_url: &str,
    recall_bin: Option<&str>,
    snippet_max: usize,
    limit_cap: usize,
) -> Result<()> {
    let recall_binary = resolve_recall_bin(recall_bin)?;
    info!(nats_url, recall_binary, "responder starting");

    let client = async_nats::connect(nats_url)
        .await
        .with_context(|| format!("failed to connect to NATS at {nats_url}"))?;

    let mut query_sub = client
        .subscribe(SUBJECT_QUERY)
        .await
        .context("failed to subscribe to query subject")?;

    let mut ping_sub = client
        .subscribe(SUBJECT_PING)
        .await
        .context("failed to subscribe to ping subject")?;

    info!("responder ready");

    let hostname = hostname();

    loop {
        tokio::select! {
            Some(msg) = query_sub.next() => {
                let client = client.clone();
                let recall_binary = recall_binary.clone();
                let hostname = hostname.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_query(msg, &client, &recall_binary, snippet_max, limit_cap, &hostname).await {
                        error!(error = %e, "query handler error");
                    }
                });
            }
            Some(msg) = ping_sub.next() => {
                let client = client.clone();
                let hostname = hostname.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_ping(msg, &client, &hostname).await {
                        error!(error = %e, "ping handler error");
                    }
                });
            }
            else => break,
        }
    }

    Ok(())
}

async fn handle_query(
    msg: async_nats::Message,
    client: &async_nats::Client,
    recall_binary: &str,
    snippet_max: usize,
    limit_cap: usize,
    hostname: &str,
) -> Result<()> {
    let req: QueryRequest = match serde_json::from_slice(&msg.payload) {
        Ok(r) => r,
        Err(e) => {
            warn!(error = %e, "failed to parse query request");
            return Ok(());
        }
    };

    info!(req_id = %req.req_id, query = %req.query, "handling query");

    let reply_subject = result_subject(&req.req_id);

    // Refuse mutating verbs (read-only invariant).
    if let Some(ref verb) = req.verb {
        if is_mutating_verb(verb) {
            warn!(req_id = %req.req_id, verb, "rejecting mutating verb");
            let response = QueryResponse {
                req_id: req.req_id.clone(),
                hits: vec![],
                truncated: false,
                error: Some(format!(
                    "operation refused: '{verb}' is a mutating verb; tether-recall is read-only in v1"
                )),
            };
            publish_response(client, &reply_subject, &response, hostname).await?;
            return Ok(());
        }
    }

    // Clamp limit.
    let effective_limit = req.limit.min(limit_cap);

    // Invoke recall CLI (no shell, arg-built).
    match invoke_recall(recall_binary, &req.query, req.kind.as_deref(), effective_limit, req.hybrid).await {
        Ok(raw_hits) => {
            let hits: Vec<RecallHit> = raw_hits
                .into_iter()
                .take(effective_limit)
                .map(|h| RecallHit {
                    id: h.id,
                    kind: h.kind,
                    subject: h.subject,
                    score: h.score,
                    snippet: truncate_snippet(&h.snippet, snippet_max),
                })
                .collect();
            let truncated = hits.len() < req.limit.min(limit_cap + 1) && req.limit > effective_limit;
            let response = QueryResponse {
                req_id: req.req_id,
                hits,
                truncated,
                error: None,
            };
            publish_response(client, &reply_subject, &response, hostname).await?;
        }
        Err(e) => {
            error!(req_id = %req.req_id, error = %e, "recall invocation failed");
            let response = QueryResponse {
                req_id: req.req_id,
                hits: vec![],
                truncated: false,
                error: Some(format!("recall invocation failed: {e}")),
            };
            publish_response(client, &reply_subject, &response, hostname).await?;
        }
    }

    Ok(())
}

async fn handle_ping(
    msg: async_nats::Message,
    client: &async_nats::Client,
    hostname: &str,
) -> Result<()> {
    let ping: PingRequest = match serde_json::from_slice(&msg.payload) {
        Ok(p) => p,
        Err(e) => {
            warn!(error = %e, "failed to parse ping");
            return Ok(());
        }
    };

    let pong = PongResponse {
        ping_id: ping.ping_id,
        sent_at_ms: ping.sent_at_ms,
        node: hostname.to_string(),
    };

    let payload = serde_json::to_vec(&pong).context("failed to serialize pong")?;
    // Reply on the reply subject if set, else on a fixed pong subject.
    let reply_subject = msg
        .reply
        .as_deref()
        .unwrap_or("wm.fleet.recall.pong")
        .to_string();
    client
        .publish(reply_subject, payload.into())
        .await
        .context("failed to publish pong")?;

    Ok(())
}

async fn publish_response(
    client: &async_nats::Client,
    subject: &str,
    response: &QueryResponse,
    _hostname: &str,
) -> Result<()> {
    let payload = serde_json::to_vec(response).context("failed to serialize response")?;
    client
        .publish(subject.to_string(), payload.into())
        .await
        .context("failed to publish response")?;
    Ok(())
}

/// A raw hit as parsed from recall CLI JSON output.
#[derive(Debug, serde::Deserialize)]
struct RawHit {
    id: String,
    kind: String,
    subject: String,
    #[serde(default)]
    score: f64,
    #[serde(default)]
    snippet: String,
}

/// Invoke the `recall` CLI as a subprocess (no shell, arg-built) and return parsed hits.
///
/// # Errors
/// Returns an error if the subprocess fails to start, times out, or outputs invalid JSON.
pub async fn invoke_recall(
    recall_binary: &str,
    query: &str,
    kind: Option<&str>,
    limit: usize,
    hybrid: bool,
) -> Result<Vec<RawHit>> {
    let mut cmd = TokioCommand::new(recall_binary);
    // Build args programmatically — no shell, no string interpolation.
    cmd.arg("query")
        .arg(query)
        .arg("--format")
        .arg("json")
        .arg("--limit")
        .arg(limit.to_string());

    if let Some(k) = kind {
        cmd.arg("--kind").arg(k);
    }

    if hybrid {
        cmd.arg("--hybrid");
    }

    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd.spawn().context("failed to spawn recall subprocess")?;

    let stdout_handle = child.stdout.take().context("failed to get stdout")?;

    let timeout_result = tokio::time::timeout(SUBPROCESS_TIMEOUT, async move {
        let mut stdout = stdout_handle;
        let mut buf = Vec::new();
        stdout
            .read_to_end(&mut buf)
            .await
            .context("failed to read recall stdout")?;
        child.wait().await.context("failed to wait for recall")?;
        Ok::<Vec<u8>, anyhow::Error>(buf)
    })
    .await;

    let output = match timeout_result {
        Ok(Ok(buf)) => buf,
        Ok(Err(e)) => return Err(e),
        Err(_) => anyhow::bail!("recall subprocess timed out after {SUBPROCESS_TIMEOUT:?}"),
    };

    // Parse JSON array of hits.
    let hits: Vec<RawHit> = serde_json::from_slice(&output)
        .with_context(|| format!("recall returned invalid JSON: {:?}", String::from_utf8_lossy(&output)))?;

    Ok(hits)
}

/// Truncate a snippet to `max_chars`, appending `…` if truncated.
#[must_use]
pub fn truncate_snippet(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{truncated}…")
    }
}

/// Resolve the path to the `recall` binary.
fn resolve_recall_bin(override_path: Option<&str>) -> Result<String> {
    if let Some(path) = override_path {
        return Ok(path.to_string());
    }
    // Search PATH for `recall`.
    if let Ok(path) = which_recall() {
        return Ok(path);
    }
    // Common install locations.
    for candidate in &[
        "/home/jsy/.local/bin/recall",
        "/usr/local/bin/recall",
        "/usr/bin/recall",
    ] {
        if std::path::Path::new(candidate).exists() {
            return Ok((*candidate).to_string());
        }
    }
    anyhow::bail!(
        "could not find `recall` binary on PATH or common locations; \
        set WM_RECALL_BIN or pass --recall-bin"
    )
}

fn which_recall() -> Result<String> {
    let output = std::process::Command::new("which")
        .arg("recall")
        .output()
        .context("failed to run `which recall`")?;
    if output.status.success() {
        let path = String::from_utf8(output.stdout)
            .context("which output not UTF-8")?
            .trim()
            .to_string();
        Ok(path)
    } else {
        anyhow::bail!("`recall` not found on PATH")
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

    #[test]
    fn truncate_snippet_short() {
        assert_eq!(truncate_snippet("hello", 100), "hello");
    }

    #[test]
    fn truncate_snippet_exact() {
        assert_eq!(truncate_snippet("hello", 5), "hello");
    }

    #[test]
    fn truncate_snippet_long() {
        let s = "a".repeat(200);
        let result = truncate_snippet(&s, 10);
        assert!(result.chars().count() <= 10);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncate_snippet_unicode() {
        // 5 emoji = 5 chars
        let s = "😀😀😀😀😀extra";
        let result = truncate_snippet(s, 5);
        assert!(result.chars().count() <= 5);
    }
}
