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
