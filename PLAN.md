# Binary prediction-market matching engine

Greenfield library in this repo. Scope is an in-memory, deterministic **matching engine** for binary (and linked multi-outcome) prediction markets — not a full exchange, wallet, or chain settlement layer. Settlement, fees, and networking plug in later against a stable event log.

## Goal

Build a CLOB that treats a binary market as **one economic instrument with two complementary views**, not two independent books. Matching must implement the rule that makes Polymarket and Kalshi work:

> A bid to buy YES at `p` crosses a bid to buy NO at `q` iff `p + q ≥ 1`.

Every other “structure” (Kalshi’s yes-bid/no-bid API, Polymarket’s YES bid/ask tab, mint vs merge vs transfer) is a **venue adapter** on top of that rule — not a second matcher.

---

## Kalshi vs Polymarket: one engine, two adapters

The review question: should Kalshi and Polymarket be **separate implementations**?

**Call: do not fork the matching engine. Do fork the venue surface.**

Two `BinaryBook`s (or two match loops) would be the wrong split. The differences between the venues are not in *who matches whom* or *at what price*. They are in how an order is *named* on the way in, how the book is *shown* on the way out, and how a fill is *settled* by the host. Duplicating the `p + q ≥ collat` loop, FIFO queues, FOK atomicity, and the no-cross invariant would create two copies of the bug-sensitive core and let them drift.

What was checked, difference by difference:

| Concern | Kalshi | Polymarket | Where it lives |
|---|---|---|---|
| Matching rule | YES bid `p` crosses NO bid `q` iff `p+q ≥ 1` | Same identity; YES/NO books are mirrors | **Engine** (one loop) |
| Fill price | Maker price; taker improvement | Maker price; taker improvement | **Engine** |
| Priority | Price-time FIFO | Price-time FIFO | **Engine** (HIP-4 side-time is a later policy, not a venue fork) |
| TIF | GTC, IOC, FOK, post-only | GTC, GTD, FOK, FAK, post-only | **Engine** (shared TIF enum) |
| Order vocabulary | `book_side` bid≡YES / ask≡NO; or action×side | `token_id` + BUY/SELL | **Adapter** |
| Book snapshot | Yes bids + no bids only; asks implied | Bid/ask per outcome token | **Adapter** |
| Tick grid | Often tapered bands (`price_ranges`) | Linear `0.1`…`0.0001` (plus `0.0025` sports) | **Engine policy** (`TickGrid`), not a matcher fork |
| Settlement | Cash longs/shorts; every fill locks `$1` | `COMPLEMENTARY` / `MINT` / `MERGE` on tokens | **Adapter label** + host ledger. Engine emits original intents. |
| Inventory | No outcome tokens | ERC-1155 YES/NO balances | **Host**, not the book |
| Multi-outcome | Separate binaries + account Collateral Return | N binaries + NegRisk conversion | **Not matching.** Event container + later conversion ops. |

Live evidence that the books are the same instrument: a 7-day Polymarket WebSocket reconstruction (Gebele et al., 2026) found YES and NO books were exact complements except 20 isolated API desyncs. Kalshi’s public docs state the same identity and therefore omit asks from the orderbook endpoint.

What *would* justify a second engine, and is not true here:

- A different match rule (e.g. combinatorial fills across outcomes).
- A different priority axiom that cannot be a policy flag.
- A book that is allowed to rest crossed on one venue and not the other.

None of those apply. Collateral Return and NegRisk conversion are position transforms **after** matching; they do not belong inside `place()`.

So the crate splits like this:

```
BinaryBook          // the only matcher
  ├─ venue::kalshi      // ingest + dual-bid snapshot + cash-style fill DTO
  └─ venue::polymarket  // ingest + per-token bid/ask snapshot + MatchKind DTO
```

The adapters are real modules with their own types and tests. They do **not** own ladders, do **not** run a second match loop, and must not be able to violate the no-cross invariant on their own. If a future venue needs a genuinely different matcher (parlays, LMSR), it gets a new engine type — it does not special-case inside `BinaryBook`.

---

## Research: how live venues actually book this

### The identity

A binary contract pays `$1` or `$0`. One YES + one NO is always worth `$1` of collateral (split/merge). Therefore:

| Intent              | Canonical equivalent        |
|---------------------|-----------------------------|
| Buy YES at `p`      | Sell NO at `1 − p`          |
| Sell YES at `p`     | Buy NO at `1 − p`           |
| Buy NO at `q`       | Sell YES at `1 − q`         |
| Sell NO at `q`      | Buy YES at `1 − q`          |

This is not a display trick. It is the matching invariant. Independent YES and NO books that do **not** cross-match are arbitrageable and go idle at launch (nobody holds inventory yet).

### Kalshi — dual-bid book, cash ledger

- Fully centralized CLOB (CFTC DCM). No tokens.
- Public orderbook returns **only bids**: `yes_dollars[]` and `no_dollars[]`. Asks are implied: `best YES ask = 1 − best NO bid`.
- Internally they collapsed action×side into one bit: `book_side`: **bid ≡ YES, ask ≡ NO**. An order at price `p` with `outcome_side=no` matches an order at the **same** `p` with `outcome_side=yes` (single YES-priced book).
- Selling YES collateralizes the short: reserve `(1 − p) × qty`. Buying YES reserves `p × qty`. Every fill is “one party long YES, the other long NO, $1 locked.”
- Tick grids are per-market and often **tapered** (finer near 0 and 1). Prices are fixed-point dollar strings, not floats.
- TIF: GTC, IOC, FOK. Post-only, reduce-only, STP (`taker_at_cross`, etc.).
- Multi-outcome events are **separate binary contracts** plus account-level **Collateral Return** (net redundant payoff). The matching engine does not combinatorially match across outcomes.

### Polymarket — token books, three settlement paths, same matching rule

- Off-chain CLOB operator, on-chain CTF settlement (Polygon).
- UI looks like two books (YES bid/ask, NO bid/ask). They are mirrors of one book.
- Because tokens exist, a fill has three *settlement* paths (CTFExchange `MatchType`):

  | Kind            | Orders                         | What happens                                      |
  |-----------------|--------------------------------|---------------------------------------------------|
  | `COMPLEMENTARY` | Buy YES vs sell YES            | Transfer tokens + cash. Supply unchanged.         |
  | `MINT`          | Buy YES vs buy NO, `p+q ≥ 1`   | Split `$1` → 1 YES + 1 NO. Creates inventory.     |
  | `MERGE`         | Sell YES vs sell NO, `p+q ≤ 1` | Merge 1 YES + 1 NO → `$1`. Destroys inventory.    |

  `MERGE` is `MINT` on the inverted view. `COMPLEMENTARY` is “buy YES vs buy NO” after canonicalizing the sell. **One matching rule, three settlement labels.**
- Orders are EIP-712 intents; price is implicit (`makerAmount / takerAmount`). TIF: GTC, GTD, FOK, FAK. Post-only. Tick sizes `0.1` … `0.0001` plus `0.0025` on some sports.
- **NegRisk** (categorical events, e.g. elections): N linked binary markets, exactly one YES wins. Identities `Σ YES_i ≡ 1` and `Σ_{k∈S} NO_k ≡ (|S|−1) + Σ_{j∉S} YES_j`. Conversion is a **position transform**, not a matching-engine cross. The engine still matches per binary market.

### Hyperliquid HIP-4 — merged book, price-side-time

- YES and NO books of one outcome are merged to share liquidity (same complement identity).
- Priority generalizes from price-time to **price-side-time**: at the same merged price, resting **sells (inventory)** sort ahead of resting **buy-duals**. This only matters if the engine distinguishes “I am selling tokens I hold” from “I am buying the complement.”
- Questions group mutually exclusive outcomes; `negateOutcome` / `mergeQuestion` are off-book conversions (NegRisk analogues).

---

## Architecture

### Unifying model

Store **two bid ladders per binary market**, in each outcome’s own price:

```
BinaryBook
  yes_bids: Ladder   // buy YES, descending price
  no_bids:  Ladder   // buy NO,  descending price
```

Incoming intents are canonicalized onto a ladder **before** matching:

Prices are in the **named outcome's** units (what the trader typed):

| `(action, outcome, price)` | Rests on        | At price        |
|----------------------------|-----------------|-----------------|
| Buy YES `p`                | `yes_bids`      | `p`             |
| Sell NO `p`                | `yes_bids`      | `1 − p`         |
| Buy NO `q`                 | `no_bids`       | `q`             |
| Sell YES `p`               | `no_bids`       | `1 − p`         |

Kalshi YES-priced `book_side` (bid ≡ YES, ask ≡ NO) is converted by the Kalshi adapter *before* this table.

