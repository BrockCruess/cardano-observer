//! Dano Finance (Danogo) detection - CLMM swaps and flexible-pool lending.
//!
//! Sources:
//! - https://docs.dano.finance/
//! - https://docs.dano.finance/introduction/litepaper/danogo-flexible-pool-lending
//! - https://docs.dano.finance/developers/integration/lending-apis
//!
//! Two unrelated surfaces share this module because they are one protocol:
//!
//! 1. **CLMM swaps** (`category: "finance"`). The concentrated-liquidity pools
//!    no order UTxO - users spend the pool directly. We detect pool rewrites via
//!    the pool script credential plus a withdraw-zero reward account, then diff
//!    successive pool values keyed by the pool's LP NFT. See [`PoolTracker`].
//!
//! 2. **Flexible-pool lending** (also `category: "finance"`). Every float market
//!    UTxO carrying one Float Market Token; the market id is that token's asset
//!    name and is reused as the asset name of the market's dToken (supply
//!    receipt) and loan validity token. User flows are keyed off mint deltas:
//!    - Supply / Withdraw - dToken mint / burn
//!    - Borrow / Repay - loan NFT mint / burn (minted in pairs: a validity
//!      token held at the loan script and an owner token sent to the borrower)
//!
//!    Amounts come from diffing the market UTxO's underlying balance across
//!    blocks, so the first sighting of a market only seeds the cache.
//!
//! Coverage of the rest of the product line, checked 2026-07-20:
//! - **Leverage trading** needs no separate detector. It is implemented inside
//!   the flexible-pool contract (audit report lists "Flexible Pool Lending
//!   v1.1.0 (Support Leverage Trading)"), so opening or closing a leveraged
//!   position mints/burns the same loan NFT and lands here as borrow / repay.
//! - **Fixed-pool lending** is audited and documented but has no live pools:
//!   all 28 offers from `get-available-loan-offers` are float pools
//!   (`loanDuration: null`), and no fixed pool appears in cached chain data.
//!   Constants can't be derived from an inactive contract, so it is left out
//!   rather than guessed. Revisit if fixed pools launch.
//! - **Oracle aggregator** is infrastructure, not user activity - no events.
//! - **Bond DEX / staking bonds** are legacy: `docs.danogo.io` now redirects to
//!   `docs.dano.finance`, whose litepaper covers only fixed, flexible and
//!   leverage, and no bond token appears in the token registry.
//! - **Liquidations** are not separated from repays: both burn the loan NFT and
//!   telling them apart needs the redeemer, which chain-sync doesn't give us
//!   cheaply. A liquidation currently reports as a repay.

use super::dex::{DexHit, WantedOut};
use super::DappHit;
use crate::parse::{actor_from_tx, attach_actor};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

pub const NAME: &str = "Dano Finance";
/// dApp label for lending events; matches `DAPP_APPS` in `static/dapp/mod.js`.
const DAPP: &str = "Dano Finance";
pub const ORDERBOOK: bool = false;

// ── DEX surface (CLMM) ───────────────────────────────────────────────────

pub const ORDER_ADDRESSES: &[(&str, super::dex::Role)] = &[];
pub const ORDER_SCRIPT_HASHES: &[(&str, super::dex::Role)] = &[];

/// Direct-spend CLMM pools (no batcher order UTxO).
pub const POOL_ADDRESSES: &[&str] = &[
    "addr1x8vtd879xcmme7kmc3rfpqlhq67zj06dn53fvervtjsk0w7dwgsd23ac468cjj8rcnyuc3s72rtupu6j9dw0xpw83exsufvrg4",
    "addr1w8vtd879xcmme7kmc3rfpqlhq67zj06dn53fvervtjsk0wc7a283u",
];

/// CLMM pool validator (payment credential of both pool addresses).
pub const POOL_SCRIPT_HASHES: &[&str] =
    &["d8b69fc53637bcfadbc4469083f706bc293f4d9d2296646c5ca167bb"];

