# binary-books

Deterministic matching engine for binary prediction markets.

This is a library, not a venue. It does not talk to a network, hold wallets, or
settle on a chain. Hosts drive it with `place` / `cancel` and consume the event
log.

## One matcher, two adapters

Kalshi and Polymarket use the **same** match rule:

> A bid to buy YES at `p` crosses a bid to buy NO at `q` iff `p + q ≥ 1`.

They do **not** get separate matching engines. Differences are order vocabulary
and book display:

| | Kalshi | Polymarket |
|---|---|---|
| Snapshot | YES bids + NO bids (asks implied) | Bid/ask per outcome token |
| Ingest | `book_side` bid≡YES / ask≡NO | `(token, BUY/SELL, price)` |
| Fill DTO | cash pair | same fill + `MatchKind` |
| Matcher | `BinaryBook` | `BinaryBook` |

See `PLAN.md` for why forking the matcher would be the wrong split, and for
mint / merge / complementary settlement labels.

## Canonical book

Each binary market stores two bid ladders. Trader prices are in the **named
outcome's** units; sells land on the complementary ladder at `1 − p`.

```
Buy YES  p  →  yes_bids[p]
Sell NO  p  →  yes_bids[1-p]
Buy NO   q  →  no_bids[q]
Sell YES p  →  no_bids[1-p]
```

Fills are maker-priced. A YES bid at 65 against a resting NO bid at 40 trades
at 60/40 (taker improvement). After every accepted command the book is not
crossed: `best_yes + best_no < 1` (or a side is empty).

## Quick start

```rust
use binary_books::*;

let mut book = BinaryBook::new(TickGrid::cents());
book.place(NewOrder { /* buy NO @ 40 */ .. });
let events = book.place(NewOrder { /* buy YES @ 60 */ .. });
// → Trade { yes_px: 60, no_px: 40, kind: Mint, .. }
```

Kalshi-style snapshot (bids only):

```rust
let snap = binary_books::venue::kalshi::snapshot(&book);
// snap.yes, snap.no
```

Polymarket-style snapshot (bid/ask for one token):

```rust
let yes = binary_books::venue::polymarket::snapshot(&book, Outcome::Yes);
// yes.bids, yes.asks  (asks = complement of NO bids)
```

A resting buy-NO at 40¢ is a Kalshi NO bid at 40 and a Polymarket YES ask at 60.

## Multi-outcome events

An `Event` owns N independent `BinaryBook`s (winner-takes-all). Matching is
still per book. `Event::yes_ask_basket` walks implied YES asks so a later
NegRisk / collateral-return layer can price a complete set. That conversion
is not matching.

## Examples

```
cargo run --example launch_mint
cargo run --example views
```

## Invariants (tests)

`cargo test` covers FIFO, mint/merge/complementary labels, TIF, STP, adapter
complements, banded ticks, journal replay, and a proptest that random order
streams never leave the book crossed.