Matching an incoming YES bid at `p`:

```
while remaining > 0:
    q = best_no_bid
    if q is None or p + q < collat: break
    fill qty = min(remaining, head.size)
    yes_fill_price = collat - q     # implied YES ask = maker
    no_fill_price  = q
    # taker improvement if p > yes_fill_price
rest remainder on yes_bids[p]
```

Symmetric for incoming NO bids. After every accepted command:

```
best_yes_bid + best_no_bid < collat    (or a side is empty)
```

That is the **no-cross invariant**. A crossed book is a matcher bug.

### Venue adapters (not a second matcher)

- **Kalshi adapter** — ingest `outcome_side` / `book_side` (or legacy action×side); snapshot yes bids + no bids; fill DTO in cash-pair form (taker long YES ⇔ maker long NO at complementary prices).
- **Polymarket adapter** — ingest `(token_id, BUY/SELL, price)` against a YES or NO token; snapshot bid/ask for the requested token; fill DTO carries `MatchKind`.
- **YES-priced CLOB snapshot** — bids = `yes_bids`; asks = complement(`no_bids`). Shared helper both adapters may use.

Adapters never own orders. They translate and project.

### Match kind (settlement label, not a second matcher)

Each fill records original intents of taker and maker:

| Taker           | Maker           | Kind            | Ledger hint                         |
|-----------------|-----------------|-----------------|-------------------------------------|
| Buy YES         | Buy NO          | `Mint`          | Lock `$1`, issue YES to T, NO to M  |
| Buy YES         | Sell YES        | `Complementary` | Cash T→M, YES M→T                   |
| Sell YES        | Sell NO         | `Merge`         | Burn YES+NO, pay both               |
| Sell YES        | Buy YES         | `Complementary` | (taker is the seller)               |
| …               | …               | (the other 4 are the same three) |                     |

If the host has **no token inventory** (Kalshi mode), every fill is economically `Mint` (cash in, positions out). The label is still emitted so a Polymarket-style settler can switch on it.

### Event / multi-outcome

```
Event                         // mutually exclusive, exhaustive
  markets: [BinaryBook; N]    // one book per outcome
  constraint: WinnerTakesAll  // Σ YES_i ≡ 1 at resolution
```

**v1 matching stays per binary book.** Cross-outcome “buy the whole YES basket for < $1” is arbitrage / conversion, not price-time matching. What v1 *does* provide:

- `Event` as the unit that owns N books and a shared sequence number.
- Snapshot helper: executable cost of `Σ YES_i` and `Σ NO_i` by walking each book (depth-aware), so a later NegRisk/collateral-return layer has a correct primitive.
- Explicit non-goal for v1: combinatorial matching, `convertPositions`, Kalshi Collateral Return.

### Engine shape

Classic exchange core, library-form:

```
venue adapter (kalshi | polymarket)
    → command (place / cancel / reduce / replace)
        → Engine (routes by market_id, single-threaded)
            → BinaryBook (pure function on state)
                → EventLog (OrderAccepted | Trade | Canceled | Rejected | Resting)
                    → venue adapter (snapshot / fill DTO)
```

- One thread (or one shard) per book. No locks on the hot path.
- Deterministic: same command stream → same fills, same resting book, same ids.
- Integer **ticks** and **lots** only. No floats on the matching path.
- Host owns accounts, collateral, persistence. The engine can optionally reject on a `RiskPort` (reserve check) if the host implements it; default is “always accept” so unit tests stay ledger-free.

```
binary-books/                 # rust library crate
  PLAN.md
  src/
    types/                    # Price, Qty, TickGrid, Order, Ids, Outcome, Tif
    book/                     # Level, Ladder, BinaryBook  ← the only matcher
    matching/                 # canonicalize, match kind, priority policy, STP
    venue/
      kalshi.rs               # ingest + dual-bid snapshot + cash fill DTO
      polymarket.rs           # ingest + per-token bid/ask + MatchKind DTO
    event/                    # Event of N binaries, basket cost helper
    engine.rs                 # multi-market router + sequence
    journal.rs                # later: append-only command log + replay
```

Language: **Rust**. Matching engines want no GC pause, explicit ownership of queues, and exhaustive enums for TIF/reject reasons. Python (`pyclob`) is a fine simulator; it is the wrong core for something named to be reused as a matcher.

