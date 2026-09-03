//! Deterministic matching engine for binary prediction markets.
//!
//! One matcher (`BinaryBook`); Kalshi and Polymarket are venue adapters
//! over the same dual-bid book. See `PLAN.md`.

pub mod types;

pub use types::{
    EventId, GridError, MarketId, OrderId, OwnerId, Price, Qty, Seq, TickGrid, Ts, notional,
};
