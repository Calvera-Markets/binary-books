//! Deterministic matching engine for binary prediction markets.
//!
//! One matcher (`BinaryBook`); Kalshi and Polymarket are venue adapters
//! over the same dual-bid book. See `PLAN.md`.

pub mod book;
pub mod matching;
pub mod types;
pub mod venue;

pub use book::{BinaryBook, BookEvent, NewOrder, RejectReason};
pub use matching::{MatchKind, canonicalize, match_kind};
pub use types::{
    Action, Band, BookSide, EventId, GridError, MarketId, Order, OrderId, OrderStatus, Outcome,
    OwnerId, Price, Qty, Seq, Stp, TickGrid, TimeInForce, Ts, notional,
};
