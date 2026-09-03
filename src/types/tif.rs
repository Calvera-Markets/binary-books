use super::Ts;

/// Time-in-force. GTD expiry is a host-supplied timestamp; the book never
/// reads the wall clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeInForce {
    Gtc,
    Ioc,
    Fok,
    Gtd { expire_at: Ts },
}

impl TimeInForce {
    pub fn may_rest(self) -> bool {
        matches!(self, TimeInForce::Gtc | TimeInForce::Gtd { .. })
    }
}

/// Self-trade prevention. Applied when taker and resting maker share `owner`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Stp {
    #[default]
    None,
    /// Leave the maker in place; try the next resting order.
    SkipMaker,
    /// Cancel the taker (including any remainder). Maker stays.
    CancelTaker,
    /// Cancel the would-be maker and the taker remainder.
    CancelBoth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OrderStatus {
    New,
    Resting,
    Partial,
    Filled,
    Canceled,
    Rejected,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Ts;

    #[test]
    fn rest_flags() {
        assert!(TimeInForce::Gtc.may_rest());
        assert!(TimeInForce::Gtd {
            expire_at: Ts::from_raw(1)
        }
        .may_rest());
        assert!(!TimeInForce::Ioc.may_rest());
        assert!(!TimeInForce::Fok.may_rest());
    }
}
