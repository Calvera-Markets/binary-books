mod grid;
mod ids;
mod order;
mod price;
mod qty;
mod side;
mod tif;

pub use grid::{GridError, TickGrid};
pub use ids::{EventId, MarketId, OrderId, OwnerId, Seq, Ts};
pub use order::Order;
pub use price::Price;
pub use qty::Qty;
pub use side::{Action, BookSide, Outcome};
pub use tif::{OrderStatus, Stp, TimeInForce};

/// `price.ticks() * qty.lots()` as `u128` so a full book cannot overflow `u64`.
pub fn notional(price: Price, qty: Qty) -> u128 {
    u128::from(price.ticks()) * u128::from(qty.lots())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notional_is_u128_and_does_not_truncate() {
        let p = Price::from_ticks(u32::MAX);
        let q = Qty::from_lots(u64::MAX);
        assert_eq!(
            notional(p, q),
            u128::from(u32::MAX) * u128::from(u64::MAX)
        );
    }
}
