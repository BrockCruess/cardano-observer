# cardano-observer-backend

Self-hosted Cardano data API for [cardano-observer](../README.md). It serves
the metadata, account, and transaction lookups the observer uses for event
enrichment by querying a [cardano-db-sync](https://github.com/IntersectMBO/cardano-db-sync)
PostgreSQL database directly - no third-party API required.

## Why

cardano-observer enriches its live event feed (pool tickers, DRep names,
governance action titles, delegation from→to, historical tx detail) through an
external HTTP API. This crate provides that API from your own infrastructure,
so the app keeps working no matter what happens to hosted providers. The
observer can switch between this backend and its legacy provider with a single
env var (`USE_OBSERVER_BACKEND=true` in the observer's `.env`).

## Requirements

- A synced **cardano-node**
- **cardano-db-sync** writing to PostgreSQL (the standard `cexplorer` database)

If you already run a local API server for the observer, you already have both -
this backend points at the same database.

## Running

```bash
cp .env.example .env      # set DBSYNC_URL
cargo run --release -p cardano-observer-backend
```

Configuration (env or `.env`):

| Variable | Default | Purpose |
| --- | --- | --- |
| `DBSYNC_URL` | (required) | PostgreSQL URL of the db-sync database |
| `DBSYNC_MAX_CONNECTIONS` | `8` | Connection pool size |
| `BACKEND_BIND` | `0.0.0.0:3300` | Listen address |
| `NETWORK` | `mainnet` | `mainnet` / `preprod` / `preview` |
| `RUST_LOG` | `info` | Log level |

Then in the observer's `.env`:

```
USE_OBSERVER_BACKEND=true
OBSERVER_BACKEND_URL=http://127.0.0.1:3300
```

## Endpoints

All list endpoints accept `count` (1-100, default 100), `page` (default 1) and
`order` (`asc`/`desc`) query params; sending an `unpaged: true` request header
returns the complete listing in one response.

| Endpoint | Notes |
| --- | --- |
| `GET /` | Service name, version, network |
| `GET /health` | `{ "is_healthy": bool }` - checks database connectivity |
| `GET /health/clock` | Server time in ms |
| `GET /blocks/latest` | Latest block summary (handy for checking db-sync progress) |
| `GET /pools` | Bech32 ids of all registered pools |
| `GET /pools/extended` | Pool list with embedded off-chain metadata |
| `GET /pools/{pool_id}/metadata` | Off-chain metadata for one pool (bech32 or hex id) |
| `GET /governance/dreps` | DRep list; supports `retired` / `expired` boolean filters |
| `GET /governance/dreps/{drep_id}/metadata` | CIP-119 anchor metadata (CIP-105 or CIP-129 ids) |
| `GET /governance/proposals/{tx_hash}/{cert_index}/metadata` | CIP-108 anchor metadata |
| `GET /accounts/{stake_address}` | Registration state, balances, current pool + DRep |
| `GET /accounts/{stake_address}/delegations` | Delegation history |
| `GET /txs/{hash}` | Transaction summary |
| `GET /txs/{hash}/utxos` | Resolved inputs and outputs |

Amounts are strings (lovelace / asset quantities exceed 2^53), asset units are
`policy_hex + name_hex`, and DRep ids are returned in CIP-129 form. Errors use
the standard envelope `{ "status_code", "error", "message" }`.

`/pools/extended` intentionally omits `active_stake` / `live_stake` /
`live_saturation` / `blocks_minted`: each one costs a ledger-wide or
chain-wide aggregation per request (the reference implementations of this
endpoint are known to hang on exactly those), and the observer does not
consume them. Everything else matches the shapes the observer expects
field-for-field, including the `error` envelope on metadata whose off-chain
fetch failed.

## Roadmap

The HTTP layer is one service inside a deliberately small binary; the next
planned service is a chain-sync event stream sourced from the node itself
(the observer's live-follow path), letting a single self-hosted backend cover
both live events and historical enrichment. Modules are laid out so that can
land without reshaping the crate:

- `routes/` - HTTP API (this)
- future `sync/` - node chain-sync follower + websocket event stream

## License

MIT, same as cardano-observer.