---

## Key decisions

1. **One matcher, two venue adapters.** Kalshi and Polymarket do not get separate `BinaryBook` implementations. They get separate ingest/snapshot/fill DTO modules over the same dual-bid engine. Fork the matcher only if a venue needs a different match rule.
2. **Dual-bid canonical storage, not two independent CLOBs.** Independent books cannot mint at launch and will desync. Dual-bid is Kalshi-native and projects to Polymarket’s mirrored bid/ask without a second matcher.
3. **One matching rule: `p_yes + p_no ≥ collat`.** Mint/merge/transfer are labels derived from original `(action, outcome)`, not separate engines.
4. **Maker-price execution.** Fill YES at `collat − maker_no_price` (and vice versa). Taker limit may be strictly better; surplus goes to the taker, not the house. Matches both venues and `pyclob`.
5. **Integer ticks + `TickGrid` from day one.** `complement(p) = collat − p` is exact only on a grid. Linear cents, deci-cents, and Kalshi-style tapered bands are `TickGrid` policies, not if-statements in the matcher.
6. **Price-time FIFO default; price-side-time as a policy.** HIP-4’s “inventory sells beat dual buys at the same merged price” is a `Priority` enum, not the v1 default. Kalshi does not need it.
7. **Per-book matching for multi-outcome.** Event is a container + sum-to-one helpers. NegRisk conversion is a later crate/module, not v1 matching.
8. **Library, not a server.** `place`/`cancel` return an event batch. Persistence, HTTP, and wallets come after the matcher is correct.
9. **No floats, no threads, no persistence in v1.** Determinism first. Journal/replay is an explicit later commit so tests can freeze a stream.

---

## Out of scope (v1)

- Networking, auth, EIP-712, wire-level Kalshi/Polymarket API compatibility
- On-chain settlement / CTF / relayer
- Fees, funding, liquidations
- Combinatorial / parlay matching
- NegRisk `convertPositions` / HIP-4 `negateOutcome`
- Multi-shard sequencing, replication
- AMM / LMSR hybrid

---

## Invariants the tests must keep true

After every command that is not a reject:

1. **No-cross:** `best_yes_bid + best_no_bid < collat` (or a side empty).
2. **Conservation:** each `Trade` has `yes_qty == no_qty` (one YES issued iff one NO issued, or one YES transferred iff cash transferred).
3. **Complement views:** Polymarket YES asks == complement(Kalshi no bids), same sizes, same order counts.
4. **Price-time:** at a level, earlier `seq` fills first (unless `Priority::PriceSideTime` is on).
5. **Tick:** every resting price ∈ `TickGrid`; `complement(complement(p)) == p`.
6. **TIF:** FOK either fully fills or leaves the book unchanged; IOC never rests; post-only never takes.
7. **Idempotent cancel:** cancel of unknown/terminal id is a no-op reject, not a panic.
8. **Determinism:** replay of the command log reproduces fills, resting quantities, and next order id.
9. **Adapter purity:** Kalshi and Polymarket snapshots of the same `BinaryBook` are complements of each other; neither adapter mutates book state except by calling `place`/`cancel` on the engine.

---

## Commit stream

Each commit compiles, has tests, and is reviewable on its own. Messages below are the actual subjects to use.

### C0 — `docs: matching engine plan`

- `PLAN.md` (this file), including the Kalshi vs Polymarket adapter decision.

### C1 — `chore: rust crate skeleton and integer money types`

- `Cargo.toml`, `src/lib.rs`, `src/types/{price,qty,ids}.rs`
- `Price` = newtype over `u32` ticks. `Qty` = newtype over `u64` lots.
- `TickGrid`: `contains`, `complement`, `clamp`, linear constructor `linear(collat, step)`.
- Reject `0` and `collat` as tradable prices by default (Kalshi 1–99).
- Tests: complement involution, grid membership, overflow-safe `price * qty` via `u128` notional helper.

### C2 — `feat: order domain types and canonical book side`

- `Outcome { Yes, No }`, `Action { Buy, Sell }`, `BookSide { YesBid, NoBid }`
- `canonicalize(action, outcome, price, grid) -> (BookSide, Price)` using the table above
- `TimeInForce { Gtc, Ioc, Fok, Gtd { expire_at } }`, `PostOnly`, `OrderStatus`
- `Order` struct: id, owner, market, original intent, canonical side/price, remaining qty, seq, tif
- Tests: all 4 intent pairs map onto the two ladders; `Sell Yes @ p` is `NoBid @ complement(p)`.

