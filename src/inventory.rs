//! Polymarket-style token execution on top of the matcher.
//!
//! The book still only emits [`BookEvent::Trade`]. This ledger turns those
//! labels into cash and outcome-token movements:
//!
//! - [`MatchKind::Mint`]: both pay cash, both receive complementary tokens
//! - [`MatchKind::Complementary`]: cash vs tokens of the same outcome
//! - [`MatchKind::Merge`]: both deliver tokens, both receive cash
//!
//! Off-book CTF ops: [`TokenLedger::split`] and [`TokenLedger::merge`].

use crate::book::{BookEvent, FillParty, RejectReason};
use crate::matching::MatchKind;
use crate::notional;
use crate::risk::CashLedger;
use crate::types::{Action, MarketId, Outcome, OwnerId, Price, Qty, TickGrid};
use std::collections::HashMap;

#[derive(Debug)]
pub struct TokenLedger {
    grid: TickGrid,
    cash: CashLedger,
    tokens: HashMap<(OwnerId, MarketId, Outcome), Qty>,
}

impl TokenLedger {
    pub fn new(grid: TickGrid) -> Self {
        Self {
            grid,
            cash: CashLedger::new(),
            tokens: HashMap::new(),
        }
    }

    pub fn grid(&self) -> &TickGrid {
        &self.grid
    }

    pub fn credit_cash(&mut self, owner: OwnerId, amount: u128) {
        self.cash.credit(owner, amount);
    }

    pub fn cash(&self, owner: OwnerId) -> u128 {
        self.cash.bankroll(owner)
    }

    pub fn tokens(&self, owner: OwnerId, market: MarketId, outcome: Outcome) -> Qty {
        self.tokens
            .get(&(owner, market, outcome))
            .copied()
            .unwrap_or(Qty::ZERO)
    }

    /// `$qty` collateral → `qty` YES + `qty` NO.
    pub fn split(
        &mut self,
        owner: OwnerId,
        market: MarketId,
        qty: Qty,
    ) -> Result<(), RejectReason> {
        let cost = notional(self.grid.collat(), qty);
        self.cash.debit(owner, cost)?;
        self.credit_tokens(owner, market, Outcome::Yes, qty);
        self.credit_tokens(owner, market, Outcome::No, qty);
        Ok(())
    }

    /// `qty` YES + `qty` NO → `$qty` collateral.
    pub fn merge(
        &mut self,
        owner: OwnerId,
        market: MarketId,
        qty: Qty,
    ) -> Result<(), RejectReason> {
        self.debit_tokens(owner, market, Outcome::Yes, qty)?;
        if let Err(e) = self.debit_tokens(owner, market, Outcome::No, qty) {
            self.credit_tokens(owner, market, Outcome::Yes, qty);
            return Err(e);
        }
        self.cash.credit(owner, notional(self.grid.collat(), qty));
        Ok(())
    }

    pub fn apply_events(
        &mut self,
        market: MarketId,
        events: &[BookEvent],
    ) -> Result<(), RejectReason> {
        for e in events {
            if let BookEvent::Trade {
                taker,
                maker,
                yes_px,
                no_px,
                qty,
                kind,
            } = e
            {
                self.apply_fill(market, *taker, *maker, *yes_px, *no_px, *qty, *kind)?;
            }
        }
        Ok(())
    }

    fn apply_fill(
        &mut self,
        market: MarketId,
        taker: FillParty,
        maker: FillParty,
        yes_px: Price,
        no_px: Price,
        qty: Qty,
        kind: MatchKind,
    ) -> Result<(), RejectReason> {
        match kind {
            MatchKind::Mint => {
                self.buy_leg(taker, market, yes_px, no_px, qty)?;
                if let Err(e) = self.buy_leg(maker, market, yes_px, no_px, qty) {
                    self.undo_buy_leg(taker, market, yes_px, no_px, qty);
                    return Err(e);
                }
            }
            MatchKind::Merge => {
                self.sell_leg(taker, market, yes_px, no_px, qty)?;
                if let Err(e) = self.sell_leg(maker, market, yes_px, no_px, qty) {
                    self.undo_sell_leg(taker, market, yes_px, no_px, qty);
                    return Err(e);
                }
            }
            MatchKind::Complementary => {
                let (buyer, seller) = if taker.action == Action::Buy {
                    (taker, maker)
                } else {
                    (maker, taker)
                };
                debug_assert_eq!(buyer.outcome, seller.outcome);
                self.sell_leg(seller, market, yes_px, no_px, qty)?;
                if let Err(e) = self.buy_leg(buyer, market, yes_px, no_px, qty) {
                    self.undo_sell_leg(seller, market, yes_px, no_px, qty);
                    return Err(e);
                }
            }
        }
        Ok(())
    }

