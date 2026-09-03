//! Venue adapters. Kalshi and Polymarket share [`crate::BinaryBook`].
//!
//! These modules translate order vocabulary and project snapshots. They
//! must not run a second match loop.

pub mod kalshi;
pub mod polymarket;

use crate::book::{BinaryBook, Ladder};
use crate::types::{Price, Qty, TickGrid};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DepthLevel {
    pub price: Price,
    pub qty: Qty,
    pub orders: usize,
}

/// YES-priced CLOB: bids = yes bids, asks = complement(no bids), best ask first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct YesClob {
    pub bids: Vec<DepthLevel>,
    pub asks: Vec<DepthLevel>,
}

pub fn yes_clob(book: &BinaryBook) -> YesClob {
    YesClob {
        bids: depth_levels(book.yes_bids()),
        asks: complement_asks(book.no_bids(), book.grid()),
    }
}

fn depth_levels(ladder: &Ladder) -> Vec<DepthLevel> {
    ladder
        .depth()
        .into_iter()
        .map(|(price, qty, orders)| DepthLevel { price, qty, orders })
        .collect()
}

fn complement_asks(ladder: &Ladder, grid: &TickGrid) -> Vec<DepthLevel> {
    ladder
        .depth()
        .into_iter()
        .map(|(px, qty, orders)| DepthLevel {
            price: grid.complement(px),
            qty,
            orders,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BinaryBook, NewOrder};
    use crate::types::{
        Action, MarketId, OrderId, Outcome, OwnerId, Price, Qty, Stp, TickGrid, TimeInForce,
    };

    fn rest_buy_no_40() -> BinaryBook {
        let mut b = BinaryBook::new(TickGrid::cents());
        b.place(NewOrder {
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
        b
    }

    #[test]
    fn yes_clob_ask_is_complement_of_no_bid() {
        let book = rest_buy_no_40();
        let snap = yes_clob(&book);
        assert!(snap.bids.is_empty());
        assert_eq!(
            snap.asks,
            vec![DepthLevel {
                price: Price::from_ticks(60),
                qty: Qty::from_lots(7),
                orders: 1,
            }]
        );
    }
}
