//! Protocol types for the `wm.fleet.recall.*` NATS subjects.
//!
//! All messages are JSON-encoded. The subjects are:
//! - `wm.fleet.recall.query` — requester → responder
//! - `wm.fleet.recall.result.<req_id>` — responder → requester
//! - `wm.fleet.recall.ping` — requester → responder (for status check)
//! - `wm.fleet.recall.pong` — responder → requester (for status check)
//!
//! Read-only invariant: the responder only supports query/list operations.
//! Any request carrying a mutating verb is rejected with an error reply.

use serde::{Deserialize, Serialize};

/// Default query result limit (requester default, capped server-side at `LIMIT_CAP`).
pub const DEFAULT_LIMIT: usize = 10;

/// Default requester timeout in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// NATS subject for incoming query requests.
pub const SUBJECT_QUERY: &str = "wm.fleet.recall.query";

/// NATS subject prefix for query results (appended with `.<req_id>`).
pub const SUBJECT_RESULT_PREFIX: &str = "wm.fleet.recall.result";

/// NATS subject for ping requests (status check).
pub const SUBJECT_PING: &str = "wm.fleet.recall.ping";

/// A query request published by the requester on `wm.fleet.recall.query`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryRequest {
    /// Unique request identifier (`UUIDv4`).
    pub req_id: String,
    /// Originating node name (informational).
    pub node: String,
    /// The query text.
    pub query: String,
    /// Optional memory kind filter (reflective / semantic / procedural / …).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Maximum number of hits requested (requester preference; server may cap lower).
    #[serde(default = "default_limit")]
    pub limit: usize,
    /// Whether to use hybrid (FTS5 + vector) search.
    #[serde(default)]
    pub hybrid: bool,
    /// Optional operation verb — must be absent or one of the allowed read verbs.
    /// Any mutating verb (write, forget, delete, …) will be rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verb: Option<String>,
}

/// A single recall hit included in a query response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecallHit {
    /// Internal recall entry ID.
    pub id: String,
    /// Memory kind (reflective / semantic / procedural / …).
    pub kind: String,
    /// Entry subject / title.
    pub subject: String,
    /// Relevance score (higher is better).
    pub score: f64,
    /// Truncated content snippet (never exceeds server-configured max length).
    pub snippet: String,
}

/// A query response published by the responder on `wm.fleet.recall.result.<req_id>`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryResponse {
    /// Echo of the originating request ID.
    pub req_id: String,
    /// Ranked list of recall hits (may be empty).
    pub hits: Vec<RecallHit>,
    /// True when the hit list was clamped by the server-side limit cap.
    pub truncated: bool,
    /// Present when the request was rejected (e.g. mutating verb).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A ping request for the status subcommand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PingRequest {
    /// Unique ping ID.
    pub ping_id: String,
    /// Unix timestamp millis when the ping was sent.
    pub sent_at_ms: u64,
}

/// A pong response from the responder.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PongResponse {
    /// Echo of the ping ID.
    pub ping_id: String,
    /// Unix timestamp millis when the ping was sent (echoed for RTT computation).
    pub sent_at_ms: u64,
    /// Responder node name.
    pub node: String,
}

/// Verbs that are allowed on the read path.
#[allow(dead_code)]
pub const ALLOWED_VERBS: &[&str] = &["query", "search", "list", "get", "find"];

/// Verbs that are always rejected (mutating operations).
pub const MUTATING_VERBS: &[&str] = &[
    "write", "forget", "delete", "remove", "insert", "update", "upsert", "add", "put", "set",
    "patch", "replace", "clear", "reset", "drop",
];

/// Returns `true` when the given verb is a mutating operation that must be refused.
#[must_use]
pub fn is_mutating_verb(verb: &str) -> bool {
    let lower = verb.to_lowercase();
    MUTATING_VERBS.iter().any(|&mv| lower == mv || lower.starts_with(mv))
}

const fn default_limit() -> usize {
    DEFAULT_LIMIT
}

/// Result subject for a given request ID.
#[must_use]
pub fn result_subject(req_id: &str) -> String {
    format!("{SUBJECT_RESULT_PREFIX}.{req_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutating_verb_detection() {
        assert!(is_mutating_verb("write"));
        assert!(is_mutating_verb("forget"));
        assert!(is_mutating_verb("delete"));
        assert!(is_mutating_verb("WRITE"));
        assert!(!is_mutating_verb("query"));
        assert!(!is_mutating_verb("search"));
        assert!(!is_mutating_verb("list"));
    }

    #[test]
    fn result_subject_format() {
        let subj = result_subject("test-id-123");
        assert_eq!(subj, "wm.fleet.recall.result.test-id-123");
    }

    #[test]
    fn query_request_serde_roundtrip() {
        let req = QueryRequest {
            req_id: "abc".to_string(),
            node: "work".to_string(),
            query: "search text".to_string(),
            kind: Some("semantic".to_string()),
            limit: 5,
            hybrid: false,
            verb: None,
        };
        let json = serde_json::to_string(&req).expect("serialization should succeed");
        let decoded: QueryRequest =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(decoded.req_id, "abc");
        assert_eq!(decoded.limit, 5);
    }
}
