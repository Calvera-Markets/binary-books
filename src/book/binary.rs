use super::{BookEvent, Ladder, RejectReason};
use crate::matching::{canonicalize, match_kind};
use crate::types::{
    Action, BookSide, MarketId, Order, OrderId, OrderStatus, Outcome, OwnerId, Price, Qty, Seq,
    TickGrid, TimeInForce,
};
use std::collections::HashMap;

/// Inbound order. `limit` is in the named outcome's units.
#[derive(Clone, Debug)]
pub struct NewOrder {
    pub id: OrderId,
    pub owner: OwnerId,
    pub market: MarketId,
    pub action: Action,
    pub outcome: Outcome,
    pub limit: Price,
    pub qty: Qty,
    pub tif: TimeInForce,
    pub post_only: bool,
}

/// Dual-bid matching engine for one binary market.
///
/// YES bids cross NO bids iff `p + q ≥ collat`. Kalshi and Polymarket are
/// adapters over this book, not separate matchers.
#[derive(Clone, Debug)]
pub struct BinaryBook {
    grid: TickGrid,
    yes_bids: Ladder,
    no_bids: Ladder,
    orders: HashMap<OrderId, Order>,
    next_seq: Seq,
}

impl BinaryBook {
    pub fn new(grid: TickGrid) -> Self {
        Self {
            grid,
            yes_bids: Ladder::new(),
            no_bids: Ladder::new(),
            orders: HashMap::new(),
            next_seq: Seq::from_raw(1),
        }
    }

    pub fn grid(&self) -> &TickGrid {
        &self.grid
    }

    pub fn yes_bids(&self) -> &Ladder {
        &self.yes_bids
    }

    pub fn no_bids(&self) -> &Ladder {
        &self.no_bids
    }

    pub fn order(&self, id: OrderId) -> Option<&Order> {
        self.orders.get(&id)
    }

    /// True if best YES + best NO ≥ collat. Must be false after every accept.
    pub fn is_crossed(&self) -> bool {
        match (self.yes_bids.best(), self.no_bids.best()) {
            (Some(y), Some(n)) => {
                u64::from(y.ticks()) + u64::from(n.ticks()) >= u64::from(self.grid.collat().ticks())
            }
            _ => false,
        }
    }

    pub fn place(&mut self, new: NewOrder) -> Vec<BookEvent> {
        if new.qty.is_zero() {
            return vec![BookEvent::Rejected {
                order_id: new.id,
                reason: RejectReason::ZeroQty,
            }];
        }
        if !self.grid.is_tradable(new.limit) {
            return vec![BookEvent::Rejected {
                order_id: new.id,
                reason: RejectReason::InvalidTick,
            }];
        }

        let seq = self.next_seq;
        self.next_seq = self.next_seq.next();
        let (side, price) = canonicalize(new.action, new.outcome, new.limit, &self.grid);

        let mut order = Order {
            id: new.id,
            owner: new.owner,
            market: new.market,
            action: new.action,
            outcome: new.outcome,
            limit: new.limit,
            side,
            price,
            qty: new.qty,
            remaining: new.qty,
            seq,
            tif: new.tif,
            post_only: new.post_only,
            status: OrderStatus::New,
        };

        let mut events = vec![BookEvent::Accepted {
            order_id: order.id,
            seq,
        }];
        self.match_incoming(&mut order, &mut events);

        if order.remaining.is_zero() {
            return events;
        }

        order.status = if order.filled().is_zero() {
            OrderStatus::Resting
        } else {
            OrderStatus::Partial
        };
        self.ladder_mut(order.side)
            .rest(order.price, order.id, order.remaining);
        events.push(BookEvent::Resting {
            order_id: order.id,
            remaining: order.remaining,
        });
        self.orders.insert(order.id, order);
        events
    }

    fn match_incoming(&mut self, taker: &mut Order, events: &mut Vec<BookEvent>) {
        let opposite = taker.side.opposite();
        while !taker.remaining.is_zero() {
            let Some(best) = self.ladder(opposite).best() else {
                break;
            };
            if !crosses(taker.price, best, self.grid.collat()) {
                break;
            }
            let Some((_, maker_id, take, gone)) =
                self.ladder_mut(opposite).take_best(taker.remaining)
            else {
                break;
            };
            let maker = self
                .orders
                .get_mut(&maker_id)
                .expect("resting maker must be in the map");
            let maker_intent = (maker.action, maker.outcome);
            maker.remaining = maker.remaining.saturating_sub(take);
            maker.status = if maker.remaining.is_zero() {
                OrderStatus::Filled
            } else {
                OrderStatus::Partial
            };
            taker.remaining = taker.remaining.saturating_sub(take);

            let (yes_px, no_px) = fill_prices(taker.side, best, &self.grid);
            events.push(BookEvent::Trade {
                taker: taker.id,
                maker: maker_id,
                yes_px,
                no_px,
                qty: take,
                kind: match_kind((taker.action, taker.outcome), maker_intent),
            });

            if gone {
                self.orders.remove(&maker_id);
            }
        }
        if taker.remaining.is_zero() {
            taker.status = OrderStatus::Filled;
        }
    }

    fn ladder(&self, side: BookSide) -> &Ladder {
        match side {
            BookSide::YesBid => &self.yes_bids,
            BookSide::NoBid => &self.no_bids,
        }
    }

    fn ladder_mut(&mut self, side: BookSide) -> &mut Ladder {
        match side {
            BookSide::YesBid => &mut self.yes_bids,
            BookSide::NoBid => &mut self.no_bids,
        }
    }
}

