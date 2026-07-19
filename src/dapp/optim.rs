//! Optim Finance OADA system detection (mainnet).
//!
//! Sources:
//! - https://optim-finance.gitbook.io/optim-finance
//! - https://optim-finance.gitbook.io/optim-finance/oada/oada-and-soada
//! - https://optim-finance.gitbook.io/optim-finance/oada-ui/oada-smart-contract-api
//!
//! User flows keyed off published token policies (empty asset names for
//! OADA / sOADA; `OPTIMiz` for locks):
//! - Mint OADA — OADA mint without sOADA activity
//! - Stake — sOADA mint (OADA usually burned in the same tx)
//! - Unstake — sOADA burn (OADA usually minted back)
//! - Lock / Unlock — OPTIMiz mint / burn
//!
//! Bonds, auctions, and lending AMOs are left for a later pass. Standalone
//! OADA burns are skipped (no user redeem; typically AMO / DEX exit).

use super::DappHit;
use crate::parse::{
    actor_from_tx, actor_receiving_asset, address_has_script_payment, attach_actor,
};
use serde_json::{json, Value};

const DAPP: &str = "Optim Finance";

/// OADA policy (empty asset name). Docs: oada/oada-and-soada.
const OADA_POLICY: &str = "f6099832f9563e4cf59602b3351c3c5a8a7dda2d44575ef69b82cf8d";
/// sOADA staking receipt (empty asset name).
const SOADA_POLICY: &str = "02a574e2f048e288e2a77f48872bf8ffd61d73f9476ac5a83601610b";
/// OPTIMiz lock receipt (`OPTIMiz` asset name).
const OPTIMIZ_POLICY: &str = "fcad3f8a7e27b9cbde9d49a3de830f65085b35cc5090fa796b0760e4";
const OPTIMIZ_NAME: &str = "4f5054494d697a";

const DECIMALS: u32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EventType {
    MintOada,
    Stake,
    Unstake,
    Lock,
    Unlock,
}

impl EventType {
    fn as_str(self) -> &'static str {
        match self {
            Self::MintOada => "mint_oada",
            Self::Stake => "stake",
            Self::Unstake => "unstake",
            Self::Lock => "lock",
            Self::Unlock => "unlock",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::MintOada => "Mint OADA - Optim",
            Self::Stake => "Stake OADA - Optim",
            Self::Unstake => "Unstake sOADA - Optim",
            Self::Lock => "Lock OADA - Optim",
            Self::Unlock => "Unlock OADA - Optim",
        }
    }
}

pub struct Scanner;

impl Scanner {
    pub fn new() -> Self {
        Self
    }

    pub fn warm_from_tx_entries(&self, _entries: &[(String, Value)]) {}

