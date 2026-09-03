//! Winner-takes-all event: N independent binary books.
//!
//! Matching stays per book. Basket helpers walk implied asks so a later
//! NegRisk / collateral-return layer can price complete sets.

use crate::book::{BinaryBook, BookEvent, NewOrder, RejectReason};
use crate::types::{EventId, MarketId, OrderId, Price, Qty, TickGrid};
use std::collections::BTreeMap;

#[derive(Clone, Debug)]
pub struct Event {
    id: EventId,
    grid: TickGrid,
    markets: BTreeMap<MarketId, BinaryBook>,
    open: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BasketQuote {
    /// Sum of per-outcome prices in ticks for one complete set.
    pub cost_ticks: u128,
    pub max_qty: Qty,
}

impl Event {
    pub fn new(id: EventId, grid: TickGrid, markets: impl IntoIterator<Item = MarketId>) -> Self {
        let markets = markets
            .into_iter()
            .map(|m| (m, BinaryBook::new(grid.clone())))
            .collect();
        Self {
            id,
            grid,
            markets,
            open: true,
        }
    }

    pub fn id(&self) -> EventId {
        self.id
    }

    pub fn grid(&self) -> &TickGrid {
        &self.grid
    }

    pub fn market_ids(&self) -> impl Iterator<Item = MarketId> + '_ {
        self.markets.keys().copied()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    pub fn book(&self, market: MarketId) -> Option<&BinaryBook> {
        self.markets.get(&market)
    }

    pub fn book_mut(&mut self, market: MarketId) -> Option<&mut BinaryBook> {
        self.markets.get_mut(&market)
    }

    pub fn place(
        &mut self,
        market: MarketId,
        order: NewOrder,
    ) -> Result<Vec<BookEvent>, RejectReason> {
        if !self.open {
            return Err(RejectReason::Closed);
        }
        self.markets
            .get_mut(&market)
            .map(|b| b.place(order))
            .ok_or(RejectReason::NotFound)
    }

    pub fn cancel(
        &mut self,
        market: MarketId,
        id: OrderId,
    ) -> Result<Vec<BookEvent>, RejectReason> {
        self.markets
            .get_mut(&market)
            .map(|b| b.cancel(id))
            .ok_or(RejectReason::NotFound)
    }

    /// Executable cost of one YES of every outcome (complete YES basket).
    pub fn yes_ask_basket(&self) -> Option<BasketQuote> {
        self.basket(|book| {
            let no = book.no_bids().best()?;
            let ask = book.grid().complement(no);
            let qty = book.no_bids().best_qty()?;
            Some((ask, qty))
        })
    }

    /// Executable cost of one NO of every outcome.
    pub fn no_ask_basket(&self) -> Option<BasketQuote> {
        self.basket(|book| {
            let yes = book.yes_bids().best()?;
            let ask = book.grid().complement(yes);
            let qty = book.yes_bids().best_qty()?;
            Some((ask, qty))
        })
    }

    fn basket(&self, leg: impl Fn(&BinaryBook) -> Option<(Price, Qty)>) -> Option<BasketQuote> {
        if self.markets.is_empty() {
            return None;
        }
        let mut cost = 0u128;
        let mut max_qty: Option<Qty> = None;
        for book in self.markets.values() {
            let (px, qty) = leg(book)?;
            cost += u128::from(px.ticks());
            max_qty = Some(match max_qty {
                None => qty,
                Some(q) => q.min(qty),
            });
        }
        Some(BasketQuote {
            cost_ticks: cost,
            max_qty: max_qty?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, OrderId, Outcome, OwnerId, Stp, TimeInForce};

    fn oid(n: u64) -> OrderId {
        OrderId::from_raw(n)
    }
    fn mid(n: u64) -> MarketId {
        MarketId::from_raw(n)
    }
    fn p(t: u32) -> Price {
        Price::from_ticks(t)
    }
    fn q(n: u64) -> Qty {
        Qty::from_lots(n)
    }

    fn order(id: u64, market: u64, outcome: Outcome, limit: u32, qty: u64) -> NewOrder {
        NewOrder {
            id: oid(id),
            owner: OwnerId::from_raw(id),
            market: mid(market),
            action: Action::Buy,
            outcome,
            limit: p(limit),
            qty: q(qty),
            tif: TimeInForce::Gtc,
            post_only: false,
            stp: Stp::None,
        }
    }

    fn three() -> Event {
        Event::new(
            EventId::from_raw(1),
            TickGrid::cents(),
            [mid(1), mid(2), mid(3)],
        )
    }

    #[test]
    fn matching_is_per_market() {
        let mut e = three();
        e.place(mid(1), order(1, 1, Outcome::No, 40, 10)).unwrap();
        let ev = e.place(mid(2), order(2, 2, Outcome::Yes, 60, 10)).unwrap();
        assert!(ev.iter().all(|x| !matches!(x, BookEvent::Trade { .. })));
        assert!(e.book(mid(1)).unwrap().no_bids().best() == Some(p(40)));
        assert!(e.book(mid(2)).unwrap().yes_bids().best() == Some(p(60)));
    }

    #[test]
    fn yes_basket_uses_implied_asks() {
        let mut e = three();
        e.place(mid(1), order(1, 1, Outcome::No, 40, 10)).unwrap();
        e.place(mid(2), order(2, 2, Outcome::No, 50, 5)).unwrap();
        e.place(mid(3), order(3, 3, Outcome::No, 30, 8)).unwrap();
        let qte = e.yes_ask_basket().unwrap();
        // asks = 60 + 50 + 70 = 180 ticks, max_qty = 5
        assert_eq!(qte.cost_ticks, 180);
        assert_eq!(qte.max_qty, q(5));
    }

    #[test]
    fn unknown_market_is_not_found() {
        let mut e = three();
        assert_eq!(
            e.place(mid(9), order(1, 9, Outcome::Yes, 50, 1)),
            Err(RejectReason::NotFound)
        );
    }

    #[test]
    fn basket_none_if_a_leg_has_no_ask() {
        let mut e = three();
        e.place(mid(1), order(1, 1, Outcome::No, 40, 1)).unwrap();
        assert!(e.yes_ask_basket().is_none());
    }
}