fn crosses(incoming: Price, opposite_best: Price, collat: Price) -> bool {
    u64::from(incoming.ticks()) + u64::from(opposite_best.ticks()) >= u64::from(collat.ticks())
}

/// Maker-price: the resting canonical price sets the split; taker may improve.
fn fill_prices(taker_side: BookSide, maker_px: Price, grid: &TickGrid) -> (Price, Price) {
    match taker_side {
        BookSide::YesBid => (grid.complement(maker_px), maker_px),
        BookSide::NoBid => (maker_px, grid.complement(maker_px)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matching::MatchKind;

    fn p(t: u32) -> Price {
        Price::from_ticks(t)
    }
    fn q(n: u64) -> Qty {
        Qty::from_lots(n)
    }
    fn oid(n: u64) -> OrderId {
        OrderId::from_raw(n)
    }

    fn new_order(id: u64, action: Action, outcome: Outcome, limit: u32, qty: u64) -> NewOrder {
        NewOrder {
            id: oid(id),
            owner: OwnerId::from_raw(id),
            market: MarketId::from_raw(1),
            action,
            outcome,
            limit: p(limit),
            qty: q(qty),
            tif: TimeInForce::Gtc,
            post_only: false,
        }
    }

    fn book() -> BinaryBook {
        BinaryBook::new(TickGrid::cents())
    }

    fn trades(events: &[BookEvent]) -> Vec<(u64, u64, u32, u32, u64, MatchKind)> {
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
                } => Some((
                    taker.raw(),
                    maker.raw(),
                    yes_px.ticks(),
                    no_px.ticks(),
                    qty.lots(),
                    *kind,
                )),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn buy_yes_60_vs_buy_no_40_mints_at_maker() {
        let mut b = book();
        let rest = b.place(new_order(1, Action::Buy, Outcome::No, 40, 100));
        assert!(matches!(rest.last(), Some(BookEvent::Resting { .. })));
        let fill = b.place(new_order(2, Action::Buy, Outcome::Yes, 60, 100));
        assert_eq!(
            trades(&fill),
            vec![(2, 1, 60, 40, 100, MatchKind::Mint)]
        );
        assert!(!b.is_crossed());
        assert!(b.yes_bids().is_empty());
        assert!(b.no_bids().is_empty());
    }

    #[test]
    fn taker_improvement_buy_yes_65_vs_no_40() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::No, 40, 100));
        let fill = b.place(new_order(2, Action::Buy, Outcome::Yes, 65, 100));
        // Maker NO 40 ⇒ implied YES ask 60. Taker limit 65, fills at 60.
        assert_eq!(
            trades(&fill),
            vec![(2, 1, 60, 40, 100, MatchKind::Mint)]
        );
        assert!(!b.is_crossed());
    }

    #[test]
    fn no_cross_when_sum_below_collat() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::No, 40, 100));
        let ev = b.place(new_order(2, Action::Buy, Outcome::Yes, 50, 100));
        assert!(trades(&ev).is_empty());
        assert!(matches!(ev.last(), Some(BookEvent::Resting { .. })));
        assert_eq!(b.yes_bids().best(), Some(p(50)));
        assert_eq!(b.no_bids().best(), Some(p(40)));
        assert!(!b.is_crossed());
    }

    #[test]
    fn partial_fill_remainder_rests() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::No, 40, 40));
        let ev = b.place(new_order(2, Action::Buy, Outcome::Yes, 60, 100));
        assert_eq!(trades(&ev), vec![(2, 1, 60, 40, 40, MatchKind::Mint)]);
        assert_eq!(
            ev.last(),
            Some(&BookEvent::Resting {
                order_id: oid(2),
                remaining: q(60)
            })
        );
        assert_eq!(b.yes_bids().best(), Some(p(60)));
        assert!(b.no_bids().is_empty());
        assert!(!b.is_crossed());
    }

    #[test]
    fn sweep_multiple_no_levels() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::No, 50, 10));
        b.place(new_order(2, Action::Buy, Outcome::No, 40, 10));
        let ev = b.place(new_order(3, Action::Buy, Outcome::Yes, 70, 20));
        // Best NO is 50 (implied YES 50), then 40 (implied YES 60).
        assert_eq!(
            trades(&ev),
            vec![
                (3, 1, 50, 50, 10, MatchKind::Mint),
                (3, 2, 60, 40, 10, MatchKind::Mint),
            ]
        );
        assert!(!b.is_crossed());
        assert!(b.yes_bids().is_empty() && b.no_bids().is_empty());
    }

    #[test]
    fn sell_yes_60_is_buy_no_40_and_matches_yes_bids() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::Yes, 60, 100));
        let ev = b.place(new_order(2, Action::Sell, Outcome::Yes, 60, 100));
        assert_eq!(
            trades(&ev),
            vec![(2, 1, 60, 40, 100, MatchKind::Complementary)]
        );
        assert!(!b.is_crossed());
    }

    #[test]
    fn reject_off_grid_and_zero_qty() {
        let mut b = book();
        let z = b.place(new_order(1, Action::Buy, Outcome::Yes, 50, 0));
        assert!(matches!(
            z[0],
            BookEvent::Rejected {
                reason: RejectReason::ZeroQty,
                ..
            }
        ));
        let mut bad = new_order(2, Action::Buy, Outcome::Yes, 50, 1);
        bad.limit = p(0);
        let r = b.place(bad);
        assert!(matches!(
            r[0],
            BookEvent::Rejected {
                reason: RejectReason::InvalidTick,
                ..
            }
        ));
    }
}
