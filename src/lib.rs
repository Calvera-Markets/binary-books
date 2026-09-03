//! Deterministic matching engine for binary prediction markets.
//!
//! One matcher (`BinaryBook`); Kalshi and Polymarket are venue adapters
//! over the same dual-bid book. See `PLAN.md`.

pub mod book;
pub mod matching;
pub mod types;

pub use matching::canonicalize;
pub use types::{
    Action, BookSide, EventId, GridError, MarketId, Order, OrderId, OrderStatus, Outcome, OwnerId,
    Price, Qty, Seq, TickGrid, TimeInForce, Ts, notional,
};
