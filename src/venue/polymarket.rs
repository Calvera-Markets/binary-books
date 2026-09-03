//! Polymarket surface: per-token bid/ask snapshot and token ingest.
//!
//! Does not own a book. Does not match.

use super::{DepthLevel, complement_asks, depth_levels};
use crate::book::{BinaryBook, BookEvent, NewOrder};
use crate::matching::MatchKind;
use crate::types::{
    Action, MarketId, OrderId, Outcome, OwnerId, Price, Qty, Stp, TimeInForce,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub outcome: Outcome,
    pub bids: Vec<DepthLevel>,
    pub asks: Vec<DepthLevel>,
}

pub fn snapshot(book: &BinaryBook, outcome: Outcome) -> Snapshot {
    match outcome {
        Outcome::Yes => Snapshot {
            outcome,
            bids: depth_levels(book.yes_bids()),
            asks: complement_asks(book.no_bids(), book.grid()),
        },
        Outcome::No => Snapshot {
            outcome,
            bids: depth_levels(book.no_bids()),
            asks: complement_asks(book.yes_bids(), book.grid()),
        },
    }
}

pub fn ingest(
    outcome: Outcome,
    action: Action,
    price: Price,
    qty: Qty,
    id: OrderId,
    owner: OwnerId,
    market: MarketId,
) -> NewOrder {
    NewOrder {
        id,
        owner,
        market,
        action,
        outcome,
        limit: price,
        qty,
        tif: TimeInForce::Gtc,
        post_only: false,
        stp: Stp::None,
    }
}

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
                taker: *taker,
                maker: *maker,
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
    use crate::venue::kalshi;

    fn rest_buy_no_40() -> BinaryBook {
        let mut book = BinaryBook::new(TickGrid::cents());
        book.place(ingest(
            Outcome::No,
            Action::Buy,
            Price::from_ticks(40),
            Qty::from_lots(7),
            OrderId::from_raw(1),
            OwnerId::from_raw(1),
            MarketId::from_raw(1),
        ));
        book
    }

    #[test]
    fn buy_no_40_is_yes_ask_60_and_no_bid_40() {
        let book = rest_buy_no_40();
        let yes = snapshot(&book, Outcome::Yes);
        let no = snapshot(&book, Outcome::No);
        assert!(yes.bids.is_empty());
        assert_eq!(yes.asks[0].price, Price::from_ticks(60));
        assert_eq!(yes.asks[0].qty, Qty::from_lots(7));
        assert_eq!(no.bids[0].price, Price::from_ticks(40));
        assert!(no.asks.is_empty());
    }

    #[test]
    fn kalshi_and_polymarket_snapshots_are_complements() {
        let book = rest_buy_no_40();
        let k = kalshi::snapshot(&book);
        let yes = snapshot(&book, Outcome::Yes);
        assert_eq!(k.no[0].qty, yes.asks[0].qty);
        assert_eq!(k.no[0].orders, yes.asks[0].orders);
        assert_eq!(
            k.no[0].price.ticks() + yes.asks[0].price.ticks(),
            book.grid().collat().ticks()
        );
    }
}
