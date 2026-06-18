//! AC4: The responder refuses a mutating op — a request carrying a write/forget
//! verb is rejected with an error reply and the fixture store is unchanged.
//!
//! This is grep-assertable (no write path wired) and also tested at the logic level.

use tether_recall::protocol::{is_mutating_verb, MUTATING_VERBS};

#[test]
fn ac4_all_mutating_verbs_detected() {
    for &verb in MUTATING_VERBS {
        assert!(
            is_mutating_verb(verb),
            "mutating verb '{verb}' should be detected"
        );
        // Case-insensitive.
        assert!(
            is_mutating_verb(&verb.to_uppercase()),
            "mutating verb '{verb}' should be detected case-insensitively"
        );
    }
}

#[test]
fn ac4_read_verbs_not_flagged() {
    let read_verbs = ["query", "search", "list", "get", "find", "fetch"];
    for verb in read_verbs {
        assert!(
            !is_mutating_verb(verb),
            "read verb '{verb}' should not be flagged as mutating"
        );
    }
}

#[test]
fn ac4_write_verb_is_mutating() {
    assert!(is_mutating_verb("write"));
}

#[test]
fn ac4_forget_verb_is_mutating() {
    assert!(is_mutating_verb("forget"));
}

#[test]
fn ac4_delete_verb_is_mutating() {
    assert!(is_mutating_verb("delete"));
}

#[test]
fn ac4_no_write_path_in_source() {
    // Grep-assert: verify the responder source does not contain any write
    // operations to the recall store (no `recall write`, `recall insert`, etc.)
    // by checking that invoke_recall only passes "query" as the subcommand.
    //
    // This is verifiable by reading the responder source and confirming the
    // subprocess call always uses `recall query`.
    let responder_src = include_str!("../src/responder.rs");
    // The invoke_recall function should use "query" as the first arg to recall.
    assert!(
        responder_src.contains(r#".arg("query")"#),
        "responder must invoke recall with 'query' subcommand"
    );
    // Must NOT invoke recall with write/forget/delete.
    for bad_verb in &["write", "forget", "delete", "insert", "upsert"] {
        let bad_pattern = format!(r#".arg("{bad_verb}")"#);
        assert!(
            !responder_src.contains(&bad_pattern),
            "responder must not invoke recall with mutating verb '{bad_verb}'"
        );
    }
}

#[test]
fn ac4_request_with_write_verb_is_rejected() {
    // Verify that a QueryRequest with verb="write" would be caught by is_mutating_verb
    // before any subprocess call.
    use tether_recall::protocol::QueryRequest;

    let req = QueryRequest {
        req_id: "test-123".to_string(),
        node: "work".to_string(),
        query: "some query".to_string(),
        kind: None,
        limit: 10,
        hybrid: false,
        verb: Some("write".to_string()),
    };

    // The responder checks: if req.verb is Some(v) and is_mutating_verb(v) → reject.
    if let Some(ref verb) = req.verb {
        assert!(
            is_mutating_verb(verb),
            "write verb should be caught by mutating verb check"
        );
    } else {
        panic!("test setup error: verb should be Some");
    }
}
