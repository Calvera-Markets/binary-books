use crate::matching::MatchKind;
use crate::types::{Action, OrderId, Outcome, OwnerId, Price, Qty, Seq};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    InvalidTick,
    ZeroQty,
    NotFound,
    InvalidReduce,
    PostOnlyWouldTake,
    PostOnlyWithTif,
    FokUnfilled,
    Expired,
    MinSize,
    Closed,
    DuplicateEvent,
    Insufficient,
    InsufficientInventory,
}

/// One side of a fill. Intents are the original `(action, outcome)`, not the
/// canonical ladder, so settlement can mint / merge / transfer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillParty {
    pub order_id: OrderId,
    pub owner: OwnerId,
    pub action: Action,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BookEvent {
    Accepted {
        order_id: OrderId,
        seq: Seq,
    },
    Trade {
        taker: FillParty,
        maker: FillParty,
        yes_px: Price,
        no_px: Price,
        qty: Qty,
        kind: MatchKind,
    },
    Resting {
        order_id: OrderId,
        remaining: Qty,
    },
    Canceled {
        order_id: OrderId,
        remaining: Qty,
    },
    Rejected {
        order_id: OrderId,
        reason: RejectReason,
    },
}

impl BookEvent {
    pub fn trades(events: &[Self]) -> Vec<&Self> {
        events
            .iter()
            .filter(|e| matches!(e, BookEvent::Trade { .. }))
            .collect()
    }
}
