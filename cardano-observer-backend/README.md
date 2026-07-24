# cardano-observer-backend

The data API for [cardano-observer](../README.md). It serves the metadata,
account, and transaction lookups the observer uses for event enrichment by
querying a [cardano-db-sync](https://github.com/IntersectMBO/cardano-db-sync)
PostgreSQL database directly.

## Why

cardano-observer enriches its live event feed (pool tickers, DRep names,
governance action titles, delegation from→to, historical tx detail) through an
HTTP API. This crate is that API, run from your own infrastructure: a small Rust
service in front of the cardano-db-sync database you already run to follow the
chain. Nothing external is required, so the app keeps working independently of
any hosted service.

## Requirements

- A synced **cardano-node**
- **cardano-db-sync** writing to PostgreSQL (the standard `cexplorer` database)

The backend is read-only against that database and adds no load beyond the
queries it runs for the observer.

## Running

```bash
cp .env.example .env      # set DBSYNC_URL
cargo run --release -p cardano-observer-backend
```

Configuration (env or `.env`):

| Variable | Default | Purpose |
| --- | --- | --- |
| `DBSYNC_URL` | (required) | PostgreSQL URL of the db-sync database, e.g. `postgres://usernme@localhost:5432/cexplorer` |
| `DBSYNC_MAX_CONNECTIONS` | `8` | Connection pool size |
| `DBSYNC_STATEMENT_TIMEOUT_MS` | `60000` | Server-side per-statement timeout in ms (`0` disables) |
| `BACKEND_BIND` | `0.0.0.0:3300` | Listen address |
| `NETWORK` | `mainnet` | `mainnet` / `preprod` / `preview` |
| `RUST_LOG` | `info` | Log level |

Then point the observer at it (in the observer's `.env`):

```
OBSERVER_BACKEND_URL=http://127.0.0.1:3300
```

When run on the same host as db-sync, the observer's `start.sh` / `start-dev.sh`
will build and launch this backend for you if `DBSYNC_URL` is set in the root
`.env` - see the [main README](../README.md).

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
| `GET /pools/{pool_id}/updates` | Lifecycle actions: `tx_hash`, `cert_index`, `action` (`registered` / `deregistered`) |
| `GET /pools/{pool_id}/registrations` | **Extension** - parameters each registration certificate declared (pledge, cost, margin, VRF, reward account, metadata, owners, relays) |
| `GET /governance/dreps` | DRep list; supports `retired` / `expired` boolean filters |
| `GET /governance/dreps/{drep_id}/metadata` | CIP-119 anchor metadata (CIP-105 or CIP-129 ids) |
| `GET /governance/proposals/{tx_hash}/{cert_index}/metadata` | CIP-108 anchor metadata |
| `GET /accounts/{stake_address}` | Registration state, balances, current pool + DRep |
| `GET /accounts/{stake_address}/delegations` | Delegation history |
| `GET /txs/{hash}` | Transaction summary |
| `GET /txs/{hash}/utxos` | Resolved inputs and outputs |

Amounts are strings (lovelace / asset quantities exceed 2^53), asset units are
`policy_hex + name_hex`, and DRep ids are returned in CIP-129 form. Errors use
the standard envelope `{ "status_code", "error", "message" }`. Off-chain
metadata whose fetch failed carries an `error` envelope `{ code, message }`.

`/pools/{pool_id}/registrations` is the one endpoint here with no counterpart in
the API this surface otherwise mirrors: that API reports *that* a pool
registered but never the parameters a past certificate carried, which is what
diffing a re-registration against the previous one needs. It is additive - the
standard `/pools/{pool_id}/updates` keeps its usual action-log shape.

`/pools/extended` intentionally omits `active_stake` / `live_stake` /
`live_saturation` / `blocks_minted`: each one costs a ledger-wide or chain-wide
aggregation per request, and the observer does not consume them. Everything else
matches the shapes the observer expects field-for-field.

## Recommended db-sync indexes

cardano-db-sync creates indexes for its own needs, but a few extra ones make
this backend's queries fast on mainnet-sized data (most visibly `/pools/extended`,
`/governance/dreps`, `/accounts/{stake}`, and `/txs/{hash}/utxos`). They mirror
the exact filter and join columns the backend uses. All are `CREATE INDEX IF NOT
EXISTS`, safe to run on a live database (consider `CREATE INDEX CONCURRENTLY` on
a busy one), and additive - drop any your db-sync version already provides.

```sql
-- Pools
CREATE INDEX IF NOT EXISTS obs_idx_pool_hash_view ON pool_hash (view);
CREATE INDEX IF NOT EXISTS obs_idx_pool_update_hash_reg ON pool_update (hash_id, registered_tx_id, cert_index);
CREATE INDEX IF NOT EXISTS obs_idx_pool_retire_hash_ann ON pool_retire (hash_id, announced_tx_id, cert_index);
CREATE INDEX IF NOT EXISTS obs_idx_off_chain_pool_data_hash ON off_chain_pool_data (hash);
CREATE INDEX IF NOT EXISTS obs_idx_off_chain_pool_fetch_error_pmr ON off_chain_pool_fetch_error (pmr_id, id);

-- DReps / governance
CREATE INDEX IF NOT EXISTS obs_idx_drep_hash_view ON drep_hash (view);
CREATE INDEX IF NOT EXISTS obs_idx_drep_hash_raw_has_script ON drep_hash (raw, has_script);
CREATE INDEX IF NOT EXISTS obs_idx_drep_registration_hash_deposit ON drep_registration (drep_hash_id, deposit, tx_id, cert_index);
CREATE INDEX IF NOT EXISTS obs_idx_drep_registration_anchor ON drep_registration (drep_hash_id, voting_anchor_id, tx_id, cert_index);
CREATE INDEX IF NOT EXISTS obs_idx_voting_procedure_drep ON voting_procedure (drep_voter, tx_id);
CREATE INDEX IF NOT EXISTS obs_idx_drep_distr_hash_epoch ON drep_distr (hash_id, epoch_no);
CREATE INDEX IF NOT EXISTS obs_idx_off_chain_vote_data_anchor ON off_chain_vote_data (voting_anchor_id, id);
CREATE INDEX IF NOT EXISTS obs_idx_off_chain_vote_fetch_error_anchor ON off_chain_vote_fetch_error (voting_anchor_id, id);
CREATE INDEX IF NOT EXISTS obs_idx_gov_action_proposal_tx_index ON gov_action_proposal (tx_id, index);

-- Accounts
CREATE INDEX IF NOT EXISTS obs_idx_stake_address_view ON stake_address (view);
CREATE INDEX IF NOT EXISTS obs_idx_delegation_addr ON delegation (addr_id, id);
CREATE INDEX IF NOT EXISTS obs_idx_delegation_vote_addr ON delegation_vote (addr_id, id);
CREATE INDEX IF NOT EXISTS obs_idx_stake_registration_addr_tx ON stake_registration (addr_id, tx_id);
CREATE INDEX IF NOT EXISTS obs_idx_stake_deregistration_addr_tx ON stake_deregistration (addr_id, tx_id);
CREATE INDEX IF NOT EXISTS obs_idx_reward_addr_epoch ON reward (addr_id, spendable_epoch) INCLUDE (amount, type);
CREATE INDEX IF NOT EXISTS obs_idx_reward_rest_addr_epoch ON reward_rest (addr_id, spendable_epoch) INCLUDE (amount);
CREATE INDEX IF NOT EXISTS obs_idx_withdrawal_addr ON withdrawal (addr_id);

-- Transactions / UTxOs
CREATE INDEX IF NOT EXISTS obs_idx_tx_in_outref ON tx_in (tx_out_id, tx_out_index) INCLUDE (tx_in_id);
CREATE INDEX IF NOT EXISTS obs_idx_tx_in_txinid ON tx_in (tx_in_id);
CREATE INDEX IF NOT EXISTS obs_idx_reference_tx_in_txinid ON reference_tx_in (tx_in_id);
CREATE INDEX IF NOT EXISTS obs_idx_collateral_tx_in_txinid ON collateral_tx_in (tx_in_id);
CREATE INDEX IF NOT EXISTS obs_idx_collateral_tx_out_tx ON collateral_tx_out (tx_id);
CREATE INDEX IF NOT EXISTS obs_idx_ma_tx_out_txout ON ma_tx_out (tx_out_id);
```

Transaction and pool/DRep lookups by hash use db-sync's native unique indexes on
`tx.hash`, `pool_hash.view`, and `drep_hash.raw`, so no hash-encoding indexes are
needed.

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
