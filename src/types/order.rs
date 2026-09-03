use super::{
    Action, BookSide, MarketId, OrderId, OrderStatus, Outcome, OwnerId, Price, Qty, Seq, Stp,
    TimeInForce,
};

/// Resting or inbound order. `limit` is the trader's original price in the
/// named outcome's units; `price` is the canonical ladder price.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    pub id: OrderId,
    pub owner: OwnerId,
    pub market: MarketId,
    pub action: Action,
    pub outcome: Outcome,
    pub limit: Price,
    pub side: BookSide,
    pub price: Price,
    pub qty: Qty,
    pub remaining: Qty,
    pub seq: Seq,
    pub tif: TimeInForce,
    pub post_only: bool,
    pub stp: Stp,
    pub status: OrderStatus,
}

impl Order {
    pub fn filled(self: &Self) -> Qty {
        self.qty.saturating_sub(self.remaining)
    }

    pub fn is_open(&self) -> bool {
        matches!(self.status, OrderStatus::Resting | OrderStatus::Partial)
            && !self.remaining.is_zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, BookSide, MarketId, OrderId, Outcome, OwnerId, Seq, TimeInForce};

    fn sample() -> Order {
        Order {
            id: OrderId::from_raw(1),
            owner: OwnerId::from_raw(1),
            market: MarketId::from_raw(1),
            action: Action::Buy,
            outcome: Outcome::Yes,
            limit: Price::from_ticks(40),
            side: BookSide::YesBid,
            price: Price::from_ticks(40),
            qty: Qty::from_lots(10),
            remaining: Qty::from_lots(4),
            seq: Seq::from_raw(1),
            tif: TimeInForce::Gtc,
            post_only: false,
            stp: Stp::None,
            status: OrderStatus::Partial,
        }
    }

    #[test]
    fn filled_qty_and_open() {
        let o = sample();
        assert_eq!(o.filled(), Qty::from_lots(6));
        assert!(o.is_open());
    }
}
