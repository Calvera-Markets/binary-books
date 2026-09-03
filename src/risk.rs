//! Optional cash reserve port. Kalshi-style shorts: reserve the canonical
//! price times qty (buy YES at `p` locks `p`; sell YES at `p` locks `1-p`).

use crate::book::RejectReason;
use crate::matching::canonicalize;
use crate::types::{Action, Outcome, OwnerId, Price, Qty, TickGrid};
use std::collections::HashMap;

pub fn reserve_notional(
    grid: &TickGrid,
    action: Action,
    outcome: Outcome,
    limit: Price,
    qty: Qty,
) -> u128 {
    let (_, px) = canonicalize(action, outcome, limit, grid);
    crate::notional(px, qty)
}

pub trait RiskPort {
    fn reserve(&mut self, owner: OwnerId, notional: u128) -> Result<(), RejectReason>;
    fn release(&mut self, owner: OwnerId, notional: u128);
}

#[derive(Debug, Default)]
pub struct NoopRisk;

impl RiskPort for NoopRisk {
    fn reserve(&mut self, _owner: OwnerId, _notional: u128) -> Result<(), RejectReason> {
        Ok(())
    }
    fn release(&mut self, _owner: OwnerId, _notional: u128) {}
}

#[derive(Clone, Debug, Default)]
struct Acct {
    bankroll: u128,
    reserved: u128,
}

#[derive(Debug, Default)]
pub struct CashLedger {
    accts: HashMap<OwnerId, Acct>,
}

impl CashLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn credit(&mut self, owner: OwnerId, amount: u128) {
        self.accts.entry(owner).or_default().bankroll += amount;
    }

    pub fn bankroll(&self, owner: OwnerId) -> u128 {
        self.accts.get(&owner).map(|a| a.bankroll).unwrap_or(0)
    }

    pub fn reserved(&self, owner: OwnerId) -> u128 {
        self.accts.get(&owner).map(|a| a.reserved).unwrap_or(0)
    }

    pub fn available(&self, owner: OwnerId) -> u128 {
        self.bankroll(owner).saturating_sub(self.reserved(owner))
    }
}

impl RiskPort for CashLedger {
    fn reserve(&mut self, owner: OwnerId, notional: u128) -> Result<(), RejectReason> {
        let acct = self.accts.entry(owner).or_default();
        if acct.bankroll.saturating_sub(acct.reserved) < notional {
            return Err(RejectReason::Insufficient);
        }
        acct.reserved = acct.reserved.saturating_add(notional);
        Ok(())
    }

    fn release(&mut self, owner: OwnerId, notional: u128) {
        let acct = self.accts.entry(owner).or_default();
        let give = notional.min(acct.reserved);
        acct.reserved -= give;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Price;

    fn owner() -> OwnerId {
        OwnerId::from_raw(1)
    }

    #[test]
    fn buy_yes_reserves_price_times_qty() {
        let g = TickGrid::cents();
        assert_eq!(
            reserve_notional(
                &g,
                Action::Buy,
                Outcome::Yes,
                Price::from_ticks(40),
                Qty::from_lots(10)
            ),
            400
        );
    }

    #[test]
    fn sell_yes_reserves_complement() {
        let g = TickGrid::cents();
        assert_eq!(
            reserve_notional(
                &g,
                Action::Sell,
                Outcome::Yes,
                Price::from_ticks(40),
                Qty::from_lots(100)
            ),
            6000
        );
    }

    #[test]
    fn insufficient_bankroll() {
        let mut led = CashLedger::new();
        led.credit(owner(), 100);
        assert!(led.reserve(owner(), 50).is_ok());
        assert_eq!(led.available(owner()), 50);
        assert_eq!(led.reserve(owner(), 51), Err(RejectReason::Insufficient));
        led.release(owner(), 50);
        assert_eq!(led.available(owner()), 100);
    }

    #[test]
    fn refund_on_price_improvement() {
        let mut led = CashLedger::new();
        led.credit(owner(), 6500);
        let g = TickGrid::cents();
        let locked = reserve_notional(
            &g,
            Action::Buy,
            Outcome::Yes,
            Price::from_ticks(65),
            Qty::from_lots(100),
        );
        assert_eq!(locked, 6500);
        led.reserve(owner(), locked).unwrap();
        let fill = crate::notional(Price::from_ticks(60), Qty::from_lots(100));
        led.release(owner(), locked - fill);
        assert_eq!(led.reserved(owner()), 6000);
        assert_eq!(led.available(owner()), 500);
    }

    #[test]
    fn noop_never_fails() {
        let mut n = NoopRisk;
        assert!(n.reserve(owner(), u128::MAX).is_ok());
    }
}