    fn undo_buy_leg(
        &mut self,
        party: FillParty,
        market: MarketId,
        yes_px: Price,
        no_px: Price,
        qty: Qty,
    ) {
        let _ = self.debit_tokens(party.owner, market, party.outcome, qty);
        let px = price_of(party.outcome, yes_px, no_px);
        self.cash.credit(party.owner, notional(px, qty));
    }

    fn undo_sell_leg(
        &mut self,
        party: FillParty,
        market: MarketId,
        yes_px: Price,
        no_px: Price,
        qty: Qty,
    ) {
        let px = price_of(party.outcome, yes_px, no_px);
        let _ = self.cash.debit(party.owner, notional(px, qty));
        self.credit_tokens(party.owner, market, party.outcome, qty);
    }

    fn buy_leg(
        &mut self,
        party: FillParty,
        market: MarketId,
        yes_px: Price,
        no_px: Price,
        qty: Qty,
    ) -> Result<(), RejectReason> {
        let px = price_of(party.outcome, yes_px, no_px);
        self.cash.debit(party.owner, notional(px, qty))?;
        self.credit_tokens(party.owner, market, party.outcome, qty);
        Ok(())
    }

    fn sell_leg(
        &mut self,
        party: FillParty,
        market: MarketId,
        yes_px: Price,
        no_px: Price,
        qty: Qty,
    ) -> Result<(), RejectReason> {
        self.debit_tokens(party.owner, market, party.outcome, qty)?;
        let px = price_of(party.outcome, yes_px, no_px);
        self.cash.credit(party.owner, notional(px, qty));
        Ok(())
    }

    fn credit_tokens(&mut self, owner: OwnerId, market: MarketId, outcome: Outcome, qty: Qty) {
        let e = self
            .tokens
            .entry((owner, market, outcome))
            .or_insert(Qty::ZERO);
        *e = e.saturating_add(qty);
    }

    fn debit_tokens(
        &mut self,
        owner: OwnerId,
        market: MarketId,
        outcome: Outcome,
        qty: Qty,
    ) -> Result<(), RejectReason> {
        let have = self.tokens(owner, market, outcome);
        let Some(next) = have.checked_sub(qty) else {
            return Err(RejectReason::InsufficientInventory);
        };
        if next.is_zero() {
            self.tokens.remove(&(owner, market, outcome));
        } else {
            self.tokens.insert((owner, market, outcome), next);
        }
        Ok(())
    }
}

