/// Which outcome token the trader is talking about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Outcome {
    Yes,
    No,
}

impl Outcome {
    pub fn complement(self) -> Self {
        match self {
            Outcome::Yes => Outcome::No,
            Outcome::No => Outcome::Yes,
        }
    }
}

/// Buy or sell the named outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Buy,
    Sell,
}

/// Canonical ladder an order rests on after complement mapping.
///
/// Buy YES and sell NO are the same economic bid (long YES).
/// Buy NO and sell YES are the same economic bid (long NO).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BookSide {
    YesBid,
    NoBid,
}

impl BookSide {
    pub fn opposite(self) -> Self {
        match self {
            BookSide::YesBid => BookSide::NoBid,
            BookSide::NoBid => BookSide::YesBid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complements_flip() {
        assert_eq!(Outcome::Yes.complement(), Outcome::No);
        assert_eq!(BookSide::YesBid.opposite(), BookSide::NoBid);
    }
}
