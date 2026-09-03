use crate::types::{Action, Outcome};

/// Settlement label derived from original intents. Not a second matcher:
/// every fill already happened on the dual-bid book.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MatchKind {
    /// Both parties are buying complementary outcomes. Split `$1` → YES + NO.
    Mint,
    /// Both parties are selling complementary outcomes. Merge YES + NO → `$1`.
    Merge,
    /// Buy vs sell of the same outcome. Transfer tokens + cash.
    Complementary,
}

/// Taker and maker original `(action, outcome)`.
///
/// Buy vs buy → mint, sell vs sell → merge, otherwise complementary.
/// Same-ladder intents never meet, so the mixed cases are always
/// complementary-outcome buys vs sells of one named token.
pub fn match_kind(taker: (Action, Outcome), maker: (Action, Outcome)) -> MatchKind {
    match (taker.0, maker.0) {
        (Action::Buy, Action::Buy) => MatchKind::Mint,
        (Action::Sell, Action::Sell) => MatchKind::Merge,
        _ => MatchKind::Complementary,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(taker_a: Action, taker_o: Outcome, maker_a: Action, maker_o: Outcome) -> MatchKind {
        match_kind((taker_a, taker_o), (maker_a, maker_o))
    }

    #[test]
    fn buy_vs_buy_is_mint() {
        assert_eq!(
            k(Action::Buy, Outcome::Yes, Action::Buy, Outcome::No),
            MatchKind::Mint
        );
        assert_eq!(
            k(Action::Buy, Outcome::No, Action::Buy, Outcome::Yes),
            MatchKind::Mint
        );
    }

    #[test]
    fn sell_vs_sell_is_merge() {
        assert_eq!(
            k(Action::Sell, Outcome::Yes, Action::Sell, Outcome::No),
            MatchKind::Merge
        );
        assert_eq!(
            k(Action::Sell, Outcome::No, Action::Sell, Outcome::Yes),
            MatchKind::Merge
        );
    }

    #[test]
    fn buy_vs_sell_same_outcome_is_complementary() {
        assert_eq!(
            k(Action::Buy, Outcome::Yes, Action::Sell, Outcome::Yes),
            MatchKind::Complementary
        );
        assert_eq!(
            k(Action::Sell, Outcome::Yes, Action::Buy, Outcome::Yes),
            MatchKind::Complementary
        );
        assert_eq!(
            k(Action::Buy, Outcome::No, Action::Sell, Outcome::No),
            MatchKind::Complementary
        );
        assert_eq!(
            k(Action::Sell, Outcome::No, Action::Buy, Outcome::No),
            MatchKind::Complementary
        );
    }

    #[test]
    fn eight_intent_pairs_collapse_to_three_kinds() {
        let mut mint = 0;
        let mut merge = 0;
        let mut comp = 0;
        for taker_a in [Action::Buy, Action::Sell] {
            for taker_o in [Outcome::Yes, Outcome::No] {
                for maker_a in [Action::Buy, Action::Sell] {
                    for maker_o in [Outcome::Yes, Outcome::No] {
                        match k(taker_a, taker_o, maker_a, maker_o) {
                            MatchKind::Mint => mint += 1,
                            MatchKind::Merge => merge += 1,
                            MatchKind::Complementary => comp += 1,
                        }
                    }
                }
            }
        }
        // 16 intent pairs: 4 buy-buy, 4 sell-sell, 8 mixed.
        assert_eq!((mint, merge, comp), (4, 4, 8));
    }
}
