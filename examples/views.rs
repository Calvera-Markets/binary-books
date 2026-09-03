//! Same resting order, Kalshi vs Polymarket snapshots.

use binary_books::{
    venue, Action, BinaryBook, NewOrder, OrderId, Outcome, OwnerId, Price, Qty, Stp, TickGrid,
    TimeInForce,
};

fn main() {
    let mut book = BinaryBook::new(TickGrid::cents());
    book.place(NewOrder {
        id: OrderId::from_raw(1),
        owner: OwnerId::from_raw(1),
        market: binary_books::MarketId::from_raw(1),
        action: Action::Buy,
        outcome: Outcome::No,
        limit: Price::from_ticks(40),
        qty: Qty::from_lots(7),
        tif: TimeInForce::Gtc,
        post_only: false,
        stp: Stp::None,
    });

    let k = venue::kalshi::snapshot(&book);
    let yes = venue::polymarket::snapshot(&book, Outcome::Yes);
    let no = venue::polymarket::snapshot(&book, Outcome::No);

    println!(
        "Kalshi NO bid  {}¢ x {}",
        k.no[0].price.ticks(),
        k.no[0].qty.lots()
    );
    println!(
        "Poly YES ask   {}¢ x {}",
        yes.asks[0].price.ticks(),
        yes.asks[0].qty.lots()
    );
    println!(
        "Poly NO bid    {}¢ x {}",
        no.bids[0].price.ticks(),
        no.bids[0].qty.lots()
    );
}