pub const POOL_NFT_POLICIES: &[&str] = &[];
pub const POOL_NFT_UNITS: &[&str] = &[];
pub const POOL_NFT_PREFIXES: &[(&str, &str)] = &[];

/// LP / position NFT policy (= CLMM pool script hash). Each pool UTxO carries
/// exactly one asset under this policy; the pool-value cache is keyed on it.
const LP_POLICY: &str = "d8b69fc53637bcfadbc4469083f706bc293f4d9d2296646c5ca167bb";

/// Withdraw-zero reward account used by every CLMM swap (detects such txs even
/// when we haven't seen the prior pool UTxO yet).
const REWARD_ACCOUNT: &str = "stake178vtd879xcmme7kmc3rfpqlhq67zj06dn53fvervtjsk0wczh2pdw";

pub fn want(_output: &Value, _tx: &Value) -> Option<WantedOut> {
    None
}

// ── Lending surface (flexible / float pools) ─────────────────────────────

/// Float Market Token - one per market, asset name = market id.
const MARKET_POLICY: &str = "814de8a99452972a9fa9fe2c0f59f49697f208005c001ecac1ddfd57";
/// dToken supply receipt; asset name = market id.
const DTOKEN_POLICY: &str = "94dca24a1f1fcc2ff51cd90f32f4fe9e786d861a2dbf7d27598d26e8";
/// Loan validity + owner NFTs, minted/burned together when a loan opens/closes.
const LOAN_NFT_POLICY: &str = "aca8e306eda3eb6c25a838bebac37d929c216aab13c8d463fca5a08d";

/// A flexible-pool market and the asset it lends.
struct Market {
    /// Asset name of the market's Float Market Token / dToken / loan token.
    id: &'static str,
    display: &'static str,
    /// Empty policy = ADA.
    under_policy: &'static str,
    under_name: &'static str,
}

/// Mainnet float markets. Each `id` was resolved by reading the underlying
/// asset held in that market's UTxO and cross-checked against the Float Market
/// Token's CIP-25 name (`"(…) Float <SYMBOL> Market Token"`). Danogo runs more
/// than one market per asset (differing risk parameters).
const MARKETS: &[Market] = &[
    Market {
        id: "f04403181fbd051edd971af67b85f6c6fe1d9d98949a80b9f3803a14",
        display: "ADA",
        under_policy: "",
        under_name: "",
    },
    Market {
        id: "86ce5902603f940d8935b0ab065b82c11b3eaeb455a13bee3fa7b79d",
        display: "ADA",
        under_policy: "",
        under_name: "",
    },
    Market {
        id: "fc7f95ea342aff20a4fed512e4b7805003641c483af3e4a379e8eccb",
        display: "NIGHT",
        under_policy: "0691b2fecca1ac4f53cb6dfb00b7013e561d1f34403b957cbb5af1fa",
        under_name: "4e49474854",
    },
    Market {
        id: "7553c4258a557314d787908085e67fb50f7d6f8f6ff6e0d1c9b9306b",
        display: "USDA",
        under_policy: "fe7c786ab321f41c654ef6c1af7b3250a613c24e4213e0425a7ae456",
        under_name: "55534441",
    },
    Market {
        id: "34ef55c788a6363d5af44cf4570d068fca932b7df5fcdc77eb909f55",
        display: "USDA",
        under_policy: "fe7c786ab321f41c654ef6c1af7b3250a613c24e4213e0425a7ae456",
        under_name: "55534441",
    },
    Market {
        id: "11bbbea6a20e0cb0433fc7723e16427d435c298e1b7304365af6b3e6",
        display: "USDCx",
        under_policy: "1f3aec8bfe7ea4fe14c5f121e2a92e301afe414147860d557cac7e34",
        under_name: "5553444378",
    },
    Market {
        id: "6ac29b9eabd162cd37479f1c7fa61410364c78ac4601aebd2c079e4c",
        display: "USDCx",
        under_policy: "1f3aec8bfe7ea4fe14c5f121e2a92e301afe414147860d557cac7e34",
        under_name: "5553444378",
    },
    Market {
        id: "b96f6167426d625b8975c0437966745c9240e6488517b2b2f44bc7c7",
        display: "wanUSDC",
        under_policy: "25c5de5f5b286073c593edfd77b48abc7a48e5a4f3d4cd9d428ff935",
        under_name: "55534443",
    },
    Market {
        id: "83b8a0a2074061394fddfe9aebee6fab4924dd73ec1326162f453dc9",
        display: "STRIKE",
        under_policy: "f13ac4d66b3ee19a6aa0f2a22298737bd907cc95121662fc971b5275",
        under_name: "535452494b45",
    },
    Market {
        id: "2648750a97772749ed283bfd4ac0cf230e1a587924b5bfdf2b109331",
        display: "USDM",
        under_policy: "c48cbb3d5e57ed56e276bc45f99ab39abe94e6cd7ac39fb402da47ad",
        under_name: "0014df105553444d",
    },
    Market {
        id: "2212068512d00ba78800e493bce47121d492043c864d896ea9f5ad5c",
        display: "USDM",
        under_policy: "c48cbb3d5e57ed56e276bc45f99ab39abe94e6cd7ac39fb402da47ad",
        under_name: "0014df105553444d",
    },
    Market {
        id: "a1bc0130b1eb217e58c2b40f4e6ed7647af5c71157466bd6a0dbb85c",
        display: "INDY",
        under_policy: "533bb94a8850ee3ccbe483106489399112b74c905342cb1792a797a0",
        under_name: "494e4459",
    },
    Market {
        id: "251d5ccb51543f3647b344d4a4c8f2df5bff9d164854e3e7fe4b1711",
        display: "MIN",
        under_policy: "29d222ce763455e3d7a09a665ce554f00ac89d2e99a1a83d267170c6",
        under_name: "4d494e",
    },
];

