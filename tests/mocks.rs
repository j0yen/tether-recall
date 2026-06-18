//! Mock tests for deferred acceptance criteria.
//!
//! These tests exercise the same public API surface as real ACs but against
//! in-process fakes or stubs rather than real hardware/network dependencies.
//! See autobuilder SKILL.md "Hardware mock convention".
//!
//! Deferred ACs: [6]
//!
//! Note: ac6 mock tests are in `tests/mocks_ac6.rs` (a sibling integration
//! test binary) because Rust's integration test runner treats each file in
//! `tests/` as a separate binary; subdirectory modules require a mod.rs entry
//! which is not compatible with the flat integration test layout. The
//! `tests/mocks/ac6.rs` path is preserved as documentation of the
//! hardware-mock convention; `tests/mocks_ac6.rs` is the runnable test.

