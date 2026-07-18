# cardano-observer

**A real-time chain event monitor for Cardano.** One lightweight Rust binary
follows your node's chain tip through Ogmios and streams *every* on-chain event
to a polished, zero-dependency web UI - transactions, token transfers, mints
and burns, DEX and dApp activity, staking and pool certificates, governance
actions, votes, DReps, reward withdrawals, metadata messages, and even chain
forks, orphaned blocks and slot battles.

**[Try it now!](https://observer.brock.tools/)**

![vertical feed](docs/screenshot-vertical.png)

## Features

- **Everything, live.** Chain-synced from the tip via Ogmios' WebSocket
  protocol, events reach the browser within milliseconds of the block arriving
  at your node.
- **The feed *is* the chain.** Blocks are nodes on a glowing spine; every
  event in a block hangs off it as a colour-coded card. Events nest by
  causality - block → transaction → detail events (mint, transfer, swap,
  cert, metadata, …) - so children indent under their parent tx instead of
  floating as a flat list. Works vertically (mobile & desktop) and
  horizontally (one column per block) - toggle anytime.
- **Light-cone hover.** Hover any tx-scoped card to light its spend-graph
  neighborhood: the hovered transaction, its input ancestry (past), and the
  txs that spend its outputs (future) get a category-coloured inset glow.
  Built from `inputTxs` on transaction events; the client graph covers the
  live stream plus the 24h retention preload and prunes with retention trim.
- **Fork visibility.** Rollbacks are detected from the chain-sync protocol
  itself: orphaned blocks are struck through and ribboned in place, a fork
  card explains the rollback, and when a competing block wins the same slot
  (or height) you get a **slot battle** card naming winner and loser.
- **DEX awareness.** Swap orders, batch settlements, cancellations, and LP
  deposits / redeems on every major Cardano DEX - Minswap (V1+V2), SundaeSwap
  (V1+V3), WingRiders (V1+V2), MuesliSwap, Splash (incl. Spectrum contracts),
  VyFinance, CSWAP, GeniusYield, ChadSwap, and Dano Finance - detected from
  order-script credentials and pool NFTs, with buy / sell / swap / deposit /
  redeem inferred from the deposited assets. No separate DEX indexer needed.
  **CIP-26 filter:** swap / order / fill / cancel cards are only shown when
  every involved native asset is in the Cardano token registry
  (`token-registry.json`). Trades in unregistered tokens (and incomplete
  one-sided asks) are dropped entirely so the feed stays readable. LP
  deposit / redeem events are not filtered this way - LP share tokens are
  rarely registered.
- **dApp awareness.** Known dApp scripts are classified into a dedicated
  category:
  - [Iagon](https://docs.iagon.com/blockchain/on-chain-activity) — stake
    delegation, node registration / pledge / retirement, earnings claims,
    position listings / sales, subscriptions, and stake withdrawals.
  - [Indigo Protocol](https://docs.indigoprotocol.io/) — CDP open / close /
    mint / burn / collateral, liquidations & redemptions, stability-pool
    accounts, INDY staking, ROB orders, stableswap, interest, and governance
    touches (mainnet V3 validator set from
    `config.indigoprotocol.io`).
  - [FluidTokens](https://docs.fluidtokens.com/) — V3 lending pools / requests /
    loans (repay, liquidate, recast, collateral changes), dutch auctions,
    Aquarium sponsored-fee tanks, $FLDT staking, and legacy P2P lending.
- **History survives restarts.** Events and transaction details are persisted
  to append-only JSONL files (full history on disk; never compacted) and
  restored on startup, so the feed never starts empty. Chain-sync then
  resumes from the last persisted block, **backfilling everything that
  happened while the server was down** - including forks: a rollback that
  occurred offline still gets its orphan cards on the next start. Scroll
  further back through the full on-disk history via infinite load-more;
  search covers the in-memory retention window (default 24h) after a
  background preload into the browser.
- **Trending subjects.** A rolling top-10 ticker of asset / pool / CIP-20
  keywords over the retention window; click a term to search.
- **Full transaction modal.** Click any card for the complete transaction:
  inputs/outputs with amounts and asset chips, certificates, withdrawals,
  proposals, votes, metadata, raw JSON, and explorer links (mainnet or
  `preprod.` / `preview.` Cardanoscan, Cexplorer, AdaStat). Served from an
  in-memory cache; falls back to Blockfrost for older transactions.
- **Token, pool & DRep metadata.** Asset names, tickers, and decimals resolve
  from a durable CIP-26 token-registry cache (`DATA_DIR/token-registry.json`),
  re-downloaded daily at 00:00 UTC. Unregistered assets get a local stub (no
  Blockfrost). Pool tickers and DRep names come from Blockfrost scrapes into
  `DATA_DIR/pools.json` and `DATA_DIR/dreps.json` (also refreshed daily), with
  per-miss fetches appended. CIP-108 governance action titles are fetched on
  first sight into `DATA_DIR/gov-actions.json`.
- **ADA Handles.** Truncated addresses on cards and in the tx modal resolve to
  the account's preferred `$handle` when available, via the free public
  [Handles API](https://api.handle.me) (or your own KoraLabs / Cardano
  Foundation Handle resolver). Disable with `ADA_HANDLE_URL=none`.
- **Delegation context.** Stake-pool and DRep delegations always show the
  delegating stake address (or `$handle`), plus **from → to** when the
  previous target is known - via an in-process tracker seeded from persisted
  history, with Blockfrost account lookups for misses.
- **Filters that stick.** Per-category chips (including governance subtypes),
  free-text search (tx / block / address / policy / ticker / DEX name), URL
  deep-links (`?q=minswap`, `?BROCK`, `?filters=minswap&blocks&iagon`, …), a
  minimum-₳ filter, layout and density toggles - all cached in the browser's
  localStorage for your next visit. Search runs over the preloaded retention
  window in the browser (no per-query server scan). Start a preset with
  `?filters=` then list every category / DEX venue / dApp as `&name` flags
  (e.g.   `?filters=minswap&blocks&iagon` → Minswap DEX only, Blocks, Iagon
  dApp only; `?filters=indigo` → Indigo Protocol;
  `?filters=fluidtokens` → FluidTokens). Names match the on-screen chips:
  category multi-word labels work via any word (`forks` / `battles`);
  one-word DEX/dApp names match in full (`vyfinance`, `sundaeswap`,
  `geniusyield`, `fluidtokens`); multi-word names use the first word (`dano`
  for Dano Finance, `indigo` for Indigo Protocol).
- **Reading-friendly.** Scroll down and the feed pauses; a "new events" pill
  counts what you're missing and snaps you back to the tip when clicked.
- **Light on the host.** Builds to a single static binary (~4 MB) with no
  database of its own and no JS build chain - hand-written static assets
  (HTML/CSS/JS plus logos/images) are embedded at compile time. Responses
  are gzip/brotli-compressed; static assets carry ETag + Cache-Control.
  RAM scales with `EVENT_RETENTION_HOURS` (events + matching tx bodies for
  the detail modal): a 24h mainnet window is typically on the order of
  hundreds of MB for the event buffer alone, plus a matching browser-side
  copy for fast search.

| Vertical layout | Horizontal layout |
|---|---|
| ![vertical](docs/screenshot-vertical.png) | ![horizontal](docs/screenshot-horizontal.png) |

## Event types

| Category | Colour | Events |
|---|---|---|
| Blocks | blue | every block, with issuer pool, size, fees, output volume |
| Transactions | teal | every transaction: amounts, fees, in/out counts, contract flag |
| DEX | fuchsia | swap orders (buy/sell/swap), LP deposits/redeems, batch settlements, cancellations across all major DEXes |
| dApp | teal-cyan | known dApp activity (Iagon; Indigo CDP / SP / staking; FluidTokens lending / Aquarium / …) |
| Tokens | gold | native asset transfers, enriched with registry metadata |
| Mint / Burn | orange | token mints and burns, incl. NFT name decoding (CIP-67/68 aware) |
| Staking | green | pool delegations (stake/`$handle` + from→to when known), stake key (de)registrations, reward withdrawals |
| Pools | magenta | pool registrations (pledge / margin / cost) and retirements |
| Governance | violet | governance actions, votes (DRep/SPO/CC), DRep delegations (stake/`$handle` + from→to), DRep lifecycle, committee changes |
| Metadata | cyan | transaction metadata incl. CIP-20 messages, shown verbatim |
| Forks & battles | red | rollbacks, orphaned blocks, slot & height battles |

## Requirements

- [Ogmios](https://ogmios.dev) attached to a `cardano-node` (**required** -
  this is the event source)
- [Blockfrost RYO](https://github.com/blockfrost/blockfrost-backend-ryo)
  (optional but recommended - pool/DRep/gov-action metadata, account lookups,
  historical txs, and the recurring pool/DRep scrapes into `pools.json` /
  `dreps.json`)
- [cardano-db-sync](https://github.com/IntersectMBO/cardano-db-sync) **if you
  run Blockfrost RYO** - RYO is an API over a db-sync database, so enrichment
  and historical tx lookups need that stack behind `BLOCKFROST_URL`. The
  observer itself does not talk to db-sync directly.
- An [ADA Handle](https://handle.me) resolver (optional) - defaults to the free
  public API at `https://api.handle.me`. Self-host with
  [handles-public-api](https://github.com/koralabs/handles-public-api) or
  [cf-adahandle-resolver](https://github.com/cardano-foundation/cf-adahandle-resolver)
  and point `ADA_HANDLE_URL` at it.
- Rust 1.85+ to build (edition 2024)

Without Blockfrost, the live feed still works from Ogmios alone; token
enrichment falls back to the on-disk CIP-26 registry cache, pool/DRep names
stay incomplete until Blockfrost is configured, and older tx modals may be
incomplete. Handle labels still work against the public API unless you set
`ADA_HANDLE_URL=none`.

## Quick start

Build and run locally - there is no pre-built binary to download.

```bash
git clone <this repo> && cd cardano-observer
cp .env.example .env        # point OGMIOS_URL / BLOCKFROST_URL at your services
./start.sh                  # cargo build --release, then run the binary
# → open http://<host>:9070
```

`./start.sh` copies `.env.example` if needed, cleans this package (so embedded
`static/` assets stay fresh), builds a release binary, and execs
`./target/release/cardano-observer`. Equivalent by hand:

```bash
cargo build --release
./target/release/cardano-observer
```

For local UI / Rust work, use `./start-dev.sh` instead - it watches `src/` and
`static/` with cargo-watch and rebuilds on change (the frontend is embedded via
`include_str!`, so a rebuild is required to pick up HTML/CSS/JS edits; refresh
the browser after each rebuild).

### Configuration (`.env`)

| Variable | Default | Purpose |
|---|---|---|
| `OGMIOS_URL` | `ws://127.0.0.1:1337` | Ogmios WebSocket endpoint (unused when `DEMO=true`) |
| `BLOCKFROST_URL` | *(unset / disabled)* | Blockfrost RYO base URL (optional; needs db-sync behind it). Example: `http://127.0.0.1:3000` |
| `BLOCKFROST_PROJECT_ID` | *(empty)* | `project_id` header, if your instance needs one |
| `ADA_HANDLE_URL` | public API for `NETWORK` | Handle resolver base URL. Defaults to `https://api.handle.me` (mainnet) or the matching preprod/preview host. Point at a local instance (e.g. `http://127.0.0.1:9095`). `none` / `off` / `false` disables |
| `ADA_HANDLE_API` | `auto` | `auto` \| `kora` \| `cf` — HTTP API shape (`auto` picks CF for port 9095 / adahandle URLs, else KoraLabs) |
| `TOKEN_REGISTRY_ZIP` | Cardano Foundation GitHub master zip | CIP-26 mappings zip used to build `token-registry.json` on first boot (also re-downloaded daily at 00:00 UTC while running) |
| `TOKEN_REGISTRY_REFRESH` | `false` | `true` / `1` / `yes` to re-download the registry zip on boot |
| `POOL_CACHE_REFRESH` | `false` | `true` / `1` / `yes` to re-scrape Blockfrost `/pools` into `pools.json` on boot (also auto-refreshed daily at 00:00 UTC while running) |
| `DREP_CACHE_REFRESH` | `false` | `true` / `1` / `yes` to re-scrape Blockfrost `/governance/dreps` into `dreps.json` on boot (also auto-refreshed daily at 00:00 UTC while running) |
| `NETWORK` | `mainnet` | `mainnet` \| `preprod` \| `preview` (addresses & explorer links) |
| `BIND` | `0.0.0.0:9070` | web UI listen address |
| `DATA_DIR` | `./data` | persisted event/tx history + registry/pool/drep/gov-action caches (JSONL/JSON); tx bodies are kept forever on disk with a hash index; on startup any event whose tx body is missing is refilled from Ogmios (no event republish); `none` / `off` / `false` disables persistence |
| `BACKFILL_HOURS` | `24` | resume chain-sync from the last persisted block if younger than this; `0` = start at tip |
| `EVENT_RETENTION_HOURS` | `24` | hours of events (and hot tx bodies) kept in memory for trending, search, and fast tip modals; full event + tx history still on disk |
| `TX_CACHE` | `0` | optional soft max for in-memory tx bodies (`0` = keep all txs in the retention window) |
| `DEMO` | `false` | synthetic event stream - try the UI with no node (persistence off) |
| `RUST_LOG` | `info` | log level (`error` \| `warn` \| `info` \| `debug` \| `trace`) |

Slot→time and epoch math are discovered from the node itself
(`queryNetwork/startTime` + era summaries), so testnets and future eras work
without code changes.

### Try it without a node

Set `DEMO=true` in `.env` (or inline) and start as usual:

```bash
DEMO=true ./start.sh
# or, for a quick debug build:
DEMO=true cargo run
```

generates a realistic synthetic feed (blocks, tokens, governance, periodic
forks and slot battles) so you can explore the UI anywhere. Persistence is
disabled in demo mode so synthetic events never pollute a real `DATA_DIR`.

## Architecture

```
cardano-node ── Ogmios (chain-sync WS) ──▶ parse / DEX / dApp ──▶ ring buffer ──▶ WS fan-out ──▶ browser
                                                    │                  │
                                                    │                  └─ JSONL persist (DATA_DIR)
                                                    │
                      ┌── token-registry.json ──────┤
                      │                             │
Blockfrost RYO  ◀── enrichment / pools / dreps / gov┘
   └── cardano-db-sync (required by RYO)

Handle API    ◀── stake → preferred $handle (optional)
```

- `src/main.rs` - process entry: config, boot caches, spawn scrapes / daily
  refresh / chain-sync (or demo), axum server
- `src/config.rs` - `.env` / environment configuration
- `src/model.rs` - shared event / block types
- `src/ogmios.rs` - chain-sync client (find intersection at tip, pipelined
  `nextBlock`, automatic reconnect that resumes from the last seen blocks)
- `src/parse.rs` - one Ogmios block → many typed events; bech32 stake/pool/
  DRep/asset-fingerprint encoding, CIP-14/20/67 handling
- `src/dex.rs` - DEX order / fill / cancel / LP detection from script
  credentials and datums
- `src/dapp/` - dApp detectors (`mod.rs` dispatcher; `iagon.rs`, `indigo.rs`,
  `fluidtokens.rs`)
- `src/state.rs` - time-bounded event buffer, tx cache, orphan & battle
  bookkeeping, broadcast channel
- `src/trending.rs` - rolling subject-keyword frequency over the retention
  window
- `src/persist.rs` - append-only JSONL history (full event + tx history;
  never compacted), restore on boot, hash-indexed tx lookups
- `src/enrich.rs` - CIP-26 stamps, pool/DRep/gov-action/Handle caches,
  Blockfrost account / historical-tx lookups; background + daily scrapes
- `src/handles.rs` - ADA Handle preferred-name lookup (KoraLabs or CF API)
- `src/registry.rs` - CIP-26 token registry zip → durable slim cache (daily
  re-download)
- `src/pools.rs` - Blockfrost pool-metadata scrape → `pools.json` (daily
  refresh)
- `src/dreps.rs` - Blockfrost DRep scrape (unpaged + active filters) +
  registration-anchor fetch → `dreps.json` (daily refresh)
- `src/gov_actions.rs` - first-sight CIP-108 titles via Ogmios/Blockfrost →
  `gov-actions.json`
- `src/deleg.rs` - stake/DRep from→to tracker across live + restored events
- `src/demo.rs` - synthetic event stream when `DEMO=true`
- `src/server.rs` - axum server: embedded UI, gzip/brotli, ETag caching,
  `/ws` stream, `/api/events`, `/api/buffer` (chunked via `?before=&limit=`),
  `/api/search`, `/api/trending`, `/api/tx`, `/api/asset`, `/api/registry`,
  `/api/pool`, `/api/drep`, `/api/dreps`, `/api/handle`, `/api/gov-action`,
  `/api/gov-actions`, `/api/stats`, `/healthz`
- `static/` - the whole frontend (HTML/CSS/JS + logos/images); no frameworks,
  no build step; embedded via `include_str!` / `include_bytes!`

### Notes

- The UI shows input *references* for live transactions (Ogmios doesn't
  resolve them); configure Blockfrost (and therefore cardano-db-sync) to get
  fully resolved inputs for historical lookups.
- Event colours were chosen as a colourblind-checked categorical palette; every
  card also carries an icon and a text label, so colour never stands alone.
- Each new tab gets a short tip snapshot (~25 events); scroll loads older
  pages from the in-memory window, then disk. The browser background-hydrates
  `/api/buffer?before=&limit=` in steady chunks (full `EVENT_RETENTION_HOURS`
  window into RAM) so search and filter changes stay local and fast — DOM only
  mounts a viewport (or every match, when the filtered set is small).

## Acknowledgments

Feed event hierarchy (block → transaction → detail events) and light-cone
hover highlighting by
[Pi Lanningham (@Quantumplation)](https://github.com/Quantumplation).

## License

MIT
