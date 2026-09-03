use super::Price;

/// Error constructing a [`TickGrid`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridError {
    /// Collateral (the `$1` tick count) must be positive.
    ZeroCollateral,
    /// Step must be positive.
    ZeroStep,
    /// `collat` must be divisible by `step` so complements stay on-grid.
    StepNotDivisor,
    /// Grid must have at least one tradable price (`collat > step`).
    NoTradablePrice,
}

/// Discrete price grid. Complements are exact: `p + complement(p) = collat`.
///
/// Linear grids (this module, C1) use a single step. Banded / tapered grids
/// come later and must preserve the same complement involution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickGrid {
    collat: Price,
    step: u32,
}

impl TickGrid {
    /// Linear grid: tradable prices are `step, 2*step, …, collat - step`.
    ///
    /// `0` and `collat` are on the grid as settlement bounds but are not
    /// tradable (Kalshi 1–99 when `collat = 100`, `step = 1`).
    pub fn linear(collat_ticks: u32, step: u32) -> Result<Self, GridError> {
        if collat_ticks == 0 {
            return Err(GridError::ZeroCollateral);
        }
        if step == 0 {
            return Err(GridError::ZeroStep);
        }
        if collat_ticks % step != 0 {
            return Err(GridError::StepNotDivisor);
        }
        if collat_ticks <= step {
            return Err(GridError::NoTradablePrice);
        }
        Ok(Self {
            collat: Price::from_ticks(collat_ticks),
            step,
        })
    }

    /// One-cent binary contract: prices `1..=99`, collat `100`.
    pub fn cents() -> Self {
        Self::linear(100, 1).expect("cents grid is valid")
    }

    pub fn collat(&self) -> Price {
        self.collat
    }

    pub fn step(&self) -> u32 {
        self.step
    }

    /// True if `p` is a multiple of `step` in `0..=collat` (includes bounds).
    pub fn contains(&self, p: Price) -> bool {
        let t = p.ticks();
        t <= self.collat.ticks() && t % self.step == 0
    }

    /// True if `p` can rest or take: on-grid and strictly inside `(0, collat)`.
    pub fn is_tradable(&self, p: Price) -> bool {
        let t = p.ticks();
        t > 0 && t < self.collat.ticks() && t % self.step == 0
    }

    /// `collat - p`. Caller must pass a price that [`contains`] this grid.
    pub fn complement(&self, p: Price) -> Price {
        debug_assert!(self.contains(p), "complement of off-grid price");
        Price::from_ticks(self.collat.ticks() - p.ticks())
    }

    /// Snap down to the nearest tradable tick; `None` if below the first tick.
    pub fn clamp(&self, p: Price) -> Option<Price> {
        let t = p.ticks().min(self.collat.ticks().saturating_sub(self.step));
        let snapped = (t / self.step) * self.step;
        let snapped = Price::from_ticks(snapped);
        self.is_tradable(snapped).then_some(snapped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cents_grid_membership() {
        let g = TickGrid::cents();
        assert!(g.contains(Price::ZERO));
        assert!(g.contains(Price::from_ticks(100)));
        assert!(g.is_tradable(Price::from_ticks(1)));
        assert!(g.is_tradable(Price::from_ticks(99)));
        assert!(!g.is_tradable(Price::ZERO));
        assert!(!g.is_tradable(Price::from_ticks(100)));
        assert!(!g.is_tradable(Price::from_ticks(101)));
    }

    #[test]
    fn complement_is_involution_on_tradable_prices() {
        let g = TickGrid::cents();
        for t in 1..=99 {
            let p = Price::from_ticks(t);
            let q = g.complement(p);
            assert_eq!(p.ticks() + q.ticks(), 100);
            assert_eq!(g.complement(q), p);
            assert!(g.is_tradable(q));
        }
        assert_eq!(g.complement(Price::from_ticks(40)).ticks(), 60);
    }

    #[test]
    fn linear_rejects_bad_params() {
        assert_eq!(TickGrid::linear(0, 1), Err(GridError::ZeroCollateral));
        assert_eq!(TickGrid::linear(100, 0), Err(GridError::ZeroStep));
        assert_eq!(TickGrid::linear(100, 3), Err(GridError::StepNotDivisor));
        assert_eq!(TickGrid::linear(1, 1), Err(GridError::NoTradablePrice));
    }

    #[test]
    fn step_two_skips_odd_ticks() {
        let g = TickGrid::linear(100, 2).unwrap();
        assert!(g.is_tradable(Price::from_ticks(2)));
        assert!(!g.is_tradable(Price::from_ticks(1)));
        assert_eq!(g.complement(Price::from_ticks(40)).ticks(), 60);
        assert!(g.is_tradable(g.complement(Price::from_ticks(2))));
    }

    #[test]
    fn clamp_snaps_down_to_tradable() {
        let g = TickGrid::linear(100, 5).unwrap();
        assert_eq!(g.clamp(Price::from_ticks(14)), Some(Price::from_ticks(10)));
        assert_eq!(g.clamp(Price::from_ticks(99)), Some(Price::from_ticks(95)));
        assert_eq!(g.clamp(Price::ZERO), None);
        assert_eq!(g.clamp(Price::from_ticks(100)), Some(Price::from_ticks(95)));
    }
}
