/// Quantity in integer lots. Never a float.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Qty(u64);

impl Qty {
    pub const ZERO: Self = Self(0);
    pub const ONE: Self = Self(1);

    pub const fn from_lots(lots: u64) -> Self {
        Self(lots)
    }

    pub const fn lots(self) -> u64 {
        self.0
    }

    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    pub fn checked_sub(self, other: Self) -> Option<Self> {
        self.0.checked_sub(other.0).map(Self)
    }

    pub fn min(self, other: Self) -> Self {
        Self(self.0.min(other.0))
    }
}

impl From<Qty> for u64 {
    fn from(q: Qty) -> u64 {
        q.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sub_and_min() {
        let a = Qty::from_lots(10);
        let b = Qty::from_lots(3);
        assert_eq!(a.checked_sub(b), Some(Qty::from_lots(7)));
        assert_eq!(b.checked_sub(a), None);
        assert_eq!(a.min(b), b);
        assert!(Qty::ZERO.is_zero());
    }
}
