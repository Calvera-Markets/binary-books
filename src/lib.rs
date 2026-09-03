//! Deterministic matching engine for binary prediction markets.
//!
//! One matcher (`BinaryBook`); Kalshi and Polymarket are venue adapters
//! over the same dual-bid book. See `PLAN.md`.

pub mod book;
pub mod engine;
pub mod event;
pub mod journal;
pub mod matching;
pub mod risk;
pub mod types;
pub mod venue;

pub use book::{BinaryBook, BookEvent, NewOrder, RejectReason};
pub use engine::{Engine, Place};
pub use event::{BasketQuote, Event};
pub use matching::{canonicalize, match_kind, MatchKind, Priority};
pub use risk::{reserve_notional, CashLedger, NoopRisk, RiskPort};
pub use types::{
    notional, Action, Band, BookSide, EventId, GridError, MarketId, Order, OrderId, OrderStatus,
    Outcome, OwnerId, Price, Qty, Seq, Stp, TickGrid, TimeInForce, Ts,
};
