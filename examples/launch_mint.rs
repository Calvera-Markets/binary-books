//! Empty book: buy YES 60 + buy NO 40 → mint at 60/40.

use binary_books::{
    Action, BinaryBook, BookEvent, MatchKind, NewOrder, OrderId, Outcome, OwnerId, Price, Qty, Stp,
    TickGrid, TimeInForce,
};

fn order(id: u64, outcome: Outcome, px: u32, qty: u64) -> NewOrder {
    NewOrder {
        id: OrderId::from_raw(id),
        owner: OwnerId::from_raw(id),
        market: binary_books::MarketId::from_raw(1),
        action: Action::Buy,
        outcome,
        limit: Price::from_ticks(px),
        qty: Qty::from_lots(qty),
        tif: TimeInForce::Gtc,
        post_only: false,
        stp: Stp::None,
    }
}

fn main() {
    let mut book = BinaryBook::new(TickGrid::cents());
    book.place(order(1, Outcome::Yes, 60, 100));
    let events = book.place(order(2, Outcome::No, 40, 100));
    for e in events {
        if let BookEvent::Trade {
            yes_px,
            no_px,
            qty,
            kind,
            ..
        } = e
        {
            assert_eq!(kind, MatchKind::Mint);
            println!(
                "mint {} @ YES {}¢ / NO {}¢",
                qty.lots(),
                yes_px.ticks(),
                no_px.ticks()
            );
        }
    }
    assert!(!book.is_crossed());
}
