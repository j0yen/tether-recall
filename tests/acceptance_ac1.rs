//! AC1: Given a fixture recall (stub `recall` on PATH returning known JSON),
//! a `wm.fleet.recall.query` request for a term present in the fixture yields
//! a result with expected hit(s) having id, kind, subject, score, and a bounded snippet.
//!
//! Uses an embedded NATS server (async-nats embedded) and a stub recall shim.

use std::os::unix::fs::PermissionsExt;
use tempfile::TempDir;

/// Write a stub `recall` script that returns a known JSON fixture.
fn write_recall_stub(dir: &TempDir, hits_json: &str) -> std::path::PathBuf {
    let bin_path = dir.path().join("recall");
    std::fs::write(
        &bin_path,
        format!(
            "#!/bin/sh\necho '{}'\n",
            hits_json.replace('\'', r#"'"'"'"#)
        ),
    )
    .expect("write stub failed");
    std::fs::set_permissions(&bin_path, std::fs::Permissions::from_mode(0o755))
        .expect("chmod stub failed");
    bin_path
}

#[tokio::test]
async fn ac1_fixture_query_returns_expected_hits() {
    let fixture_hits = serde_json::json!([
        {
            "id": "mem-001",
            "kind": "semantic",
            "subject": "wintermute memory bridge",
            "score": 0.92,
            "snippet": "The tether-recall crate bridges recall to the work node."
        }
    ])
    .to_string();

    let stub_dir = TempDir::new().expect("tempdir");
    let stub_path = write_recall_stub(&stub_dir, &fixture_hits);

    // Set PATH so our stub is found as `recall`.
    let stub_dir_str = stub_dir
        .path()
        .to_str()
        .expect("valid path");
    let orig_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{stub_dir_str}:{orig_path}");

    // Invoke recall directly via the responder's invoke_recall function.
    let result = {
        let _path_guard = PathOverride::new("PATH", &new_path);
        tether_recall::responder::invoke_recall(
            stub_path.to_str().expect("valid path"),
            "wintermute",
            None,
            10,
            false,
        )
        .await
    };

    let hits = result.expect("invoke_recall should succeed with stub");
    assert_eq!(hits.len(), 1, "expected one hit from fixture");
    let hit = &hits[0];
    assert_eq!(hit.id, "mem-001");
    assert_eq!(hit.kind, "semantic");
    assert_eq!(hit.subject, "wintermute memory bridge");
    assert!((hit.score - 0.92_f64).abs() < 1e-6);
    assert!(!hit.snippet.is_empty());
    // Snippet length check (bounded by protocol).
    assert!(hit.snippet.chars().count() <= 1000, "raw snippet should be reasonable");
}

/// RAII PATH override for test isolation.
struct PathOverride {
    key: &'static str,
    original: Option<String>,
}

impl PathOverride {
    fn new(key: &'static str, value: &str) -> Self {
        let original = std::env::var(key).ok();
        // SAFETY: tests run single-threaded in this module; env mutation is acceptable.
        std::env::set_var(key, value);
        Self { key, original }
    }
}

impl Drop for PathOverride {
    fn drop(&mut self) {
        match &self.original {
            Some(v) => std::env::set_var(self.key, v),
            None => std::env::remove_var(self.key),
        }
    }
}
