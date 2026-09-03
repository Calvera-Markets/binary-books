//! Canonicalization, match-kind labels, priority, and STP.
//!
//! This module must not own ladders. The dual-bid matcher lives in `book`.

mod canon;

pub use canon::canonicalize;