    pub fn scan_block(&self, txs: &[(&str, &Value)]) -> Vec<(String, DappHit)> {
        let mut out = Vec::new();
        for (tx_hash, tx) in txs {
            for hit in scan_tx(tx) {
                out.push(((*tx_hash).to_string(), hit));
            }
        }
        out
    }
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

fn scan_tx(tx: &Value) -> Vec<DappHit> {
    let Some(mint) = tx.get("mint").and_then(Value::as_object) else {
        return Vec::new();
    };
    let oada = policy_delta(mint, OADA_POLICY, "");
    let soada = policy_delta(mint, SOADA_POLICY, "");
    let optimiz = policy_delta(mint, OPTIMIZ_POLICY, OPTIMIZ_NAME);

    let mut hits = Vec::new();

    // Stake / unstake take priority over bare OADA mint in the same tx.
    if soada > 0 {
        if let Some(hit) = hit_for(EventType::Stake, soada as u64, "sOADA", SOADA_POLICY, "", tx)
        {
            hits.push(hit);
        }
    } else if soada < 0 {
        if let Some(hit) = hit_for(
            EventType::Unstake,
            (-soada) as u64,
            "sOADA",
            SOADA_POLICY,
            "",
            tx,
        ) {
            hits.push(hit);
        }
    } else if oada > 0 {
        if let Some(hit) = hit_for(EventType::MintOada, oada as u64, "OADA", OADA_POLICY, "", tx)
        {
            hits.push(hit);
        }
    }

    if optimiz > 0 {
        if let Some(hit) = hit_for(
            EventType::Lock,
            optimiz as u64,
            "OPTIMiz",
            OPTIMIZ_POLICY,
            OPTIMIZ_NAME,
            tx,
        ) {
            hits.push(hit);
        }
    } else if optimiz < 0 {
        if let Some(hit) = hit_for(
            EventType::Unlock,
            (-optimiz) as u64,
            "OPTIMiz",
            OPTIMIZ_POLICY,
            OPTIMIZ_NAME,
            tx,
        ) {
            hits.push(hit);
        }
    }

    hits
}

fn hit_for(
    et: EventType,
    qty: u64,
    ticker: &str,
    policy: &str,
    name_hex: &str,
    tx: &Value,
) -> Option<DappHit> {
    // Never emit a card without a user — enterprise (addr1v…) counts too.
    let actor = actor_receiving_asset(tx, policy, name_hex, qty).or_else(|| actor_from_tx(tx))?;

    let mut data = serde_json::Map::new();
    data.insert("dapp".into(), json!(DAPP));
    data.insert("eventType".into(), json!(et.as_str()));
    data.insert(
        "assets".into(),
        json!({
            "items": [{
                "unit": format!("{policy}{name_hex}"),
                "policy": policy,
                "nameHex": name_hex,
                "name": ticker,
                "qty": qty.to_string(),
                "ticker": ticker,
                "decimals": DECIMALS,
            }],
            "more": 0,
        }),
    );

    // Mint OADA is 1:1 with ADA — surface lovelace when the user clearly
    // funded a script output of matching size (best-effort).
    if et == EventType::MintOada {
        if let Some(ada) = mint_ada_hint(tx, qty) {
            data.insert("ada".into(), json!(ada));
        }
    }

    attach_actor(&mut data, Some(actor.as_str()));

    Some(DappHit {
        kind: "dapp_activity",
        title: et.title().into(),
        data: Value::Object(data),
    })
}

fn policy_delta(
    mint: &serde_json::Map<String, Value>,
    policy: &str,
    name: &str,
) -> i128 {
    let Some(names) = mint.get(policy).and_then(Value::as_object) else {
        return 0;
    };
    qty_i128(names.get(name).unwrap_or(&Value::Null))
}

fn qty_i128(v: &Value) -> i128 {
    v.as_i64()
        .map(i128::from)
        .or_else(|| v.as_u64().map(i128::from))
        .unwrap_or(0)
}

fn mint_ada_hint(tx: &Value, oada_qty: u64) -> Option<u64> {
    // OADA is 6 decimals; 1 OADA ≈ 1 ADA ⇒ qty lovelace ≈ oada_qty.
    let empty = Vec::new();
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    let mut best = 0u64;
    for o in outputs {
        let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
        if !address_has_script_payment(addr) {
            continue;
        }
        let ada = o
            .get("value")
            .and_then(|v| v.get("ada"))
            .and_then(|a| a.get("lovelace"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        // Prefer script outputs near the minted OADA size (±2 ADA noise).
        let lo = oada_qty.saturating_sub(2_000_000);
        let hi = oada_qty.saturating_add(2_000_000);
        if ada >= lo && ada <= hi {
            best = best.max(ada);
        }
    }
    (best > 0).then_some(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const USER: &str = "addr1q9gqsphqrze8jvgjg84decguy6uwr3se5eqkdqkhp9swz5w482ekxnn442wzke60qe8q242tuyyd4qe40hvyvkkfv0cqeczymg";

    #[test]
    fn detects_mint_oada() {
        let tx = json!({
            "mint": { OADA_POLICY: { "": 5_000_000 } },
            "outputs": [
                {
                    "address": "addr1wx7j4kmg2cs7yf92uat3ed4a3u97kr7axxr4avaz0lhwdsq87ujx7",
                    "value": { "ada": { "lovelace": 5_000_000u64 } }
                },
                {
                    "address": USER,
                    "value": {
                        "ada": { "lovelace": 2_000_000u64 },
                        OADA_POLICY: { "": 5_000_000 }
                    }
                }
            ]
        });
        let hits = scan_tx(&tx);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].data["eventType"], "mint_oada");
        assert_eq!(hits[0].title, "Mint OADA - Optim");
        assert_eq!(hits[0].data["ada"], 5_000_000);
        assert!(hits[0].data.get("stake").or(hits[0].data.get("address")).is_some());
    }

    #[test]
    fn detects_stake_not_mint() {
        let tx = json!({
            "mint": {
                OADA_POLICY: { "": -10_000_000 },
                SOADA_POLICY: { "": 9_500_000 }
            },
            "outputs": [{
                "address": USER,
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    SOADA_POLICY: { "": 9_500_000 }
                }
            }]
        });
        let hits = scan_tx(&tx);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].data["eventType"], "stake");
        assert_eq!(hits[0].title, "Stake OADA - Optim");
        assert!(hits[0].data.get("stake").or(hits[0].data.get("address")).is_some());
    }

    #[test]
    fn detects_unstake() {
        let tx = json!({
            "mint": {
                SOADA_POLICY: { "": -8_000_000 },
                OADA_POLICY: { "": 8_400_000 }
            },
            "outputs": [{
                "address": USER,
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    OADA_POLICY: { "": 8_400_000 }
                }
            }]
        });
        let hits = scan_tx(&tx);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].data["eventType"], "unstake");
    }

    #[test]
    fn detects_lock_optimiz() {
        let tx = json!({
            "mint": { OPTIMIZ_POLICY: { OPTIMIZ_NAME: 1_000_000 } },
            "outputs": [{
                "address": USER,
                "value": {
                    "ada": { "lovelace": 2_000_000u64 },
                    OPTIMIZ_POLICY: { OPTIMIZ_NAME: 1_000_000 }
                }
            }]
        });
        let hits = scan_tx(&tx);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].data["eventType"], "lock");
        assert_eq!(hits[0].title, "Lock OADA - Optim");
    }
}