### C3 — `feat: fifo price level`

- `Level`: `VecDeque<OrderId>` + aggregate `qty` + optional `order_count`
- `push_back`, `pop_front`, `remove(id)` (cancel-by-id inside the level), `reduce(id, qty)`
- Quantity decrease keeps time priority; increase is **not** offered here (replace is a later cancel+new).
- Tests: FIFO, remove from middle, reduce without losing place, empty level detection.

### C4 — `feat: bid ladder`

- `Ladder`: `BTreeMap<Price, Level>` with `best()` = max price, `walk_down()` iterator
- Insert/rest, cancel, reduce, aggregate depth snapshot `Vec<(Price, Qty, count)>`
- Tests: best-price, multi-level walk, cancel last order at a price drops the level.

### C5 — `feat: binary book matching (gtc only)`

- `BinaryBook { yes_bids, no_bids, orders: HashMap<OrderId, Order>, next_seq }`
- `place(order) -> Vec<Event>` implementing the `p + q ≥ collat` loop, maker-price fills, remainder rests
- Events: `Accepted`, `Trade { taker, maker, yes_px, no_px, qty, kind }`, `Resting`
- GTC only. No STP, no inventory.
- Tests (golden):
  - Buy YES 60 vs buy NO 40 → full mint at 60/40
  - Buy YES 65 vs resting NO 40 → taker improvement, YES fill 60
  - Buy YES 50 vs resting NO 40 → no trade, both rest, book not crossed
  - Partial fill, remainder rests
  - Multi-level sweep of NO bids by a large YES bid
  - Incoming sell YES @ 60 ≡ buy NO @ 40, matches yes bids ≥ 60

### C6 — `feat: classify mint, merge, and complementary fills`

- `MatchKind { Mint, Merge, Complementary }` from original intents
- Tests: buy-yes vs buy-no = Mint; buy-yes vs sell-yes = Complementary; sell-yes vs sell-no = Merge; the four remaining combinations collapse to those three.

### C7 — `feat: cancel and reduce`

- `cancel(id)`, `cancel_all(owner)`, `reduce(id, new_qty)` (new_qty < remaining, keep priority)
- Reduce-to-zero = cancel. Unknown id → `Rejected { NotFound }`
- Tests: cancel unfilled, cancel remainder after partial, reduce keeps FIFO place, cancel_all.

### C8 — `feat: time-in-force ioc, fok, post-only`

- IOC/FAK: match then discard remainder
- FOK: speculative match against a copy or two-phase (compute fillable qty, abort if `< order.qty`); book unchanged on kill
- Post-only: if it would trade, reject; else rest. Illegal with FOK/IOC
- GTD stored on the order; expiry is a command `expire_due(now)` so the book stays deterministic (no wall-clock inside `place`)
- Tests: FOK success/kill atomicity, IOC partial, post-only reject on cross, GTD expiry command.

### C9 — `feat: self-trade prevention`

- `Stp { None, SkipMaker, CancelTaker, CancelBoth }` (Kalshi’s `taker_at_cross` ≈ cancel taker when it would trade with self)
- Same `owner` on both sides of a potential fill
- Tests: skip to next maker, cancel taker and leave maker, no fill emitted.

### C10 — `feat: kalshi and polymarket venue adapters`

Separate **adapter** modules, not separate engines:

- `venue::kalshi`: ingest (`BookSide` / `outcome_side` + price in that outcome’s units) → engine `Order`; snapshot `{ yes: depth, no: depth }`; fill DTO as a cash pair.
- `venue::polymarket`: ingest `(outcome token, BUY/SELL, price)` → engine `Order`; snapshot bid/ask for that token; fill DTO includes `MatchKind`.
- Shared helper for YES-priced CLOB `{ bids, asks }`.
- Tests: one resting buy-NO @ 40 appears as Kalshi no-bid 40, Polymarket YES ask 60, Polymarket NO bid 40. Adapter snapshots stay complements. No second match loop in either module.

### C11 — `feat: tick grids including tapered bands`

- `TickGrid::banded(Vec<{start, end, step}>)` matching Kalshi `price_ranges`
- Place rejects off-grid prices (`Rejected { InvalidTick }`)
- Tests: linear 1¢, deci-cent, tapered (0.001 on edges, 0.01 in the middle); complement stays on-grid.

