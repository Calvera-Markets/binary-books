use binary_books::{
    Action, BinaryBook, MarketId, NewOrder, OrderId, Outcome, OwnerId, Price, Qty, Stp, TickGrid,
    TimeInForce,
};
use criterion::{criterion_group, criterion_main, Criterion};

fn order(id: u64, outcome: Outcome, px: u32, qty: u64) -> NewOrder {
    NewOrder {
        id: OrderId::from_raw(id),
        owner: OwnerId::from_raw(id),
        market: MarketId::from_raw(1),
        action: Action::Buy,
        outcome,
        limit: Price::from_ticks(px),
        qty: Qty::from_lots(qty),
        tif: TimeInForce::Gtc,
        post_only: false,
        stp: Stp::None,
    }
}

fn place_resting(c: &mut Criterion) {
    c.bench_function("place_1000_resting_yes", |b| {
        b.iter(|| {
            let mut book = BinaryBook::new(TickGrid::cents());
            for i in 1..=1000u64 {
                book.place(order(i, Outcome::Yes, 40, 1));
            }
            book
        })
    });
}

fn cancel_all(c: &mut Criterion) {
    c.bench_function("cancel_1000", |b| {
        b.iter_batched(
            || {
                let mut book = BinaryBook::new(TickGrid::cents());
                for i in 1..=1000u64 {
                    book.place(order(i, Outcome::Yes, 40, 1));
                }
                book
            },
            |mut book| {
                for i in 1..=1000u64 {
                    book.cancel(OrderId::from_raw(i));
                }
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

fn sweep(c: &mut Criterion) {
    c.bench_function("sweep_500_no_bids", |b| {
        b.iter_batched(
            || {
                let mut book = BinaryBook::new(TickGrid::cents());
                for i in 1..=500u64 {
                    book.place(order(i, Outcome::No, 40, 1));
                }
                book
            },
            |mut book| {
                book.place(order(501, Outcome::Yes, 60, 500));
            },
            criterion::BatchSize::SmallInput,
        )
    });
}

criterion_group!(benches, place_resting, cancel_all, sweep);
criterion_main!(benches);