fn price_of(outcome: Outcome, yes_px: Price, no_px: Price) -> Price {
    match outcome {
        Outcome::Yes => yes_px,
        Outcome::No => no_px,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book::{BinaryBook, NewOrder};
    use crate::types::{OrderId, Stp, TimeInForce};

    fn oid(n: u64) -> OrderId {
        OrderId::from_raw(n)
    }
    fn owner(n: u64) -> OwnerId {
        OwnerId::from_raw(n)
    }
    fn mid() -> MarketId {
        MarketId::from_raw(1)
    }
    fn p(t: u32) -> Price {
        Price::from_ticks(t)
    }
    fn q(n: u64) -> Qty {
        Qty::from_lots(n)
    }

    fn order(id: u64, action: Action, outcome: Outcome, px: u32, qty: u64) -> NewOrder {
        NewOrder {
            id: oid(id),
            owner: owner(id),
            market: mid(),
            action,
            outcome,
            limit: p(px),
            qty: q(qty),
            tif: TimeInForce::Gtc,
            post_only: false,
            stp: Stp::None,
        }
    }

    #[test]
    fn split_and_merge_round_trip() {
        let mut led = TokenLedger::new(TickGrid::cents());
        led.credit_cash(owner(1), 10_000);
        led.split(owner(1), mid(), q(10)).unwrap();
        assert_eq!(led.cash(owner(1)), 9_000);
        assert_eq!(led.tokens(owner(1), mid(), Outcome::Yes), q(10));
        assert_eq!(led.tokens(owner(1), mid(), Outcome::No), q(10));
        led.merge(owner(1), mid(), q(10)).unwrap();
        assert_eq!(led.cash(owner(1)), 10_000);
        assert_eq!(led.tokens(owner(1), mid(), Outcome::Yes), Qty::ZERO);
    }

    #[test]
    fn mint_fill_creates_tokens() {
        let mut book = BinaryBook::new(TickGrid::cents());
        let mut led = TokenLedger::new(TickGrid::cents());
        led.credit_cash(owner(1), 10_000);
        led.credit_cash(owner(2), 10_000);
        book.place(order(1, Action::Buy, Outcome::No, 40, 100));
        let ev = book.place(order(2, Action::Buy, Outcome::Yes, 60, 100));
        led.apply_events(mid(), &ev).unwrap();
        assert_eq!(led.tokens(owner(2), mid(), Outcome::Yes), q(100));
        assert_eq!(led.tokens(owner(1), mid(), Outcome::No), q(100));
        assert_eq!(led.cash(owner(2)), 10_000 - 6_000);
        assert_eq!(led.cash(owner(1)), 10_000 - 4_000);
        assert_eq!(led.cash(owner(1)) + led.cash(owner(2)), 10_000);
    }

    #[test]
    fn complementary_transfers_tokens() {
        let mut book = BinaryBook::new(TickGrid::cents());
        let mut led = TokenLedger::new(TickGrid::cents());
        led.credit_cash(owner(1), 10_000);
        led.split(owner(1), mid(), q(50)).unwrap();
        let after_split = led.cash(owner(1));
        book.place(order(1, Action::Sell, Outcome::Yes, 60, 50));
        led.credit_cash(owner(2), 10_000);
        let ev = book.place(order(2, Action::Buy, Outcome::Yes, 60, 50));
        led.apply_events(mid(), &ev).unwrap();
        assert_eq!(led.tokens(owner(1), mid(), Outcome::Yes), Qty::ZERO);
        assert_eq!(led.tokens(owner(2), mid(), Outcome::Yes), q(50));
        assert_eq!(led.cash(owner(1)), after_split + 3_000);
        assert_eq!(led.cash(owner(2)), 10_000 - 3_000);
    }

    #[test]
    fn merge_fill_burns_tokens() {
        let mut book = BinaryBook::new(TickGrid::cents());
        let mut led = TokenLedger::new(TickGrid::cents());
        led.credit_cash(owner(1), 10_000);
        led.credit_cash(owner(2), 10_000);
        led.split(owner(1), mid(), q(40)).unwrap();
        led.split(owner(2), mid(), q(40)).unwrap();
        book.place(order(1, Action::Sell, Outcome::No, 40, 40));
        let ev = book.place(order(2, Action::Sell, Outcome::Yes, 60, 40));
        led.apply_events(mid(), &ev).unwrap();
        assert_eq!(led.tokens(owner(1), mid(), Outcome::No), Qty::ZERO);
        assert_eq!(led.tokens(owner(2), mid(), Outcome::Yes), Qty::ZERO);
        // Each still holds the other token from split; cash: paid 4000 to split,
        // got fill (40*40=1600 for NO seller, 60*40=2400 for YES seller).
        assert_eq!(led.cash(owner(1)), 10_000 - 4_000 + 1_600);
        assert_eq!(led.cash(owner(2)), 10_000 - 4_000 + 2_400);
    }

    #[test]
    fn sell_without_tokens_fails_at_apply() {
        let mut book = BinaryBook::new(TickGrid::cents());
        let mut led = TokenLedger::new(TickGrid::cents());
        led.credit_cash(owner(2), 10_000);
        book.place(order(1, Action::Sell, Outcome::Yes, 60, 10));
        let ev = book.place(order(2, Action::Buy, Outcome::Yes, 60, 10));
        assert_eq!(
            led.apply_events(mid(), &ev),
            Err(RejectReason::InsufficientInventory)
        );
    }
}
