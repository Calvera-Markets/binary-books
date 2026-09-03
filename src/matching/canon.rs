use crate::types::{Action, BookSide, Outcome, Price, TickGrid};

/// Map a trader intent onto a dual-bid ladder.
///
/// `price` is always in the **named outcome's** units (what the trader typed).
/// Sells flip to the complementary ladder at `1 - p`.
///
/// | action | outcome | rests on | at price        |
/// |--------|---------|----------|-----------------|
/// | Buy    | Yes     | YesBid   | `p`             |
/// | Sell   | No      | YesBid   | `complement(p)` |
/// | Buy    | No      | NoBid    | `p`             |
/// | Sell   | Yes     | NoBid    | `complement(p)` |
///
/// Kalshi's YES-priced `book_side` (bid ≡ YES, ask ≡ NO) is converted by the
/// Kalshi adapter *before* this function: an ask at YES-price `p` is
/// `Sell Yes @ p`, not a NO-leg quote.
pub fn canonicalize(
    action: Action,
    outcome: Outcome,
    price: Price,
    grid: &TickGrid,
) -> (BookSide, Price) {
    match (action, outcome) {
        (Action::Buy, Outcome::Yes) => (BookSide::YesBid, price),
        (Action::Sell, Outcome::No) => (BookSide::YesBid, grid.complement(price)),
        (Action::Buy, Outcome::No) => (BookSide::NoBid, price),
        (Action::Sell, Outcome::Yes) => (BookSide::NoBid, grid.complement(price)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TickGrid;

    fn cents() -> TickGrid {
        TickGrid::cents()
    }

    fn p(t: u32) -> Price {
        Price::from_ticks(t)
    }

    #[test]
    fn buy_yes_rests_on_yes_ladder() {
        let g = cents();
        assert_eq!(
            canonicalize(Action::Buy, Outcome::Yes, p(40), &g),
            (BookSide::YesBid, p(40))
        );
    }

    #[test]
    fn sell_no_is_yes_bid_at_complement() {
        let g = cents();
        // Sell NO at 60¢ (NO-leg) ≡ buy YES at 40¢.
        assert_eq!(
            canonicalize(Action::Sell, Outcome::No, p(60), &g),
            (BookSide::YesBid, p(40))
        );
    }

    #[test]
    fn buy_no_rests_at_named_price() {
        let g = cents();
        assert_eq!(
            canonicalize(Action::Buy, Outcome::No, p(40), &g),
            (BookSide::NoBid, p(40))
        );
    }

    #[test]
    fn sell_yes_is_no_bid_at_complement() {
        let g = cents();
        assert_eq!(
            canonicalize(Action::Sell, Outcome::Yes, p(60), &g),
            (BookSide::NoBid, p(40))
        );
        assert_eq!(
            canonicalize(Action::Sell, Outcome::Yes, p(1), &g),
            (BookSide::NoBid, p(99))
        );
    }

    #[test]
    fn all_four_intents_cover_both_ladders() {
        let g = cents();
        let yes = [
            canonicalize(Action::Buy, Outcome::Yes, p(55), &g),
            canonicalize(Action::Sell, Outcome::No, p(45), &g),
        ];
        let no = [
            canonicalize(Action::Buy, Outcome::No, p(45), &g),
            canonicalize(Action::Sell, Outcome::Yes, p(55), &g),
        ];
        assert!(yes
            .iter()
            .all(|&(s, px)| s == BookSide::YesBid && px == p(55)));
        assert!(no
            .iter()
            .all(|&(s, px)| s == BookSide::NoBid && px == p(45)));
    }
}
