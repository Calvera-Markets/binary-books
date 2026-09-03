use crate::types::{OrderId, Qty};
use std::collections::VecDeque;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    id: OrderId,
    qty: Qty,
}

/// FIFO queue at a single price. Quantity decreases keep place; the book
/// does not support increasing size in-place.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Level {
    entries: VecDeque<Entry>,
    qty: Qty,
}

impl Level {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn qty(&self) -> Qty {
        self.qty
    }

    pub fn order_count(&self) -> usize {
        self.entries.len()
    }

    pub fn front_id(&self) -> Option<OrderId> {
        self.entries.front().map(|e| e.id)
    }

    pub fn front_qty(&self) -> Option<Qty> {
        self.entries.front().map(|e| e.qty)
    }

    pub fn push_back(&mut self, id: OrderId, qty: Qty) {
        debug_assert!(!qty.is_zero());
        self.qty = self.qty.saturating_add(qty);
        self.entries.push_back(Entry { id, qty });
    }

    pub fn pop_front(&mut self) -> Option<(OrderId, Qty)> {
        let e = self.entries.pop_front()?;
        self.qty = self.qty.saturating_sub(e.qty);
        Some((e.id, e.qty))
    }

    /// Fill `qty` from the head. Returns the head id and how much was taken.
    /// Head is removed if its remaining hits zero.
    pub fn take_front(&mut self, qty: Qty) -> Option<(OrderId, Qty, bool)> {
        let front = self.entries.front_mut()?;
        let take = front.qty.min(qty);
        front.qty = front.qty.saturating_sub(take);
        self.qty = self.qty.saturating_sub(take);
        let id = front.id;
        let gone = front.qty.is_zero();
        if gone {
            self.entries.pop_front();
        }
        Some((id, take, gone))
    }

    /// Remove `id` wherever it sits. Returns its remaining qty, if present.
    pub fn remove(&mut self, id: OrderId) -> Option<Qty> {
        let idx = self.entries.iter().position(|e| e.id == id)?;
        let e = self.entries.remove(idx).expect("index from position");
        self.qty = self.qty.saturating_sub(e.qty);
        Some(e.qty)
    }

    /// Decrease remaining of `id` by `delta` without losing place.
    /// Returns false if `id` is missing or `delta` exceeds remaining.
    pub fn reduce(&mut self, id: OrderId, delta: Qty) -> bool {
        let Some(idx) = self.entries.iter().position(|e| e.id == id) else {
            return false;
        };
        if delta.is_zero() {
            return true;
        }
        let remaining = self.entries[idx].qty;
        let Some(next) = remaining.checked_sub(delta) else {
            return false;
        };
        if next.is_zero() {
            self.entries.remove(idx);
        } else {
            self.entries[idx].qty = next;
        }
        self.qty = self.qty.saturating_sub(delta);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u64) -> OrderId {
        OrderId::from_raw(n)
    }

    fn q(n: u64) -> Qty {
        Qty::from_lots(n)
    }

    #[test]
    fn fifo_pop_order() {
        let mut lvl = Level::new();
        lvl.push_back(id(1), q(10));
        lvl.push_back(id(2), q(5));
        assert_eq!(lvl.qty(), q(15));
        assert_eq!(lvl.order_count(), 2);
        assert_eq!(lvl.pop_front(), Some((id(1), q(10))));
        assert_eq!(lvl.pop_front(), Some((id(2), q(5))));
        assert!(lvl.is_empty());
        assert_eq!(lvl.qty(), Qty::ZERO);
    }

    #[test]
    fn take_front_partial_then_exhaust() {
        let mut lvl = Level::new();
        lvl.push_back(id(1), q(10));
        assert_eq!(lvl.take_front(q(4)), Some((id(1), q(4), false)));
        assert_eq!(lvl.front_qty(), Some(q(6)));
        assert_eq!(lvl.take_front(q(6)), Some((id(1), q(6), true)));
        assert!(lvl.is_empty());
    }

    #[test]
    fn remove_from_middle() {
        let mut lvl = Level::new();
        lvl.push_back(id(1), q(1));
        lvl.push_back(id(2), q(2));
        lvl.push_back(id(3), q(3));
        assert_eq!(lvl.remove(id(2)), Some(q(2)));
        assert_eq!(lvl.qty(), q(4));
        assert_eq!(lvl.pop_front(), Some((id(1), q(1))));
        assert_eq!(lvl.pop_front(), Some((id(3), q(3))));
        assert!(lvl.remove(id(9)).is_none());
    }

    #[test]
    fn reduce_keeps_place() {
        let mut lvl = Level::new();
        lvl.push_back(id(1), q(10));
        lvl.push_back(id(2), q(10));
        assert!(lvl.reduce(id(1), q(7)));
        assert_eq!(lvl.front_id(), Some(id(1)));
        assert_eq!(lvl.front_qty(), Some(q(3)));
        assert_eq!(lvl.qty(), q(13));
        assert!(!lvl.reduce(id(1), q(4)));
        assert!(lvl.reduce(id(1), q(3)));
        assert_eq!(lvl.front_id(), Some(id(2)));
        assert_eq!(lvl.qty(), q(10));
    }

    #[test]
    fn reduce_missing_id_fails() {
        let mut lvl = Level::new();
        lvl.push_back(id(1), q(5));
        assert!(!lvl.reduce(id(2), q(1)));
    }
}