fn market(id: &str) -> Option<&'static Market> {
    MARKETS.iter().find(|m| m.id == id)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EventType {
    Supply,
    Withdraw,
    Borrow,
    Repay,
}

impl EventType {
    fn verb(self) -> &'static str {
        match self {
            Self::Supply => "Supply",
            Self::Withdraw => "Withdraw",
            Self::Borrow => "Borrow",
            Self::Repay => "Repay",
        }
    }

    /// Wire value read by `renderDappActivityHtml` in `static/dapp/mod.js`
    /// (it labels borrow/repay principal chips off this).
    fn event_type(self) -> &'static str {
        match self {
            Self::Supply => "supply",
            Self::Withdraw => "withdraw",
            Self::Borrow => "borrow",
            Self::Repay => "repay",
        }
    }
}

/// Lending scanner. Holds the last seen underlying balance per market so a
/// supply / withdraw / borrow / repay can report how much actually moved.
pub struct Scanner {
    markets: Mutex<HashMap<String, i128>>,
}

impl Scanner {
    pub fn new() -> Self {
        Self {
            markets: Mutex::new(HashMap::new()),
        }
    }

    /// Replay restored `{ tx, block }` entries so the first live block can
    /// already diff against a known balance (no events emitted).
    pub fn warm_from_tx_entries(&self, entries: &[(String, Value)]) {
        for (_hash, entry) in entries {
            let Some(tx) = entry.get("tx") else { continue };
            for m in MARKETS {
                if let Some(bal) = market_balance(tx, m) {
                    self.markets.lock().unwrap().insert(m.id.to_string(), bal);
                }
            }
        }
    }

    pub fn scan_block(&self, txs: &[(&str, &Value)]) -> Vec<(String, DappHit)> {
        let mut out = Vec::new();
        for (tx_hash, tx) in txs {
            if let Some(hit) = self.scan_tx(tx) {
                out.push(((*tx_hash).to_string(), hit));
            }
        }
        out
    }

