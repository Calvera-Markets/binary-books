//! Append-only command log. Replay is bit-identical if the host supplies
//! the same ids and timestamps.

use crate::book::{BinaryBook, BookEvent, NewOrder};
use crate::types::{OrderId, Qty, TickGrid, Ts};

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
}
