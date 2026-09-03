use super::{BookEvent, FillParty, Ladder, RejectReason};
use crate::matching::{canonicalize, match_kind, Priority};
use crate::types::{
    Action, BookSide, MarketId, Order, OrderId, OrderStatus, Outcome, OwnerId, Price, Qty, Seq,
    Stp, TickGrid, TimeInForce, Ts,
};
use std::collections::HashMap;

/// Inbound order. `limit` is in the named outcome's units.
#[derive(Clone, Debug, PartialEq, Eq)]
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
    pub stp: Stp,
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
    priority: Priority,
}

impl BinaryBook {
    pub fn new(grid: TickGrid) -> Self {
        Self::with_priority(grid, Priority::PriceTime)
    }

    pub fn with_priority(grid: TickGrid, priority: Priority) -> Self {
        Self {
            grid,
            yes_bids: Ladder::new(),
            no_bids: Ladder::new(),
            orders: HashMap::new(),
            next_seq: Seq::from_raw(1),
            priority,
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

        let (side, price) = canonicalize(new.action, new.outcome, new.limit, &self.grid);
        if new.post_only && !new.tif.may_rest() {
            return vec![BookEvent::Rejected {
                order_id: new.id,
                reason: RejectReason::PostOnlyWithTif,
            }];
        }
        let fillable = self.fillable_qty(side, price, new.qty);
        if new.post_only && !fillable.is_zero() {
            return vec![BookEvent::Rejected {
                order_id: new.id,
                reason: RejectReason::PostOnlyWouldTake,
            }];
        }
        if matches!(new.tif, TimeInForce::Fok) && fillable < new.qty {
            return vec![BookEvent::Rejected {
                order_id: new.id,
                reason: RejectReason::FokUnfilled,
            }];
        }

        let seq = self.next_seq;
        self.next_seq = self.next_seq.next();

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
            stp: new.stp,
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

        if !order.tif.may_rest() {
            events.push(BookEvent::Canceled {
                order_id: order.id,
                remaining: order.remaining,
            });
            return events;
        }

        order.status = if order.filled().is_zero() {
            OrderStatus::Resting
        } else {
            OrderStatus::Partial
        };
        self.ladder_mut(order.side).rest_ranked(
            order.price,
            order.id,
            order.remaining,
            order.action == Action::Sell,
        );
        events.push(BookEvent::Resting {
            order_id: order.id,
            remaining: order.remaining,
        });
        self.orders.insert(order.id, order);
        events
    }

    /// Cancel resting GTD orders with `expire_at <= now`. Host clock only.
    pub fn expire_due(&mut self, now: Ts) -> Vec<BookEvent> {
        let ids: Vec<OrderId> = self
            .orders
            .values()
            .filter_map(|o| match o.tif {
                TimeInForce::Gtd { expire_at } if expire_at.raw() <= now.raw() => Some(o.id),
                _ => None,
            })
            .collect();
        let mut events = Vec::new();
        for id in ids {
            events.extend(self.cancel(id));
        }
        events
    }

    fn fillable_qty(&self, side: BookSide, price: Price, qty: Qty) -> Qty {
        let mut left = qty;
        let mut got = Qty::ZERO;
        for (px, level_qty, _) in self.ladder(side.opposite()).depth() {
            if !crosses(price, px, self.grid.collat()) {
                break;
            }
            let take = left.min(level_qty);
            got = got.saturating_add(take);
            left = left.saturating_sub(take);
            if left.is_zero() {
                break;
            }
        }
        got
    }

    pub fn cancel(&mut self, id: OrderId) -> Vec<BookEvent> {
        let Some(order) = self.orders.remove(&id) else {
            return vec![BookEvent::Rejected {
                order_id: id,
                reason: RejectReason::NotFound,
            }];
        };
        let _ = self.ladder_mut(order.side).cancel(order.price, id);
        vec![BookEvent::Canceled {
            order_id: id,
            remaining: order.remaining,
        }]
    }

    pub fn cancel_all(&mut self, owner: OwnerId) -> Vec<BookEvent> {
        let ids: Vec<OrderId> = self
            .orders
            .values()
            .filter(|o| o.owner == owner)
            .map(|o| o.id)
            .collect();
        let mut events = Vec::new();
        for id in ids {
            events.extend(self.cancel(id));
        }
        events
    }

    /// Set remaining to `new_qty`. Zero cancels. Must be strictly less than
    /// current remaining to keep time priority; equal is a no-op accept.
    pub fn reduce(&mut self, id: OrderId, new_qty: Qty) -> Vec<BookEvent> {
        let Some(order) = self.orders.get(&id) else {
            return vec![BookEvent::Rejected {
                order_id: id,
                reason: RejectReason::NotFound,
            }];
        };
        if new_qty.is_zero() {
            return self.cancel(id);
        }
        if new_qty == order.remaining {
            return vec![];
        }
        if new_qty > order.remaining {
            return vec![BookEvent::Rejected {
                order_id: id,
                reason: RejectReason::InvalidReduce,
            }];
        }
        let delta = order.remaining.saturating_sub(new_qty);
        let side = order.side;
        let price = order.price;
        assert!(
            self.ladder_mut(side).reduce(price, id, delta),
            "level qty must match order remaining"
        );
        let order = self.orders.get_mut(&id).expect("still present");
        order.remaining = new_qty;
        vec![BookEvent::Resting {
            order_id: id,
            remaining: new_qty,
        }]
    }

    fn match_incoming(&mut self, taker: &mut Order, events: &mut Vec<BookEvent>) {
        let opposite = taker.side.opposite();
        let mut parked: Vec<(Price, OrderId, Qty, bool)> = Vec::new();
        let prefer_sell = self.priority == Priority::PriceSideTime;
        while !taker.remaining.is_zero() {
            let Some(best) = self.ladder(opposite).best() else {
                break;
            };
            if !crosses(taker.price, best, self.grid.collat()) {
                break;
            }
            let Some(maker_id) = self.ladder(opposite).front_id(best) else {
                break;
            };
            let maker_owner = self
                .orders
                .get(&maker_id)
                .expect("resting maker must be in the map")
                .owner;
            if maker_owner == taker.owner && taker.stp != Stp::None {
                match taker.stp {
                    Stp::None => unreachable!(),
                    Stp::SkipMaker => {
                        let (id, qty, is_sell) = self
                            .ladder_mut(opposite)
                            .pop_front_at(best)
                            .expect("front just peeked");
                        parked.push((best, id, qty, is_sell));
                        continue;
                    }
                    Stp::CancelTaker => {
                        events.push(BookEvent::Canceled {
                            order_id: taker.id,
                            remaining: taker.remaining,
                        });
                        taker.remaining = Qty::ZERO;
                        taker.status = OrderStatus::Canceled;
                        break;
                    }
                    Stp::CancelBoth => {
                        events.extend(self.cancel(maker_id));
                        events.push(BookEvent::Canceled {
                            order_id: taker.id,
                            remaining: taker.remaining,
                        });
                        taker.remaining = Qty::ZERO;
                        taker.status = OrderStatus::Canceled;
                        break;
                    }
                }
            }
            let Some((_, maker_id, take, gone)) = self
                .ladder_mut(opposite)
                .take_best_ranked(taker.remaining, prefer_sell)
            else {
                break;
            };
            let maker = self
                .orders
                .get_mut(&maker_id)
                .expect("resting maker must be in the map");
            let maker_party = FillParty {
                order_id: maker.id,
                owner: maker.owner,
                action: maker.action,
                outcome: maker.outcome,
            };
            maker.remaining = maker.remaining.saturating_sub(take);
            maker.status = if maker.remaining.is_zero() {
                OrderStatus::Filled
            } else {
                OrderStatus::Partial
            };
            taker.remaining = taker.remaining.saturating_sub(take);

            let (yes_px, no_px) = fill_prices(taker.side, best, &self.grid);
            events.push(BookEvent::Trade {
                taker: FillParty {
                    order_id: taker.id,
                    owner: taker.owner,
                    action: taker.action,
                    outcome: taker.outcome,
                },
                maker: maker_party,
                yes_px,
                no_px,
                qty: take,
                kind: match_kind(
                    (taker.action, taker.outcome),
                    (maker_party.action, maker_party.outcome),
                ),
            });

            if gone {
                self.orders.remove(&maker_id);
            }
        }
        for (price, id, qty, is_sell) in parked.into_iter().rev() {
            self.ladder_mut(opposite)
                .push_front_ranked(price, id, qty, is_sell);
        }
        if taker.remaining.is_zero() && taker.status != OrderStatus::Canceled {
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
            stp: Stp::None,
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
                    taker.order_id.raw(),
                    maker.order_id.raw(),
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
        assert_eq!(trades(&fill), vec![(2, 1, 60, 40, 100, MatchKind::Mint)]);
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
        assert_eq!(trades(&fill), vec![(2, 1, 60, 40, 100, MatchKind::Mint)]);
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
    fn sell_yes_vs_sell_no_is_merge() {
        let mut b = book();
        b.place(new_order(1, Action::Sell, Outcome::No, 40, 100));
        let ev = b.place(new_order(2, Action::Sell, Outcome::Yes, 60, 100));
        assert_eq!(trades(&ev), vec![(2, 1, 60, 40, 100, MatchKind::Merge)]);
        assert!(!b.is_crossed());
    }

    #[test]
    fn buy_no_vs_sell_no_is_complementary() {
        let mut b = book();
        b.place(new_order(1, Action::Sell, Outcome::No, 40, 50));
        let ev = b.place(new_order(2, Action::Buy, Outcome::No, 40, 50));
        assert_eq!(
            trades(&ev),
            vec![(2, 1, 60, 40, 50, MatchKind::Complementary)]
        );
    }

    #[test]
    fn cancel_unfilled_and_unknown() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::Yes, 40, 10));
        let ev = b.cancel(oid(1));
        assert_eq!(
            ev,
            vec![BookEvent::Canceled {
                order_id: oid(1),
                remaining: q(10)
            }]
        );
        assert!(b.yes_bids().is_empty());
        assert!(matches!(
            b.cancel(oid(1))[0],
            BookEvent::Rejected {
                reason: RejectReason::NotFound,
                ..
            }
        ));
    }

