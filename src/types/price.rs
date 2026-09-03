/// Price in integer ticks. Never a float.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price(u32);

impl Price {
    pub const ZERO: Self = Self(0);

    pub const fn from_ticks(ticks: u32) -> Self {
        Self(ticks)
    }

    pub const fn ticks(self) -> u32 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }
}

impl From<Price> for u32 {
    fn from(p: Price) -> u32 {
        p.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordering_follows_ticks() {
        assert!(Price::from_ticks(40) < Price::from_ticks(60));
        assert_eq!(Price::from_ticks(40).ticks(), 40);
    }

    #[test]
    fn checked_add_overflows_to_none() {
        assert!(Price::from_ticks(u32::MAX).checked_add(Price::from_ticks(1)).is_none());
        assert_eq!(
            Price::from_ticks(40).checked_add(Price::from_ticks(60)),
            Some(Price::from_ticks(100))
        );
    }
}