    fn scan_tx(&self, tx: &Value) -> Option<DappHit> {
        let mint = tx.get("mint").and_then(Value::as_object)?;
        let loan = policy_total(mint, LOAN_NFT_POLICY);
        let dtoken = policy_total(mint, DTOKEN_POLICY);

        // Loan lifecycle wins over the dToken movement that may accompany it.
        let event = if loan > 0 {
            EventType::Borrow
        } else if loan < 0 {
            EventType::Repay
        } else if dtoken > 0 {
            EventType::Supply
        } else if dtoken < 0 {
            EventType::Withdraw
        } else {
            return None;
        };

        // The minted token's asset name is the market id. Loan mints also carry
        // a unique per-loan name, so take whichever name names a known market.
        let policy = if loan != 0 { LOAN_NFT_POLICY } else { DTOKEN_POLICY };
        let m = mint
            .get(policy)
            .and_then(Value::as_object)?
            .keys()
            .find_map(|name| market(name))?;

        // The market UTxO is rebuilt in the same tx; diff it to size the move.
        let balance = market_balance(tx, m)?;
        let moved = {
            let mut cache = self.markets.lock().unwrap();
            let prev = cache.insert(m.id.to_string(), balance);
            // Soft cap: markets are few, but never let a bad feed grow this.
            if cache.len() > 512 {
                cache.clear();
                cache.insert(m.id.to_string(), balance);
            }
            prev.map(|p| (balance - p).abs())
        };

        let mut data = serde_json::Map::new();
        data.insert("dapp".into(), json!(DAPP));
        data.insert("eventType".into(), json!(event.event_type()));
        data.insert("market".into(), json!(m.display));
        if let Some(qty) = moved.filter(|q| *q > 0) {
            if m.under_policy.is_empty() {
                data.insert("ada".into(), json!(qty as u64));
            } else {
                let refs = [(m.under_policy.to_string(), m.under_name.to_string(), qty)];
                data.insert("assets".into(), crate::parse::asset_list(&[&refs[0]]));
            }
        }
        attach_actor(&mut data, actor_from_tx(tx).as_deref());

        Some(DappHit {
            kind: "dapp_activity",
            title: format!("{} {} - Dano Finance", event.verb(), m.display),
            data: Value::Object(data),
        })
    }
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

/// Net mint quantity across every asset name under `policy`.
fn policy_total(mint: &serde_json::Map<String, Value>, policy: &str) -> i128 {
    mint.get(policy)
        .and_then(Value::as_object)
        .map(|names| {
            names
                .values()
                .filter_map(|q| q.as_i64().map(i128::from))
                .sum()
        })
        .unwrap_or(0)
}

/// Underlying balance held by `m`'s market UTxO as rebuilt by this tx.
fn market_balance(tx: &Value, m: &Market) -> Option<i128> {
    for output in tx.get("outputs")?.as_array()? {
        let Some(value) = output.get("value").and_then(Value::as_object) else {
            continue;
        };
        let Some(names) = value.get(MARKET_POLICY).and_then(Value::as_object) else {
            continue;
        };
        if !names.contains_key(m.id) {
            continue;
        }
        return Some(if m.under_policy.is_empty() {
            i128::from(output_lovelace_from_value(output.get("value")?))
        } else {
            value
                .get(m.under_policy)
                .and_then(Value::as_object)
                .and_then(|n| n.get(m.under_name))
                .and_then(Value::as_i64)
                .map(i128::from)
                .unwrap_or(0)
        });
    }
    None
}

// ── CLMM pool tracking ───────────────────────────────────────────────────

/// Last seen value of each CLMM pool UTxO, keyed by its LP NFT unit.
#[derive(Default)]
pub struct PoolTracker {
    pools: Mutex<HashMap<String, Value>>,
}

impl PoolTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when this tx carries the CLMM withdraw-zero reward account, which
    /// every swap uses - lets us catch a swap before the pool UTxO is known.
    pub fn is_swap_tx(tx: &Value) -> bool {
        tx.get("withdrawals")
            .and_then(Value::as_object)
            .map(|w| w.contains_key(REWARD_ACCOUNT))
            .unwrap_or(false)
    }