    #[test]
    fn cancel_remainder_after_partial() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::No, 40, 40));
        b.place(new_order(2, Action::Buy, Outcome::Yes, 60, 100));
        let ev = b.cancel(oid(2));
        assert_eq!(
            ev,
            vec![BookEvent::Canceled {
                order_id: oid(2),
                remaining: q(60)
            }]
        );
        assert!(b.yes_bids().is_empty());
    }

    #[test]
    fn reduce_keeps_fifo_place() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::Yes, 60, 10));
        b.place(new_order(2, Action::Buy, Outcome::Yes, 60, 10));
        b.reduce(oid(1), q(3));
        let ev = b.place(new_order(3, Action::Buy, Outcome::No, 40, 5));
        // id 1 still first at 60, even after reduce.
        assert_eq!(trades(&ev)[0].1, 1);
        assert_eq!(trades(&ev)[0].4, 3);
        assert_eq!(trades(&ev)[1].1, 2);
        assert_eq!(trades(&ev)[1].4, 2);
    }

    #[test]
    fn cancel_all_owner() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::Yes, 40, 1));
        let mut other = new_order(2, Action::Buy, Outcome::Yes, 41, 1);
        other.owner = OwnerId::from_raw(1);
        b.place(other);
        b.place(new_order(3, Action::Buy, Outcome::No, 20, 1));
        let ev = b.cancel_all(OwnerId::from_raw(1));
        assert_eq!(ev.len(), 2);
        assert!(b.order(oid(3)).is_some());
        assert!(b.order(oid(1)).is_none());
    }

    #[test]
    fn fok_fills_fully_or_leaves_book() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::No, 40, 50));
        let mut kill = new_order(2, Action::Buy, Outcome::Yes, 60, 100);
        kill.tif = TimeInForce::Fok;
        let ev = b.place(kill);
        assert!(matches!(
            ev[0],
            BookEvent::Rejected {
                reason: RejectReason::FokUnfilled,
                ..
            }
        ));
        assert_eq!(b.no_bids().best_qty(), Some(q(50)));
        assert!(b.order(oid(1)).is_some());

        let mut ok = new_order(3, Action::Buy, Outcome::Yes, 60, 50);
        ok.tif = TimeInForce::Fok;
        let ev = b.place(ok);
        assert_eq!(trades(&ev), vec![(3, 1, 60, 40, 50, MatchKind::Mint)]);
        assert!(b.no_bids().is_empty());
    }

    #[test]
    fn ioc_partial_then_cancels_rest() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::No, 40, 40));
        let mut ioc = new_order(2, Action::Buy, Outcome::Yes, 60, 100);
        ioc.tif = TimeInForce::Ioc;
        let ev = b.place(ioc);
        assert_eq!(trades(&ev), vec![(2, 1, 60, 40, 40, MatchKind::Mint)]);
        assert!(matches!(
            ev.last(),
            Some(BookEvent::Canceled {
                remaining,
                ..
            }) if *remaining == q(60)
        ));
        assert!(b.yes_bids().is_empty());
    }

    #[test]
    fn post_only_rejects_cross_and_rests_otherwise() {
        let mut b = book();
        b.place(new_order(1, Action::Buy, Outcome::No, 40, 10));
        let mut po = new_order(2, Action::Buy, Outcome::Yes, 60, 10);
        po.post_only = true;
        assert!(matches!(
            b.place(po)[0],
            BookEvent::Rejected {
                reason: RejectReason::PostOnlyWouldTake,
                ..
            }
        ));
        assert_eq!(b.no_bids().best_qty(), Some(q(10)));

        let mut rest = new_order(3, Action::Buy, Outcome::Yes, 50, 10);
        rest.post_only = true;
        assert!(matches!(
            b.place(rest).last(),
            Some(BookEvent::Resting { .. })
        ));
        assert_eq!(b.yes_bids().best(), Some(p(50)));
    }

    #[test]
    fn post_only_illegal_with_ioc() {
        let mut b = book();
        let mut o = new_order(1, Action::Buy, Outcome::Yes, 50, 1);
        o.post_only = true;
        o.tif = TimeInForce::Ioc;
        assert!(matches!(
            b.place(o)[0],
            BookEvent::Rejected {
                reason: RejectReason::PostOnlyWithTif,
                ..
            }
        ));
    }

    #[test]
    fn gtd_expires_on_host_clock() {
        let mut b = book();
        let mut o = new_order(1, Action::Buy, Outcome::Yes, 40, 10);
        o.tif = TimeInForce::Gtd {
            expire_at: Ts::from_raw(100),
        };
        b.place(o);
        assert!(b.expire_due(Ts::from_raw(99)).is_empty());
        assert!(b.order(oid(1)).is_some());
        let ev = b.expire_due(Ts::from_raw(100));
        assert_eq!(
            ev,
            vec![BookEvent::Canceled {
                order_id: oid(1),
                remaining: q(10)
            }]
        );
    }

    #[test]
    fn stp_skip_maker_fills_next() {
        let mut b = book();
        let mut a = new_order(1, Action::Buy, Outcome::Yes, 60, 10);
        a.owner = OwnerId::from_raw(9);
        b.place(a);
        b.place(new_order(2, Action::Buy, Outcome::Yes, 60, 10));
        let mut taker = new_order(3, Action::Buy, Outcome::No, 40, 10);
        taker.owner = OwnerId::from_raw(9);
        taker.stp = Stp::SkipMaker;
        let ev = b.place(taker);
        assert_eq!(trades(&ev), vec![(3, 2, 60, 40, 10, MatchKind::Mint)]);
        assert!(b.order(oid(1)).is_some());
        assert!(b.order(oid(2)).is_none());
    }

    #[test]
    fn stp_cancel_taker_leaves_maker() {
        let mut b = book();
        let mut a = new_order(1, Action::Buy, Outcome::Yes, 60, 10);
        a.owner = OwnerId::from_raw(9);
        b.place(a);
        let mut taker = new_order(2, Action::Buy, Outcome::No, 40, 10);
        taker.owner = OwnerId::from_raw(9);
        taker.stp = Stp::CancelTaker;
        let ev = b.place(taker);
        assert!(trades(&ev).is_empty());
        assert!(matches!(
            ev.last(),
            Some(BookEvent::Canceled {
                order_id,
                ..
            }) if *order_id == oid(2)
        ));
        assert!(b.order(oid(1)).is_some());
        assert!(b.yes_bids().best_qty() == Some(q(10)));
    }

    #[test]
    fn stp_cancel_both() {
        let mut b = book();
        let mut a = new_order(1, Action::Buy, Outcome::Yes, 60, 10);
        a.owner = OwnerId::from_raw(9);
        b.place(a);
        let mut taker = new_order(2, Action::Buy, Outcome::No, 40, 10);
        taker.owner = OwnerId::from_raw(9);
        taker.stp = Stp::CancelBoth;
        let ev = b.place(taker);
        assert!(trades(&ev).is_empty());
        assert!(b.order(oid(1)).is_none());
        assert!(b.yes_bids().is_empty());
    }

    #[test]
    fn stp_none_allows_self_trade() {
        let mut b = book();
        let mut a = new_order(1, Action::Buy, Outcome::Yes, 60, 10);
        a.owner = OwnerId::from_raw(9);
        b.place(a);
        let mut taker = new_order(2, Action::Buy, Outcome::No, 40, 10);
        taker.owner = OwnerId::from_raw(9);
        let ev = b.place(taker);
        assert_eq!(trades(&ev).len(), 1);
    }

    #[test]
    fn price_side_time_sells_before_buy_duals() {
        let mut fifo = book();
        fifo.place(new_order(1, Action::Buy, Outcome::No, 40, 10));
        fifo.place(new_order(2, Action::Sell, Outcome::Yes, 60, 10));
        let ev = fifo.place(new_order(3, Action::Buy, Outcome::Yes, 60, 10));
        assert_eq!(trades(&ev)[0].1, 1, "FIFO fills the earlier buy-NO");

        let mut side = BinaryBook::with_priority(TickGrid::cents(), Priority::PriceSideTime);
        side.place(new_order(1, Action::Buy, Outcome::No, 40, 10));
        side.place(new_order(2, Action::Sell, Outcome::Yes, 60, 10));
        let ev = side.place(new_order(3, Action::Buy, Outcome::Yes, 60, 10));
        assert_eq!(
            trades(&ev)[0].1,
            2,
            "price-side-time fills the sell before the buy-dual"
        );
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
