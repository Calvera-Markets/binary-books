//! Property tests for matching invariants (PLAN.md).

use binary_books::{
    Action, BinaryBook, BookEvent, NewOrder, Outcome, OwnerId, Price, Qty, Stp, TickGrid,
    TimeInForce, venue,
};
use proptest::prelude::*;

#[derive(Clone, Debug)]
enum Op {
    Place {
        owner: u64,
        buy: bool,
        yes: bool,
        px: u32,
        qty: u64,
    },
    Cancel(u64),
}

fn op_strategy() -> impl Strategy<Value = Op> {
    prop_oneof![
        (
            1u64..8,
            any::<bool>(),
            any::<bool>(),
            1u32..99,
            1u64..20
        )
            .prop_map(|(owner, buy, yes, px, qty)| Op::Place {
                owner,
                buy,
                yes,
                px,
                qty,
            }),
        (1u64..40).prop_map(Op::Cancel),
    ]
}

fn new_order(id: u64, op: &Op) -> Option<NewOrder> {
    let Op::Place {
        owner,
        buy,
        yes,
        px,
        qty,
    } = op
    else {
        return None;
    };
    Some(NewOrder {
        id: binary_books::OrderId::from_raw(id),
        owner: OwnerId::from_raw(*owner),
        market: binary_books::MarketId::from_raw(1),
        action: if *buy { Action::Buy } else { Action::Sell },
        outcome: if *yes { Outcome::Yes } else { Outcome::No },
        limit: Price::from_ticks(*px),
        qty: Qty::from_lots(*qty),
        tif: TimeInForce::Gtc,
        post_only: false,
        stp: Stp::None,
    })
}

proptest! {
    #[test]
    fn no_cross_and_complement_views(ops in prop::collection::vec(op_strategy(), 0..50)) {
        let mut book = BinaryBook::new(TickGrid::cents());
        let mut next_id = 1u64;
        for op in &ops {
            match op {
                Op::Place { .. } => {
                    if let Some(o) = new_order(next_id, op) {
                        let events = book.place(o);
                        next_id += 1;
                        for e in &events {
                            if let BookEvent::Trade { yes_px, no_px, qty, .. } = e {
                                prop_assert_eq!(
                                    yes_px.ticks() + no_px.ticks(),
                                    book.grid().collat().ticks()
                                );
                                prop_assert!(!qty.is_zero());
                            }
                        }
                    }
                }
                Op::Cancel(id) => {
                    let _ = book.cancel(binary_books::OrderId::from_raw(*id));
                }
            }
            prop_assert!(!book.is_crossed());
            let k = venue::kalshi::snapshot(&book);
            let yes = venue::polymarket::snapshot(&book, Outcome::Yes);
            prop_assert_eq!(k.yes.len(), yes.bids.len());
            prop_assert_eq!(k.no.len(), yes.asks.len());
            for (no_bid, yes_ask) in k.no.iter().zip(yes.asks.iter()) {
                prop_assert_eq!(
                    no_bid.price.ticks() + yes_ask.price.ticks(),
                    book.grid().collat().ticks()
                );
                prop_assert_eq!(no_bid.qty, yes_ask.qty);
                prop_assert_eq!(no_bid.orders, yes_ask.orders);
            }
        }
    }
}