    /// Diff a pool rewrite against the last seen value for its LP NFT.
    pub fn detect_swap(&self, tx: &Value, pool_out: &Value) -> Option<DexHit> {
        let value = pool_out.get("value")?;
        let lp_unit = lp_unit(value)?;
        let prev = {
            let mut cache = self.pools.lock().unwrap();
            let prev = cache.get(&lp_unit).cloned();
            cache.insert(lp_unit.clone(), value.clone());
            // Soft cap: drop the whole map rather than O(n) random eviction.
            if cache.len() > 4_000 {
                cache.clear();
                cache.insert(lp_unit.clone(), value.clone());
            }
            prev
        };
        // First sighting of this pool - seed cache only.
        let prev = prev?;
        let (paid_ada, paid_assets, want_ada, want_assets) = pool_value_diff(&prev, value);
        // LP deposit: pool gained both sides / redeem: lost both. Swaps move
        // one side in and the other out.
        let paid_n = paid_assets.len() + usize::from(paid_ada > 0);
        let want_n = want_assets.len() + usize::from(want_ada > 0);
        if paid_n == 0 || want_n == 0 {
            // Likely deposit, redeem, or fee-only touch.
            return None;
        }
        let side = if paid_assets.is_empty() && paid_ada > 0 {
            "buy"
        } else if want_assets.is_empty() && want_ada > 0 {
            "sell"
        } else {
            "swap"
        };
        let refs: Vec<&(String, String, i128)> = paid_assets.iter().collect();
        let mut data = serde_json::Map::new();
        data.insert("dex".into(), json!(NAME));
        data.insert("side".into(), json!(side));
        data.insert("filled".into(), json!(true));
        if paid_ada > 0 {
            data.insert("ada".into(), json!(paid_ada));
        }
        if !refs.is_empty() {
            data.insert("assets".into(), crate::parse::asset_list(&refs));
        }
        let want = if want_ada > 0 {
            Some(WantedOut::ada(want_ada as i128, false))
        } else if let Some((p, n, q)) = want_assets.first() {
            Some(WantedOut::token(*q, false, p.clone(), n.clone()))
        } else {
            None
        };
        super::dex::attach_want(&mut data, &want, true);
        let actor = actor_from_tx(tx)?;
        attach_actor(&mut data, Some(actor.as_str()));
        Some(DexHit {
            kind: "dex_fill",
            title: format!("Swap - {NAME}"),
            data: Value::Object(data),
        })
    }
}

fn lp_unit(value: &Value) -> Option<String> {
    let obj = value.as_object()?;
    let assets = obj.get(LP_POLICY)?.as_object()?;
    let (name, _) = assets.iter().next()?;
    Some(format!("{LP_POLICY}{name}"))
}

/// Assets the pool *gained* (user paid) and *lost* (user received).
fn pool_value_diff(
    prev: &Value,
    next: &Value,
) -> (u64, Vec<(String, String, i128)>, u64, Vec<(String, String, i128)>) {
    let prev_ada = output_lovelace_from_value(prev);
    let next_ada = output_lovelace_from_value(next);
    let mut prev_map: HashMap<(String, String), i128> = HashMap::new();
    let mut next_map: HashMap<(String, String), i128> = HashMap::new();
    let mut prev_assets = Vec::new();
    let mut next_assets = Vec::new();
    crate::parse::collect_assets(Some(prev), &mut prev_assets);
    crate::parse::collect_assets(Some(next), &mut next_assets);
    for (p, n, q) in prev_assets {
        if p == LP_POLICY {
            continue; // ignore LP NFT itself
        }
        prev_map.insert((p, n), q);
    }
    for (p, n, q) in next_assets {
        if p == LP_POLICY {
            continue;
        }
        next_map.insert((p, n), q);
    }
    let mut paid = Vec::new();
    let mut want = Vec::new();
    let mut keys: HashSet<(String, String)> = prev_map.keys().cloned().collect();
    keys.extend(next_map.keys().cloned());
    for k in keys {
        let a = prev_map.get(&k).copied().unwrap_or(0);
        let b = next_map.get(&k).copied().unwrap_or(0);
        if b > a {
            paid.push((k.0, k.1, b - a));
        } else if a > b {
            want.push((k.0, k.1, a - b));
        }
    }
    let paid_ada = next_ada.saturating_sub(prev_ada);
    let want_ada = prev_ada.saturating_sub(next_ada);
    (paid_ada, paid, want_ada, want)
}

