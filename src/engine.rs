use crate::book::{BookEvent, NewOrder, RejectReason};
use crate::event::Event;
use crate::types::{
    Action, EventId, MarketId, OrderId, Outcome, OwnerId, Price, Qty, Stp, TickGrid, TimeInForce,
    Ts,
};
use std::collections::BTreeMap;

/// Host-facing router. Allocates order ids; never reads the wall clock.
#[derive(Debug)]
pub struct Engine {
    events: BTreeMap<EventId, Event>,
    next_order: OrderId,
    min_order_size: Qty,
}

/// Place request without an engine-assigned id.
#[derive(Clone, Debug)]
pub struct Place {
    pub owner: OwnerId,
    pub action: Action,
    pub outcome: Outcome,
    pub limit: Price,
    pub qty: Qty,
    pub tif: TimeInForce,
    pub post_only: bool,
    pub stp: Stp,
}

impl Engine {
    pub fn new(min_order_size: Qty) -> Self {
        Self {
            events: BTreeMap::new(),
            next_order: OrderId::from_raw(1),
            min_order_size,
        }
    }

    pub fn open_event(
        &mut self,
        id: EventId,
        grid: TickGrid,
        markets: impl IntoIterator<Item = MarketId>,
    ) -> Result<(), RejectReason> {
        if self.events.contains_key(&id) {
            return Err(RejectReason::DuplicateEvent);
        }
        self.events.insert(id, Event::new(id, grid, markets));
        Ok(())
    }

    pub fn close_event(&mut self, id: EventId) -> Result<(), RejectReason> {
        self.events
            .get_mut(&id)
            .map(|e| e.close())
            .ok_or(RejectReason::NotFound)
    }

    pub fn event(&self, id: EventId) -> Option<&Event> {
        self.events.get(&id)
    }

    pub fn event_mut(&mut self, id: EventId) -> Option<&mut Event> {
        self.events.get_mut(&id)
    }

    pub fn place(
        &mut self,
        event: EventId,
        market: MarketId,
        spec: Place,
    ) -> Result<Vec<BookEvent>, RejectReason> {
        if spec.qty < self.min_order_size {
            return Err(RejectReason::MinSize);
        }
        let id = self.next_order;
        self.next_order = self.next_order.next();
        let order = NewOrder {
            id,
            owner: spec.owner,
            market,
            action: spec.action,
            outcome: spec.outcome,
            limit: spec.limit,
            qty: spec.qty,
            tif: spec.tif,
            post_only: spec.post_only,
            stp: spec.stp,
        };
        self.events
            .get_mut(&event)
            .ok_or(RejectReason::NotFound)?
            .place(market, order)
    }

    pub fn cancel(
        &mut self,
        event: EventId,
        market: MarketId,
        id: OrderId,
    ) -> Result<Vec<BookEvent>, RejectReason> {
        self.events
            .get_mut(&event)
            .ok_or(RejectReason::NotFound)?
            .cancel(market, id)
    }

    pub fn expire_due(&mut self, event: EventId, now: Ts) -> Result<Vec<BookEvent>, RejectReason> {
        let ev = self.events.get_mut(&event).ok_or(RejectReason::NotFound)?;
        let mut out = Vec::new();
        let markets: Vec<MarketId> = ev.market_ids().collect();
        for m in markets {
            if let Some(book) = ev.book_mut(m) {
                out.extend(book.expire_due(now));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eid() -> EventId {
        EventId::from_raw(1)
    }
    fn mid(n: u64) -> MarketId {
        MarketId::from_raw(n)
    }

    fn spec(owner: u64, outcome: Outcome, px: u32, qty: u64) -> Place {
        Place {
            owner: OwnerId::from_raw(owner),
            action: Action::Buy,
            outcome,
            limit: Price::from_ticks(px),
            qty: Qty::from_lots(qty),
            tif: TimeInForce::Gtc,
            post_only: false,
            stp: Stp::None,
        }
    }

    #[test]
    fn allocates_unique_ids_across_events() {
        let mut eng = Engine::new(Qty::from_lots(1));
        eng.open_event(eid(), TickGrid::cents(), [mid(1)]).unwrap();
        eng.open_event(EventId::from_raw(2), TickGrid::cents(), [mid(1)])
            .unwrap();
        let a = eng
            .place(eid(), mid(1), spec(1, Outcome::Yes, 40, 1))
            .unwrap();
        let b = eng
            .place(EventId::from_raw(2), mid(1), spec(2, Outcome::Yes, 40, 1))
            .unwrap();
        let id_a = match a[0] {
            BookEvent::Accepted { order_id, .. } => order_id,
            _ => panic!(),
        };
        let id_b = match b[0] {
            BookEvent::Accepted { order_id, .. } => order_id,
            _ => panic!(),
        };
        assert_ne!(id_a, id_b);
    }

    #[test]
    fn closed_market_rejects() {
        let mut eng = Engine::new(Qty::from_lots(1));
        eng.open_event(eid(), TickGrid::cents(), [mid(1)]).unwrap();
        eng.close_event(eid()).unwrap();
        assert_eq!(
            eng.place(eid(), mid(1), spec(1, Outcome::Yes, 40, 1)),
            Err(RejectReason::Closed)
        );
    }

    #[test]
    fn min_size() {
        let mut eng = Engine::new(Qty::from_lots(5));
        eng.open_event(eid(), TickGrid::cents(), [mid(1)]).unwrap();
        assert_eq!(
            eng.place(eid(), mid(1), spec(1, Outcome::Yes, 40, 1)),
            Err(RejectReason::MinSize)
        );
    }
}
