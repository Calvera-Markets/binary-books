use super::Level;
use crate::types::{OrderId, Price, Qty};
use std::collections::BTreeMap;

/// Descending bid ladder: best = highest price.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ladder {
    levels: BTreeMap<Price, Level>,
}

impl Ladder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    pub fn best(&self) -> Option<Price> {
        self.levels.keys().next_back().copied()
    }

    pub fn best_qty(&self) -> Option<Qty> {
        self.levels.values().next_back().map(|l| l.qty())
    }

    pub fn rest(&mut self, price: Price, id: OrderId, qty: Qty) {
        self.rest_ranked(price, id, qty, false);
    }

    pub fn rest_ranked(&mut self, price: Price, id: OrderId, qty: Qty, is_sell: bool) {
        self.levels
            .entry(price)
            .or_default()
            .push_back_ranked(id, qty, is_sell);
    }

    pub fn front_id(&self, price: Price) -> Option<OrderId> {
        self.levels.get(&price).and_then(|l| l.front_id())
    }

    /// Pop the entire FIFO head at `price` (used to park skip-maker orders).
    pub fn pop_front_at(&mut self, price: Price) -> Option<(OrderId, Qty, bool)> {
        let level = self.levels.get_mut(&price)?;
        let popped = level.pop_front()?;
        if level.is_empty() {
            self.levels.remove(&price);
        }
        Some(popped)
    }

    pub fn push_front(&mut self, price: Price, id: OrderId, qty: Qty) {
        self.push_front_ranked(price, id, qty, false);
    }

    pub fn push_front_ranked(&mut self, price: Price, id: OrderId, qty: Qty, is_sell: bool) {
        self.levels
            .entry(price)
            .or_default()
            .push_front_ranked(id, qty, is_sell);
    }

    /// Fill against the best level's FIFO head.
    pub fn take_best(&mut self, qty: Qty) -> Option<(Price, OrderId, Qty, bool)> {
        self.take_best_ranked(qty, false)
    }

    pub fn take_best_ranked(
        &mut self,
        qty: Qty,
        prefer_sell: bool,
    ) -> Option<(Price, OrderId, Qty, bool)> {
        let price = self.best()?;
        let level = self.levels.get_mut(&price)?;
        let (id, take, gone) = level.take_ranked(qty, prefer_sell)?;
        if level.is_empty() {
            self.levels.remove(&price);
        }
        Some((price, id, take, gone))
    }

    pub fn cancel(&mut self, price: Price, id: OrderId) -> Option<Qty> {
        let level = self.levels.get_mut(&price)?;
        let qty = level.remove(id)?;
        if level.is_empty() {
            self.levels.remove(&price);
        }
        Some(qty)
    }

    pub fn reduce(&mut self, price: Price, id: OrderId, delta: Qty) -> bool {
        let Some(level) = self.levels.get_mut(&price) else {
            return false;
        };
        if !level.reduce(id, delta) {
            return false;
        }
        if level.is_empty() {
            self.levels.remove(&price);
        }
        true
    }

    /// Best-to-worst: `(price, aggregate qty, order count)`.
    pub fn depth(&self) -> Vec<(Price, Qty, usize)> {
        self.levels
            .iter()
            .rev()
            .map(|(p, l)| (*p, l.qty(), l.order_count()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u64) -> OrderId {
        OrderId::from_raw(n)
    }
    fn p(n: u32) -> Price {
        Price::from_ticks(n)
    }
    fn q(n: u64) -> Qty {
        Qty::from_lots(n)
    }

    #[test]
    fn best_is_highest_price() {
        let mut lad = Ladder::new();
        lad.rest(p(40), id(1), q(1));
        lad.rest(p(60), id(2), q(2));
        lad.rest(p(50), id(3), q(3));
        assert_eq!(lad.best(), Some(p(60)));
        assert_eq!(
            lad.depth(),
            vec![(p(60), q(2), 1), (p(50), q(3), 1), (p(40), q(1), 1)]
        );
    }

    #[test]
    fn take_walks_down_levels() {
        let mut lad = Ladder::new();
        lad.rest(p(60), id(1), q(5));
        lad.rest(p(50), id(2), q(5));
        let hit = lad.take_best(q(5)).unwrap();
        assert_eq!(hit, (p(60), id(1), q(5), true));
        assert_eq!(lad.best(), Some(p(50)));
        let hit = lad.take_best(q(3)).unwrap();
        assert_eq!(hit, (p(50), id(2), q(3), false));
        assert_eq!(lad.best(), Some(p(50)));
    }

    #[test]
    fn cancel_last_order_drops_level() {
        let mut lad = Ladder::new();
        lad.rest(p(40), id(1), q(10));
        lad.rest(p(40), id(2), q(10));
        assert_eq!(lad.cancel(p(40), id(1)), Some(q(10)));
        assert_eq!(lad.best(), Some(p(40)));
        assert_eq!(lad.cancel(p(40), id(2)), Some(q(10)));
        assert!(lad.is_empty());
        assert!(lad.cancel(p(40), id(2)).is_none());
    }

    #[test]
    fn reduce_then_drop_empty_level() {
        let mut lad = Ladder::new();
        lad.rest(p(40), id(1), q(4));
        assert!(lad.reduce(p(40), id(1), q(4)));
        assert!(lad.is_empty());
    }
}