fn output_lovelace_from_value(value: &Value) -> u64 {
    value
        .get("ada")
        .and_then(|a| a.get("lovelace"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ADA_MARKET: &str = "f04403181fbd051edd971af67b85f6c6fe1d9d98949a80b9f3803a14";

    /// Market UTxO holding `lovelace`, plus a mint of `qty` under `policy`.
    fn tx(policy: &str, name: &str, qty: i64, lovelace: u64) -> Value {
        json!({
            "mint": { policy: { name: qty } },
            "outputs": [{
                "address": "addr1wx2degj2ru0uctl4rnvs7vh5l608smvxrgkm7lf8txxjd6qs43szs",
                "value": {
                    "ada": { "lovelace": lovelace },
                    MARKET_POLICY: { ADA_MARKET: 1 },
                },
            }],
        })
    }

    #[test]
    fn dtoken_mint_is_supply_sized_by_the_market_balance_delta() {
        let s = Scanner::new();
        // First sighting only seeds the balance cache - no amount to report.
        let seed = tx(DTOKEN_POLICY, ADA_MARKET, 1_000, 100_000_000);
        let hit = s.scan_tx(&seed).expect("hit");
        assert_eq!(hit.data["eventType"], "supply");
        assert!(hit.data.get("ada").is_none(), "first sighting seeds only");

        // Pool grew by 25 ADA -> that is the supplied amount.
        let next = tx(DTOKEN_POLICY, ADA_MARKET, 1_000, 125_000_000);
        let hit = s.scan_tx(&next).expect("hit");
        assert_eq!(hit.data["eventType"], "supply");
        assert_eq!(hit.data["market"], "ADA");
        assert_eq!(hit.data["ada"], 25_000_000);
        assert_eq!(hit.title, "Supply ADA - Dano Finance");
    }

    #[test]
    fn dtoken_burn_is_withdraw() {
        let s = Scanner::new();
        s.scan_tx(&tx(DTOKEN_POLICY, ADA_MARKET, -1, 100_000_000));
        let hit = s
            .scan_tx(&tx(DTOKEN_POLICY, ADA_MARKET, -1, 90_000_000))
            .expect("hit");
        assert_eq!(hit.data["eventType"], "withdraw");
        assert_eq!(hit.data["ada"], 10_000_000);
    }

    /// A loan mint carries the market id *and* a unique per-loan name; the
    /// market must be resolved from whichever name we recognise.
    #[test]
    fn loan_nft_mint_is_borrow_and_outranks_a_same_tx_dtoken_mint() {
        let s = Scanner::new();
        let loan_id = "7f34a3b51f5961e7f149f80daf224824f296a69e72ecb39a50783dad";
        let mut t = tx(LOAN_NFT_POLICY, ADA_MARKET, 1, 100_000_000);
        t["mint"][LOAN_NFT_POLICY][loan_id] = json!(1);
        t["mint"][DTOKEN_POLICY] = json!({ ADA_MARKET: 500 });
        assert_eq!(s.scan_tx(&t).expect("hit").data["eventType"], "borrow");

        let mut t2 = tx(LOAN_NFT_POLICY, ADA_MARKET, -1, 100_000_000);
        t2["mint"][LOAN_NFT_POLICY][loan_id] = json!(-1);
        assert_eq!(s.scan_tx(&t2).expect("hit").data["eventType"], "repay");
    }

    #[test]
    fn ignores_txs_without_danogo_lending_mints() {
        let s = Scanner::new();
        assert!(s.scan_tx(&json!({ "outputs": [] })).is_none());
        assert!(s
            .scan_tx(&tx("deadbeef", ADA_MARKET, 1, 100_000_000))
            .is_none());
    }
}
