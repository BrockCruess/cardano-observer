-- Minimal replica of the cardano-db-sync tables (and columns) this API reads,
-- used by the integration tests to verify every query against a scratch
-- PostgreSQL instance.

DROP SCHEMA public CASCADE;
CREATE SCHEMA public;

CREATE TABLE epoch (
  id BIGSERIAL PRIMARY KEY,
  no INTEGER NOT NULL
);

CREATE TABLE epoch_param (
  id BIGSERIAL PRIMARY KEY,
  epoch_no INTEGER NOT NULL,
  drep_activity INTEGER
);

CREATE TABLE slot_leader (
  id BIGSERIAL PRIMARY KEY,
  pool_hash_id BIGINT,
  description TEXT
);

CREATE TABLE block (
  id BIGSERIAL PRIMARY KEY,
  hash BYTEA NOT NULL,
  epoch_no INTEGER,
  slot_no BIGINT,
  epoch_slot_no BIGINT,
  block_no INTEGER,
  previous_id BIGINT,
  slot_leader_id BIGINT NOT NULL,
  size INTEGER NOT NULL,
  time TIMESTAMP NOT NULL,
  tx_count BIGINT NOT NULL,
  vrf_key TEXT,
  op_cert BYTEA,
  op_cert_counter BIGINT
);

CREATE TABLE tx (
  id BIGSERIAL PRIMARY KEY,
  hash BYTEA NOT NULL,
  block_id BIGINT NOT NULL,
  block_index INTEGER NOT NULL,
  out_sum NUMERIC NOT NULL,
  fee NUMERIC NOT NULL,
  deposit BIGINT,
  size INTEGER NOT NULL,
  invalid_before NUMERIC,
  invalid_hereafter NUMERIC,
  valid_contract BOOLEAN NOT NULL DEFAULT TRUE,
  treasury_donation NUMERIC
);

CREATE TABLE datum (
  id BIGSERIAL PRIMARY KEY,
  bytes BYTEA
);

CREATE TABLE script (
  id BIGSERIAL PRIMARY KEY,
  hash BYTEA NOT NULL
);

CREATE TABLE stake_address (
  id BIGSERIAL PRIMARY KEY,
  hash_raw BYTEA NOT NULL,
  view TEXT NOT NULL
);

CREATE TABLE tx_out (
  id BIGSERIAL PRIMARY KEY,
  tx_id BIGINT NOT NULL,
  index SMALLINT NOT NULL,
  address TEXT NOT NULL,
  value NUMERIC NOT NULL,
  stake_address_id BIGINT,
  data_hash BYTEA,
  inline_datum_id BIGINT,
  reference_script_id BIGINT,
  consumed_by_tx_id BIGINT
);

CREATE TABLE tx_in (
  id BIGSERIAL PRIMARY KEY,
  tx_in_id BIGINT NOT NULL,
  tx_out_id BIGINT NOT NULL,
  tx_out_index SMALLINT NOT NULL
);

CREATE TABLE collateral_tx_in (
  id BIGSERIAL PRIMARY KEY,
  tx_in_id BIGINT NOT NULL,
  tx_out_id BIGINT NOT NULL,
  tx_out_index SMALLINT NOT NULL
);

CREATE TABLE reference_tx_in (
  id BIGSERIAL PRIMARY KEY,
  tx_in_id BIGINT NOT NULL,
  tx_out_id BIGINT NOT NULL,
  tx_out_index SMALLINT NOT NULL
);

CREATE TABLE collateral_tx_out (
  id BIGSERIAL PRIMARY KEY,
  tx_id BIGINT NOT NULL,
  index SMALLINT NOT NULL,
  address TEXT NOT NULL,
  value NUMERIC NOT NULL,
  data_hash BYTEA,
  inline_datum_id BIGINT,
  reference_script_id BIGINT
);

CREATE TABLE multi_asset (
  id BIGSERIAL PRIMARY KEY,
  policy BYTEA NOT NULL,
  name BYTEA NOT NULL
);

CREATE TABLE ma_tx_out (
  id BIGSERIAL PRIMARY KEY,
  ident BIGINT NOT NULL,
  quantity NUMERIC NOT NULL,
  tx_out_id BIGINT NOT NULL
);

CREATE TABLE ma_tx_mint (
  id BIGSERIAL PRIMARY KEY,
  ident BIGINT NOT NULL,
  quantity NUMERIC NOT NULL,
  tx_id BIGINT NOT NULL
);

CREATE TABLE withdrawal (
  id BIGSERIAL PRIMARY KEY,
  addr_id BIGINT NOT NULL,
  amount NUMERIC NOT NULL,
  tx_id BIGINT NOT NULL
);

CREATE TABLE treasury (
  id BIGSERIAL PRIMARY KEY,
  addr_id BIGINT NOT NULL,
  amount NUMERIC NOT NULL,
  tx_id BIGINT NOT NULL
);

CREATE TABLE reserve (
  id BIGSERIAL PRIMARY KEY,
  addr_id BIGINT NOT NULL,
  amount NUMERIC NOT NULL,
  tx_id BIGINT NOT NULL
);

CREATE TABLE reward (
  addr_id BIGINT NOT NULL,
  type TEXT NOT NULL,
  amount NUMERIC NOT NULL,
  earned_epoch BIGINT NOT NULL,
  spendable_epoch BIGINT NOT NULL
);

CREATE TABLE reward_rest (
  addr_id BIGINT NOT NULL,
  type TEXT NOT NULL,
  amount NUMERIC NOT NULL,
  earned_epoch BIGINT NOT NULL,
  spendable_epoch BIGINT NOT NULL
);

