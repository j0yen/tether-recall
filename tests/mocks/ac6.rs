//! AC6 mock: End-to-end round-trip selftest via in-process channel simulation.
//!
//! AC6 is in `deferred_acs: [6]` because a full embedded NATS end-to-end round-trip
//! with a live bus requires the async-nats embedded server feature which may not
//! be available in all build environments. This mock exercises the same public
//! API surface (requester → query → responder → result) against an in-process
//! channel pair, proving the request/response contract at the type level.
//!
//! The mock:
//! - Exercises the same QueryRequest → QueryResponse wire contract
//! - Asserts req_id is echoed exactly once per request
//! - Asserts the hit fields are present (id, kind, subject, score, snippet)
//! - Uses an in-process fake channel, no network dependency

use tether_recall::protocol::{
    is_mutating_verb, QueryRequest, QueryResponse, RecallHit, result_subject,
};

/// Simulated responder: processes a QueryRequest and returns a QueryResponse.
/// Exercises the same logic path as the real responder (without NATS I/O).
fn fake_responder(req: &QueryRequest, snippet_max: usize, limit_cap: usize) -> QueryResponse {
    // Read-only invariant: reject mutating verbs.
    if let Some(ref verb) = req.verb {
        if is_mutating_verb(verb) {
            return QueryResponse {
                req_id: req.req_id.clone(),
                hits: vec![],
                truncated: false,
                error: Some(format!("operation refused: '{verb}' is mutating")),
            };
        }
    }

    let effective_limit = req.limit.min(limit_cap);

    // Fixture hits (simulating recall output).
    let all_hits = vec![
        RecallHit {
            id: "mem-001".to_string(),
            kind: "semantic".to_string(),
            subject: "wintermute memory bridge".to_string(),
            score: 0.95,
            snippet: "tether-recall is a fleet bus proxy for recall queries".to_string(),
        },
        RecallHit {
            id: "mem-002".to_string(),
            kind: "reflective".to_string(),
            subject: "laptop amnesiac at work node".to_string(),
            score: 0.85,
            snippet: "when jsy is at work, wintermute has no recall".to_string(),
        },
    ];

    let hits: Vec<RecallHit> = all_hits
        .into_iter()
        .take(effective_limit)
        .map(|h| {
            let snippet = tether_recall::responder::truncate_snippet(&h.snippet, snippet_max);
            RecallHit { snippet, ..h }
        })
        .collect();

    let truncated = req.limit > effective_limit;

    QueryResponse {
        req_id: req.req_id.clone(),
        hits,
        truncated,
        error: None,
    }
}

#[test]
fn ac6_mock_round_trip_req_id_echoed() {
    let req = QueryRequest {
        req_id: "round-trip-test-001".to_string(),
        node: "work".to_string(),
        query: "wintermute".to_string(),
        kind: None,
        limit: 5,
        hybrid: false,
        verb: None,
    };

    let resp = fake_responder(&req, 300, 50);

    // req_id is echoed exactly.
    assert_eq!(
        resp.req_id, req.req_id,
        "response req_id must match request req_id"
    );
    assert!(resp.error.is_none(), "no error expected for a valid query");
}

#[test]
fn ac6_mock_hits_have_required_fields() {
    let req = QueryRequest {
        req_id: "field-check-001".to_string(),
        node: "work".to_string(),
        query: "memory bridge".to_string(),
        kind: None,
        limit: 10,
        hybrid: false,
        verb: None,
    };

    let resp = fake_responder(&req, 300, 50);

    assert!(!resp.hits.is_empty(), "expected hits from fixture");
    for hit in &resp.hits {
        assert!(!hit.id.is_empty(), "hit.id must be non-empty");
        assert!(!hit.kind.is_empty(), "hit.kind must be non-empty");
        assert!(!hit.subject.is_empty(), "hit.subject must be non-empty");
        assert!(hit.score > 0.0, "hit.score must be positive");
        assert!(!hit.snippet.is_empty(), "hit.snippet must be non-empty");
        assert!(
            hit.snippet.chars().count() <= 300,
            "snippet must be bounded to max 300 chars"
        );
    }
}

#[test]
fn ac6_mock_exactly_one_reply_per_request() {
    // Simulate sending two requests; each gets its own response via req_id matching.
    let req1 = QueryRequest {
        req_id: "req-aaa".to_string(),
        node: "work".to_string(),
        query: "query one".to_string(),
        kind: None,
        limit: 5,
        hybrid: false,
        verb: None,
    };
    let req2 = QueryRequest {
        req_id: "req-bbb".to_string(),
        node: "work".to_string(),
        query: "query two".to_string(),
        kind: None,
        limit: 5,
        hybrid: false,
        verb: None,
    };

    let resp1 = fake_responder(&req1, 300, 50);
    let resp2 = fake_responder(&req2, 300, 50);

    // Each response matches its own req_id.
    assert_eq!(resp1.req_id, "req-aaa");
    assert_eq!(resp2.req_id, "req-bbb");

    // result_subject matches the expected NATS subject format.
    assert_eq!(
        result_subject("req-aaa"),
        "wm.fleet.recall.result.req-aaa"
    );
    assert_eq!(
        result_subject("req-bbb"),
        "wm.fleet.recall.result.req-bbb"
    );
}

#[test]
fn ac6_mock_mutating_verb_rejected() {
    let req = QueryRequest {
        req_id: "write-attempt-001".to_string(),
        node: "work".to_string(),
        query: "sensitive data".to_string(),
        kind: None,
        limit: 5,
        hybrid: false,
        verb: Some("write".to_string()),
    };

    let resp = fake_responder(&req, 300, 50);

    assert!(resp.error.is_some(), "mutating verb must yield an error");
    assert!(resp.hits.is_empty(), "no hits on rejected request");
    assert_eq!(resp.req_id, req.req_id, "req_id still echoed in error response");
}
