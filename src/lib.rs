//! `tether-recall` library surface — exposes modules for integration tests.
//!
//! The binary is `wm-tether-recall`. This lib crate exposes the protocol,
//! requester, and responder modules so acceptance tests can call them directly.

#![deny(unsafe_code)]
#![warn(missing_docs, clippy::pedantic, clippy::nursery)]
// The lib exposes pub items for use by integration tests; unreachable_pub fires
// spuriously in the bin crate context but not for the intended lib usage.
#![allow(unreachable_pub)]

pub mod protocol;
pub mod requester;
pub mod responder;