CREATE TABLE stake_registration (
  id BIGSERIAL PRIMARY KEY,
  addr_id BIGINT NOT NULL,
  cert_index INTEGER NOT NULL,
  tx_id BIGINT NOT NULL
);

CREATE TABLE stake_deregistration (
  id BIGSERIAL PRIMARY KEY,
  addr_id BIGINT NOT NULL,
  cert_index INTEGER NOT NULL,
  tx_id BIGINT NOT NULL
);

CREATE TABLE pool_hash (
  id BIGSERIAL PRIMARY KEY,
  hash_raw BYTEA NOT NULL,
  view TEXT NOT NULL
);

CREATE TABLE pool_metadata_ref (
  id BIGSERIAL PRIMARY KEY,
  pool_id BIGINT NOT NULL,
  url TEXT NOT NULL,
  hash BYTEA NOT NULL,
  registered_tx_id BIGINT NOT NULL
);

CREATE TABLE off_chain_pool_data (
  id BIGSERIAL PRIMARY KEY,
  pool_id BIGINT NOT NULL,
  ticker_name TEXT NOT NULL,
  hash BYTEA NOT NULL,
  json JSONB NOT NULL,
  bytes BYTEA NOT NULL,
  pmr_id BIGINT NOT NULL
);

CREATE TABLE off_chain_pool_fetch_error (
  id BIGSERIAL PRIMARY KEY,
  pool_id BIGINT NOT NULL,
  fetch_time TIMESTAMP,
  pmr_id BIGINT NOT NULL,
  fetch_error TEXT NOT NULL,
  retry_count INTEGER
);

CREATE TABLE pool_update (
  id BIGSERIAL PRIMARY KEY,
  hash_id BIGINT NOT NULL,
  cert_index INTEGER NOT NULL,
  vrf_key_hash BYTEA,
  pledge NUMERIC NOT NULL,
  active_epoch_no BIGINT NOT NULL,
  meta_id BIGINT,
  margin DOUBLE PRECISION NOT NULL,
  fixed_cost NUMERIC NOT NULL,
  reward_addr_id BIGINT,
  registered_tx_id BIGINT NOT NULL
);

CREATE TABLE pool_owner (
  id BIGSERIAL PRIMARY KEY,
  addr_id BIGINT NOT NULL,
  pool_update_id BIGINT NOT NULL
);

CREATE TABLE pool_relay (
  id BIGSERIAL PRIMARY KEY,
  update_id BIGINT NOT NULL,
  ipv4 TEXT,
  ipv6 TEXT,
  dns_name TEXT,
  dns_srv_name TEXT,
  port INTEGER
);

CREATE TABLE pool_retire (
  id BIGSERIAL PRIMARY KEY,
  hash_id BIGINT NOT NULL,
  cert_index INTEGER NOT NULL,
  announced_tx_id BIGINT NOT NULL,
  retiring_epoch INTEGER NOT NULL
);

CREATE TABLE delegation (
  id BIGSERIAL PRIMARY KEY,
  addr_id BIGINT NOT NULL,
  cert_index INTEGER NOT NULL,
  pool_hash_id BIGINT NOT NULL,
  active_epoch_no BIGINT NOT NULL,
  tx_id BIGINT NOT NULL
);

CREATE TABLE drep_hash (
  id BIGSERIAL PRIMARY KEY,
  raw BYTEA,
  view TEXT NOT NULL,
  has_script BOOLEAN NOT NULL
);

CREATE TABLE voting_anchor (
  id BIGSERIAL PRIMARY KEY,
  url TEXT NOT NULL,
  data_hash BYTEA NOT NULL,
  type TEXT
);

CREATE TABLE off_chain_vote_data (
  id BIGSERIAL PRIMARY KEY,
  voting_anchor_id BIGINT NOT NULL,
  hash BYTEA,
  json JSONB NOT NULL,
  bytes BYTEA NOT NULL
);

CREATE TABLE off_chain_vote_fetch_error (
  id BIGSERIAL PRIMARY KEY,
  voting_anchor_id BIGINT NOT NULL,
  fetch_error TEXT NOT NULL,
  fetch_time TIMESTAMP,
  retry_count INTEGER
);

CREATE TABLE drep_registration (
  id BIGSERIAL PRIMARY KEY,
  tx_id BIGINT NOT NULL,
  cert_index INTEGER NOT NULL,
  deposit BIGINT,
  drep_hash_id BIGINT NOT NULL,
  voting_anchor_id BIGINT
);

CREATE TABLE delegation_vote (
  id BIGSERIAL PRIMARY KEY,
  addr_id BIGINT NOT NULL,
  cert_index INTEGER NOT NULL,
  drep_hash_id BIGINT NOT NULL,
  tx_id BIGINT NOT NULL
);

CREATE TABLE voting_procedure (
  id BIGSERIAL PRIMARY KEY,
  tx_id BIGINT NOT NULL,
  index INTEGER NOT NULL,
  drep_voter BIGINT,
  vote TEXT
);

CREATE TABLE drep_distr (
  id BIGSERIAL PRIMARY KEY,
  hash_id BIGINT NOT NULL,
  amount NUMERIC NOT NULL,
  epoch_no INTEGER NOT NULL
);

CREATE TABLE redeemer (
  id BIGSERIAL PRIMARY KEY,
  tx_id BIGINT NOT NULL,
  purpose TEXT,
  index INTEGER
);

CREATE TABLE gov_action_proposal (
  id BIGSERIAL PRIMARY KEY,
  tx_id BIGINT NOT NULL,
  index INTEGER NOT NULL,
  deposit NUMERIC,
  voting_anchor_id BIGINT,
  type TEXT
);
