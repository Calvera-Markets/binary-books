use super::Price;

/// Error constructing a [`TickGrid`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GridError {
    ZeroCollateral,
    ZeroStep,
    StepNotDivisor,
    NoTradablePrice,
    EmptyBands,
    BandOrder,
    AsymmetricBands,
}

/// Inclusive `[start, end]` range with a fixed step. `end` must be on-step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Band {
    pub start: u32,
    pub end: u32,
    pub step: u32,
}

impl Band {
    fn contains(self, t: u32) -> bool {
        t >= self.start && t <= self.end && (t - self.start) % self.step == 0
    }
}

/// Discrete price grid. Complements are exact: `p + complement(p) = collat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TickGrid {
    collat: Price,
    bands: Vec<Band>,
}

impl TickGrid {
    /// Linear grid: tradable prices are `step, 2*step, …, collat - step`.
    pub fn linear(collat_ticks: u32, step: u32) -> Result<Self, GridError> {
        Self::banded(
            collat_ticks,
            vec![Band {
                start: 0,
                end: collat_ticks,
                step,
            }],
        )
    }

    /// Banded / tapered grid (Kalshi `price_ranges`). Bands must be
    /// complement-symmetric so `complement` stays on-grid.
    pub fn banded(collat_ticks: u32, bands: Vec<Band>) -> Result<Self, GridError> {
        if collat_ticks == 0 {
            return Err(GridError::ZeroCollateral);
        }
        if bands.is_empty() {
            return Err(GridError::EmptyBands);
        }
        for b in &bands {
            if b.step == 0 {
                return Err(GridError::ZeroStep);
            }
            if b.start > b.end || b.end > collat_ticks {
                return Err(GridError::BandOrder);
            }
            if (b.end - b.start) % b.step != 0 {
                return Err(GridError::StepNotDivisor);
            }
        }
        let grid = Self {
            collat: Price::from_ticks(collat_ticks),
            bands,
        };
        if !grid.has_tradable() {
            return Err(GridError::NoTradablePrice);
        }
        if !grid.complements_on_grid() {
            return Err(GridError::AsymmetricBands);
        }
        Ok(grid)
    }

    pub fn cents() -> Self {
        Self::linear(100, 1).expect("cents grid is valid")
    }

    pub fn collat(&self) -> Price {
        self.collat
    }

    /// Finest step across bands (linear grids: the single step).
    pub fn step(&self) -> u32 {
        self.bands.iter().map(|b| b.step).min().unwrap_or(1)
    }

    pub fn bands(&self) -> &[Band] {
        &self.bands
    }

    pub fn contains(&self, p: Price) -> bool {
        let t = p.ticks();
        t <= self.collat.ticks() && self.bands.iter().any(|b| b.contains(t))
    }

    pub fn is_tradable(&self, p: Price) -> bool {
        let t = p.ticks();
        t > 0 && t < self.collat.ticks() && self.contains(p)
    }

    pub fn complement(&self, p: Price) -> Price {
        debug_assert!(self.contains(p), "complement of off-grid price");
        Price::from_ticks(self.collat.ticks() - p.ticks())
    }

    pub fn clamp(&self, p: Price) -> Option<Price> {
        let t = p
            .ticks()
            .min(self.collat.ticks().saturating_sub(self.step()));
        let mut best: Option<u32> = None;
        for b in &self.bands {
            if t < b.start {
                continue;
            }
            let cap = t.min(b.end);
            let snapped = b.start + ((cap - b.start) / b.step) * b.step;
            if snapped > 0 && snapped < self.collat.ticks() && snapped <= t {
                best = Some(best.map_or(snapped, |x| x.max(snapped)));
            }
        }
        best.map(Price::from_ticks)
    }

    fn has_tradable(&self) -> bool {
        (1..self.collat.ticks()).any(|t| self.is_tradable(Price::from_ticks(t)))
    }

    fn complements_on_grid(&self) -> bool {
        (0..=self.collat.ticks()).all(|t| {
            let p = Price::from_ticks(t);
            !self.contains(p) || self.contains(self.complement(p))
        })
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

    #[test]
    fn deci_cent_linear() {
        let g = TickGrid::linear(1000, 1).unwrap();
        assert!(g.is_tradable(Price::from_ticks(1)));
        assert_eq!(g.complement(Price::from_ticks(1)).ticks(), 999);
    }

    #[test]
    fn tapered_edges_finer_than_center() {
        // collat 1000 = $1 in 0.1¢ ticks. 0.001 on edges, 0.01 in the middle.
        let g = TickGrid::banded(
            1000,
            vec![
                Band {
                    start: 0,
                    end: 100,
                    step: 1,
                },
                Band {
                    start: 100,
                    end: 900,
                    step: 10,
                },
                Band {
                    start: 900,
                    end: 1000,
                    step: 1,
                },
            ],
        )
        .unwrap();
        assert!(g.is_tradable(Price::from_ticks(15)));
        assert!(!g.is_tradable(Price::from_ticks(105)));
        assert!(g.is_tradable(Price::from_ticks(110)));
        assert_eq!(g.complement(Price::from_ticks(15)).ticks(), 985);
        assert!(g.is_tradable(g.complement(Price::from_ticks(15))));
        assert_eq!(g.complement(Price::from_ticks(110)).ticks(), 890);
        assert!(g.is_tradable(Price::from_ticks(890)));
    }

    #[test]
    fn asymmetric_bands_rejected() {
        let err = TickGrid::banded(
            100,
            vec![
                Band {
                    start: 0,
                    end: 10,
                    step: 1,
                },
                Band {
                    start: 10,
                    end: 100,
                    step: 5,
                },
            ],
        );
        assert_eq!(err, Err(GridError::AsymmetricBands));
    }
}
