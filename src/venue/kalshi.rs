//! Kalshi surface: dual-bid snapshot and YES-priced `book_side` ingest.
//!
//! Does not own a book. Does not match.

use super::{depth_levels, DepthLevel};
use crate::book::{BinaryBook, BookEvent, NewOrder};
use crate::matching::MatchKind;
use crate::types::{Action, MarketId, OrderId, Outcome, OwnerId, Price, Qty, Stp, TimeInForce};

/// Public Kalshi book: yes bids and no bids, best-first. Asks are omitted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub yes: Vec<DepthLevel>,
    pub no: Vec<DepthLevel>,
}

pub fn snapshot(book: &BinaryBook) -> Snapshot {
    Snapshot {
        yes: depth_levels(book.yes_bids()),
        no: depth_levels(book.no_bids()),
    }
}

/// Kalshi `book_side`: bid ≡ YES, ask ≡ NO. `yes_price` is always YES-leg.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookSide {
    Bid,
    Ask,
}

pub fn ingest(
    book_side: BookSide,
    yes_price: Price,
    qty: Qty,
    id: OrderId,
    owner: OwnerId,
    market: MarketId,
) -> NewOrder {
    let (action, outcome) = match book_side {
        BookSide::Bid => (Action::Buy, Outcome::Yes),
        BookSide::Ask => (Action::Sell, Outcome::Yes),
    };
    NewOrder {
        id,
        owner,
        market,
        action,
        outcome,
        limit: yes_price,
        qty,
        tif: TimeInForce::Gtc,
        post_only: false,
        stp: Stp::None,
    }
}

/// Cash-pair view of a fill: one party long YES, the other long NO.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fill {
    pub taker: OrderId,
    pub maker: OrderId,
    pub yes_px: Price,
    pub no_px: Price,
    pub qty: Qty,
    pub kind: MatchKind,
}

pub fn fills(events: &[BookEvent]) -> Vec<Fill> {
    events
        .iter()
        .filter_map(|e| match e {
            BookEvent::Trade {
                taker,
                maker,
                yes_px,
                no_px,
                qty,
                kind,
            } => Some(Fill {
                taker: taker.order_id,
                maker: maker.order_id,
                yes_px: *yes_px,
                no_px: *no_px,
                qty: *qty,
                kind: *kind,
            }),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TickGrid;

    #[test]
    fn resting_buy_no_is_a_no_bid() {
        let mut book = BinaryBook::new(TickGrid::cents());
        book.place(NewOrder {
            id: OrderId::from_raw(1),
            owner: OwnerId::from_raw(1),
            market: MarketId::from_raw(1),
            action: Action::Buy,
            outcome: Outcome::No,
            limit: Price::from_ticks(40),
            qty: Qty::from_lots(7),
            tif: TimeInForce::Gtc,
            post_only: false,
            stp: Stp::None,
        });
        let snap = snapshot(&book);
        assert!(snap.yes.is_empty());
        assert_eq!(snap.no[0].price, Price::from_ticks(40));
        assert_eq!(snap.no[0].qty, Qty::from_lots(7));
    }

    #[test]
    fn ingest_ask_is_sell_yes() {
        let o = ingest(
            BookSide::Ask,
            Price::from_ticks(60),
            Qty::from_lots(1),
            OrderId::from_raw(1),
            OwnerId::from_raw(1),
            MarketId::from_raw(1),
        );
        assert_eq!(o.action, Action::Sell);
        assert_eq!(o.outcome, Outcome::Yes);
        assert_eq!(o.limit, Price::from_ticks(60));
    }
}
