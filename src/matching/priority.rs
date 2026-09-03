/// Queue policy at a single merged price.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Priority {
    /// Pure time FIFO (Kalshi / Polymarket).
    #[default]
    PriceTime,
    /// HIP-4: at the same price, resting sells fill before buy-duals.
    PriceSideTime,
}
