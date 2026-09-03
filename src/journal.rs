//! Append-only command log. Replay is bit-identical if the host supplies
//! the same ids and timestamps.

use crate::book::{BinaryBook, BookEvent, NewOrder};
use crate::engine::{Engine, Place};
use crate::types::{EventId, MarketId, OrderId, Qty, TickGrid, Ts};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Place(NewOrder),
    Cancel(OrderId),
    Reduce { id: OrderId, qty: Qty },
    ExpireDue(Ts),
}

#[derive(Clone, Debug, Default)]
pub struct MemoryJournal {
    cmds: Vec<Command>,
}

impl MemoryJournal {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, cmd: Command) {
        self.cmds.push(cmd);
    }

    pub fn commands(&self) -> &[Command] {
        &self.cmds
    }
}

pub fn replay(grid: TickGrid, cmds: &[Command]) -> (BinaryBook, Vec<Vec<BookEvent>>) {
    let mut book = BinaryBook::new(grid);
    let mut out = Vec::with_capacity(cmds.len());
    for cmd in cmds {
        let events = match cmd {
            Command::Place(o) => book.place(o.clone()),
            Command::Cancel(id) => book.cancel(*id),
            Command::Reduce { id, qty } => book.reduce(*id, *qty),
            Command::ExpireDue(ts) => book.expire_due(*ts),
        };
        out.push(events);
    }
    (book, out)
}

/// Depth + next-seq checksum for replay equality.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineCommand {
    OpenEvent {
        id: EventId,
        grid: TickGrid,
        markets: Vec<MarketId>,
    },
    Place {
        event: EventId,
        market: MarketId,
        spec: Place,
    },
    Cancel {
        event: EventId,
        market: MarketId,
        id: OrderId,
    },
    Reduce {
        event: EventId,
        market: MarketId,
        id: OrderId,
        qty: Qty,
    },
    CloseEvent(EventId),
    ExpireDue {
        event: EventId,
        ts: Ts,
    },
}

pub fn replay_engine(min_order_size: Qty, cmds: &[EngineCommand]) -> Engine {
    let mut engine = Engine::new(min_order_size);
    for cmd in cmds {
        match cmd {
            EngineCommand::OpenEvent { id, grid, markets } => {
                let _ = engine.open_event(*id, grid.clone(), markets.clone());
            }
            EngineCommand::Place {
                event,
                market,
                spec,
            } => {
                let _ = engine.place(*event, *market, spec.clone());
            }
            EngineCommand::Cancel { event, market, id } => {
                let _ = engine.cancel(*event, *market, *id);
            }
            EngineCommand::Reduce {
                event,
                market,
                id,
                qty,
            } => {
                let _ = engine.reduce(*event, *market, *id, *qty);
            }
            EngineCommand::CloseEvent(id) => {
                let _ = engine.close_event(*id);
            }
            EngineCommand::ExpireDue { event, ts } => {
                let _ = engine.expire_due(*event, *ts);
            }
        }
    }
    engine
}

pub fn snapshot_hash(book: &BinaryBook) -> (Vec<(u32, u64, usize)>, Vec<(u32, u64, usize)>) {
    let yes = book
        .yes_bids()
        .depth()
        .into_iter()
        .map(|(p, q, n)| (p.ticks(), q.lots(), n))
        .collect();
    let no = book
        .no_bids()
        .depth()
        .into_iter()
        .map(|(p, q, n)| (p.ticks(), q.lots(), n))
        .collect();
    (yes, no)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, MarketId, Outcome, OwnerId, Price, Qty, Stp, TimeInForce};

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

    #[test]
    fn replay_matches_live() {
        let grid = TickGrid::cents();
        let mut live = BinaryBook::new(grid.clone());
        let mut journal = MemoryJournal::new();
        let cmds = [
            Command::Place(order(1, Outcome::Yes, 50, 10)),
            Command::Place(order(2, Outcome::No, 40, 4)),
            Command::Cancel(OrderId::from_raw(1)),
            Command::Place(order(3, Outcome::Yes, 60, 10)),
        ];
        let mut live_events = Vec::new();
        for c in &cmds {
            journal.record(c.clone());
            live_events.push(match c {
                Command::Place(o) => live.place(o.clone()),
                Command::Cancel(id) => live.cancel(*id),
                Command::Reduce { id, qty } => live.reduce(*id, *qty),
                Command::ExpireDue(ts) => live.expire_due(*ts),
            });
        }
        let (replayed, replay_events) = replay(grid, journal.commands());
        assert_eq!(live_events, replay_events);
        assert_eq!(snapshot_hash(&live), snapshot_hash(&replayed));
    }

    #[test]
    fn engine_replay_matches_live_depth() {
        let grid = TickGrid::cents();
        let eid = EventId::from_raw(1);
        let m = MarketId::from_raw(1);
        let spec = |owner, px, qty| Place {
            owner: OwnerId::from_raw(owner),
            action: Action::Buy,
            outcome: Outcome::Yes,
            limit: Price::from_ticks(px),
            qty: Qty::from_lots(qty),
            tif: TimeInForce::Gtc,
            post_only: false,
            stp: Stp::None,
        };
        let cmds = [
            EngineCommand::OpenEvent {
                id: eid,
                grid: grid.clone(),
                markets: vec![m],
            },
            EngineCommand::Place {
                event: eid,
                market: m,
                spec: spec(1, 40, 10),
            },
            EngineCommand::Place {
                event: eid,
                market: m,
                spec: spec(2, 50, 5),
            },
            EngineCommand::Reduce {
                event: eid,
                market: m,
                id: OrderId::from_raw(1),
                qty: Qty::from_lots(3),
            },
        ];
        let mut live = Engine::new(Qty::ONE);
        live.open_event(eid, grid, [m]).unwrap();
        live.place(eid, m, spec(1, 40, 10)).unwrap();
        live.place(eid, m, spec(2, 50, 5)).unwrap();
        live.reduce(eid, m, OrderId::from_raw(1), Qty::from_lots(3))
            .unwrap();
        let replayed = replay_engine(Qty::ONE, &cmds);
        let h = |e: &Engine| snapshot_hash(e.event(eid).unwrap().book(m).unwrap());
        assert_eq!(h(&live), h(&replayed));
    }
}
