//! Canonicalization, match-kind labels, priority, and STP.
//!
//! This module must not own ladders. The dual-bid matcher lives in `book`.

mod canon;
mod kind;
mod priority;

pub use canon::canonicalize;
pub use kind::{match_kind, MatchKind};
pub use priority::Priority;
