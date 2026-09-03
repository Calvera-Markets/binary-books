//! Matcher + token ledger. Locks resting size in `live` so a second sell
//! cannot spend the same shares.

use crate::book::{BookEvent, RejectReason};
use crate::engine::{Engine, Place};
use crate::inventory::TokenLedger;
use crate::notional;
use crate::risk::reserve_notional;
use crate::types::{Action, EventId, MarketId, OrderId, Outcome, OwnerId, Price, Qty, TickGrid};
use std::collections::HashMap;

struct Live {
    owner: OwnerId,
    market: MarketId,
    action: Action,
    outcome: Outcome,
    limit: Price,
    remaining: Qty,
}

pub struct Exchange {
    engine: Engine,
    ledger: TokenLedger,
    live: HashMap<OrderId, Live>,
}

impl Exchange {
    pub fn new(min_order_size: Qty, grid: TickGrid) -> Self {
        Self {
            engine: Engine::new(min_order_size),
            ledger: TokenLedger::new(grid),
            live: HashMap::new(),
        }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub fn ledger(&self) -> &TokenLedger {
        &self.ledger
    }

    pub fn open_event(
        &mut self,
        id: EventId,
        markets: impl IntoIterator<Item = MarketId>,
    ) -> Result<(), RejectReason> {
        let grid = self.ledger.grid().clone();
        self.engine.open_event(id, grid, markets)
    }

    pub fn credit_cash(&mut self, owner: OwnerId, amount: u128) {
        self.ledger.credit_cash(owner, amount);
    }

    pub fn split(
        &mut self,
        owner: OwnerId,
        market: MarketId,
        qty: Qty,
    ) -> Result<(), RejectReason> {
        self.ledger.split(owner, market, qty)
    }

    pub fn merge(
        &mut self,
        owner: OwnerId,
        market: MarketId,
        qty: Qty,
    ) -> Result<(), RejectReason> {
        self.ledger.merge(owner, market, qty)
    }

    pub fn place(
        &mut self,
        event: EventId,
        market: MarketId,
        spec: Place,
    ) -> Result<Vec<BookEvent>, RejectReason> {
        self.check_collateral(market, &spec)?;
        let events = self.engine.place(event, market, spec.clone())?;
        if events
            .first()
            .is_some_and(|e| matches!(e, BookEvent::Rejected { .. }))
        {
            return Ok(events);
        }
        if let BookEvent::Accepted { order_id, .. } = events[0] {
            self.live.insert(
                order_id,
                Live {
                    owner: spec.owner,
                    market,
                    action: spec.action,
                    outcome: spec.outcome,
                    limit: spec.limit,
                    remaining: spec.qty,
                },
            );
        }
        self.ledger.apply_events(market, &events)?;
        self.follow(&events);
        Ok(events)
    }

    pub fn cancel(
        &mut self,
        event: EventId,
        market: MarketId,
        id: OrderId,
    ) -> Result<Vec<BookEvent>, RejectReason> {
        let events = self.engine.cancel(event, market, id)?;
        self.follow(&events);
        Ok(events)
    }

    pub fn reduce(
        &mut self,
        event: EventId,
        market: MarketId,
        id: OrderId,
        new_qty: Qty,
    ) -> Result<Vec<BookEvent>, RejectReason> {
        let events = self.engine.reduce(event, market, id, new_qty)?;
        self.follow(&events);
        Ok(events)
    }

    fn check_collateral(&self, market: MarketId, spec: &Place) -> Result<(), RejectReason> {
        match spec.action {
            Action::Buy => {
                let need = reserve_notional(
                    self.ledger.grid(),
                    spec.action,
                    spec.outcome,
                    spec.limit,
                    spec.qty,
                );
                if self.free_cash(spec.owner) < need {
                    return Err(RejectReason::Insufficient);
                }
            }
            Action::Sell => {
                if self.free_tokens(spec.owner, market, spec.outcome) < spec.qty {
                    return Err(RejectReason::InsufficientInventory);
                }
            }
        }
        Ok(())
    }

    fn free_cash(&self, owner: OwnerId) -> u128 {
        let locked: u128 = self
            .live
            .values()
            .filter(|l| l.owner == owner && l.action == Action::Buy)
            .map(|l| notional(l.limit, l.remaining))
            .sum();
        self.ledger.cash(owner).saturating_sub(locked)
    }

    fn free_tokens(&self, owner: OwnerId, market: MarketId, outcome: Outcome) -> Qty {
        let mut locked = Qty::ZERO;
        for l in self.live.values() {
            if l.owner == owner
                && l.market == market
                && l.outcome == outcome
                && l.action == Action::Sell
            {
                locked = locked.saturating_add(l.remaining);
            }
        }
        self.ledger
            .tokens(owner, market, outcome)
            .saturating_sub(locked)
    }

    fn follow(&mut self, events: &[BookEvent]) {
        for e in events {
            match e {
                BookEvent::Trade {
                    taker, maker, qty, ..
                } => {
                    self.debit_live(taker.order_id, *qty);
                    self.debit_live(maker.order_id, *qty);
                }
                BookEvent::Resting {
                    order_id,
                    remaining,
                } => {
                    if let Some(l) = self.live.get_mut(order_id) {
                        l.remaining = *remaining;
                    }
                }
                BookEvent::Canceled { order_id, .. } => {
                    self.live.remove(order_id);
                }
                _ => {}
            }
        }
    }

    fn debit_live(&mut self, id: OrderId, qty: Qty) {
        let Some(l) = self.live.get_mut(&id) else {
            return;
        };
        l.remaining = l.remaining.saturating_sub(qty);
        if l.remaining.is_zero() {
            self.live.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Stp, TimeInForce};

    fn eid() -> EventId {
        EventId::from_raw(1)
    }
    fn mid() -> MarketId {
        MarketId::from_raw(1)
    }
    fn owner(n: u64) -> OwnerId {
        OwnerId::from_raw(n)
    }

    fn buy(owner_n: u64, outcome: Outcome, px: u32, qty: u64) -> Place {
        Place {
            owner: owner(owner_n),
            action: Action::Buy,
            outcome,
            limit: Price::from_ticks(px),
            qty: Qty::from_lots(qty),
            tif: TimeInForce::Gtc,
            post_only: false,
            stp: Stp::None,
        }
    }

    fn sell(owner_n: u64, outcome: Outcome, px: u32, qty: u64) -> Place {
        let mut p = buy(owner_n, outcome, px, qty);
        p.action = Action::Sell;
        p
    }

    fn ex() -> Exchange {
        let mut x = Exchange::new(Qty::ONE, TickGrid::cents());
        x.open_event(eid(), [mid()]).unwrap();
        x.credit_cash(owner(1), 20_000);
        x.credit_cash(owner(2), 20_000);
        x
    }

    #[test]
    fn mint_through_exchange() {
        let mut x = ex();
        x.place(eid(), mid(), buy(1, Outcome::No, 40, 100)).unwrap();
        x.place(eid(), mid(), buy(2, Outcome::Yes, 60, 100))
            .unwrap();
        assert_eq!(x.ledger().tokens(owner(2), mid(), Outcome::Yes).lots(), 100);
        assert_eq!(x.ledger().tokens(owner(1), mid(), Outcome::No).lots(), 100);
    }

    #[test]
    fn cannot_double_sell_resting_inventory() {
        let mut x = ex();
        x.split(owner(1), mid(), Qty::from_lots(10)).unwrap();
        x.place(eid(), mid(), sell(1, Outcome::Yes, 60, 10))
            .unwrap();
        assert_eq!(
            x.place(eid(), mid(), sell(1, Outcome::Yes, 50, 10)),
            Err(RejectReason::InsufficientInventory)
        );
    }

    #[test]
    fn sell_without_split_rejected() {
        let mut x = ex();
        assert_eq!(
            x.place(eid(), mid(), sell(1, Outcome::Yes, 60, 1)),
            Err(RejectReason::InsufficientInventory)
        );
    }

    #[test]
    fn cancel_unlocks_tokens() {
        let mut x = ex();
        x.split(owner(1), mid(), Qty::from_lots(10)).unwrap();
        let ev = x
            .place(eid(), mid(), sell(1, Outcome::Yes, 60, 10))
            .unwrap();
        let id = match ev[0] {
            BookEvent::Accepted { order_id, .. } => order_id,
            _ => panic!(),
        };
        x.cancel(eid(), mid(), id).unwrap();
        x.place(eid(), mid(), sell(1, Outcome::Yes, 60, 10))
            .unwrap();
    }

    #[test]
    fn reduce_keeps_priority_via_engine() {
        let mut x = ex();
        x.place(eid(), mid(), buy(1, Outcome::Yes, 50, 10)).unwrap();
        let oid = crate::types::OrderId::from_raw(1);
        x.reduce(eid(), mid(), oid, Qty::from_lots(3)).unwrap();
        assert_eq!(
            x.engine()
                .event(eid())
                .unwrap()
                .book(mid())
                .unwrap()
                .order(oid)
                .unwrap()
                .remaining
                .lots(),
            3
        );
    }
}