### C12 — `feat: event of n binary books`

- `Event { id, markets: IndexMap<MarketId, BinaryBook>, collat }`
- Route `place`/`cancel` by `market_id`
- `BasketQuote::yes_asks(event) -> (cost, max_qty)` walks all YES implied asks for a unit complete set (executable `Σ YES`)
- Same for complete NO basket (`n − 1` bound)
- Tests: 3-candidate event, independent matching per market, basket cost uses implied asks; no cross-market fill in this commit.

### C13 — `feat: engine facade, sequence numbers, reject reasons`

- `Engine` owns many `Event`s, allocates `OrderId`s, stamps `seq` / `ts` supplied by the host (host clock, not `SystemTime` inside the matcher)
- Stable `RejectReason` enum: `InvalidTick`, `MinSize`, `NotFound`, `PostOnlyWouldTake`, `FokUnfilled`, `Expired`, `Insufficient` (for optional risk port), `Closed`
- `min_order_size` per market
- Tests: two events, id uniqueness, closed market rejects.

### C14 — `feat: optional cash reserve port (kalshi-style)`

- Trait `RiskPort { fn reserve(&mut self, owner, notional) -> Result; fn release(...) }`
- Buy YES reserves `px * qty`; sell YES / buy NO reserves `complement(px) * qty`; refund on price improvement and cancel
- Default `NoopRisk` for pure matching tests
- In-memory `CashLedger` behind the trait + tests ported from `pyclob` reserve cases (insufficient, refund on improvement)
- This is still not token inventory; it is short-sale collateral.

### C15 — `test: property tests for matching invariants`

- `proptest` command streams: random mix of place/cancel/reduce/TIF against 1–4 markets
- Assert invariants 1–9 after every command
- Shrinkable seed printed on failure
- A handful of recorded traces as regression fixtures (`tests/fixtures/*.json`)

### C16 — `feat: journal and replay`

- Append-only `Command` enum + `Journal` trait (`Vec` impl, later file)
- `Engine::replay(journal) -> Engine` bit-identical
- Snapshot of book depth + next ids as a checksum
- Tests: replay equals live; snapshot hash stable.

### C17 — `feat: price-side-time priority policy`

- `Priority::PriceTime` (default) vs `Priority::PriceSideTime`
- Under side-time, at one merged price, orders whose **original action is Sell** rank ahead of Buy-duals (HIP-4)
- Implemented as a secondary key on the level (two deques, or a sort key), not a different matcher
- Tests: same price, sell fills before buy-dual; policy off → pure FIFO.

### C18 — `docs: engine readme, rustdoc, and worked examples`

- README: identity, one matching rule, why adapters not engines, how to place a Kalshi-style vs Polymarket-style order
- Worked launch example: empty book, buy YES 60 + buy NO 40 → mint
- Explicit “not a venue” scope
- `examples/launch_mint.rs`, `examples/views.rs`

### Later (partially done after C18)

- Token inventory + `Complementary` vs `Mint` *execution* — done (`TokenLedger`, `Exchange`)
- Replace/amend that preserves priority on size-down — done (`Engine::reduce` / `replace`)
- Engine journal replay — done (`replay_engine`)
- Benchmarks — done (`benches/book.rs`)
- NegRisk conversion ops — still later
- FFI / Python bindings if a research host needs them — still later

---

## Testing approach

- Unit tests live next to the module they pin.
- Golden traces in C5/C6/C10 are the ones that must not regress; they encode venue semantics.
- Property tests in C15 are the safety net for FIFO, no-cross, complement views, and adapter purity.
- No browser, no HTTP. Closest substitute: deterministic replay + invariant checks.

## Suggested first implementation slice

C0 is the plan. C1–C6 is a working binary matcher with mint/merge/complementary labels. C7–C10 make it an order book someone can quote against, with Kalshi and Polymarket as adapters. C11–C18 make it an engine.

---

## Open questions (defaults in this plan)

1. **Language** — Rust. Python would match `pyclob` faster but is the wrong long-term core.
2. **Token inventory in v1** — engine emits `MatchKind` but settles as Kalshi cash shorts (C14). True Polymarket complementary transfers need balances per outcome token; that can follow C18.
3. **Host clock** — engine never calls `now()`; the host passes `ts` on each command. Needed for GTD and determinism.
