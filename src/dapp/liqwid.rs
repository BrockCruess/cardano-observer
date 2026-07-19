//! Liqwid Finance lending-market detection (mainnet v2).
//!
//! Sources:
//! - https://liqwid-labs.gitbook.io/liqwid-docs
//! - https://liqwid-labs.gitbook.io/liqwid-docs/api-documentation
//! - https://v2.api.liqwid.finance/graphql (`markets.receiptAsset.currencySymbol`)
//!
//! Supply mints a market's qToken (receipt); withdraw burns it. Markets are
//! identified by those published qToken policies (empty asset name).

use super::DappHit;
use crate::parse::{
    actor_from_tx, address_has_script_payment, attach_actor, stake_from_address,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::OnceLock;

const DAPP: &str = "Liqwid";

#[derive(Clone, Copy, Debug)]
struct Market {
    id: &'static str,
    display: &'static str,
    q_policy: &'static str,
    /// Empty policy = ADA.
    under_policy: &'static str,
    under_name: &'static str,
    decimals: u32,
}

/// Mainnet markets from Liqwid GraphQL `liqwid.data.markets` (2026-07).
/// Re-check https://v2.api.liqwid.finance/graphql when new markets launch.
const MARKETS: &[Market] = &[
    Market {
        id: "AGIX",
        display: "AGIX",
        q_policy: "d753e0d193680fe32710379d3a1ec48087ce94f3831505b922c2894b",
        under_policy: "f43a62fdc3965df486de8a0d32fe800963589c41b38946602a0dc535",
        under_name: "41474958",
        decimals: 8,
    },
    Market {
        id: "Ada",
        display: "ADA",
        q_policy: "a04ce7a52545e5e33c2867e148898d9e667a69602285f6a1298f9d68",
        under_policy: "",
        under_name: "",
        decimals: 6,
    },
    Market {
        id: "BTC",
        display: "wanBTC",
        q_policy: "f72166e9fac8297aeb553c19ffab14f51ae271c2cb26783ba289a3a5",
        under_policy: "25c5de5f5b286073c593edfd77b48abc7a48e5a4f3d4cd9d428ff935",
        under_name: "425443",
        decimals: 8,
    },
    Market {
        id: "COPI",
        display: "COPI",
        q_policy: "dd55119962ca550cdd4219999b9e6d25fc9128f96c7dcb5e485286eb",
        under_policy: "b6a7467ea1deb012808ef4e87b5ff371e85f7142d7b356a40d9b42a0",
        under_name: "436f726e75636f70696173205b76696120436861696e506f72742e696f5d",
        decimals: 6,
    },
    Market {
        id: "DAI",
        display: "wanDAI",
        q_policy: "8996bb07509defe0be6f0c39845a736b266c85a70d87ebfb66454a78",
        under_policy: "25c5de5f5b286073c593edfd77b48abc7a48e5a4f3d4cd9d428ff935",
        under_name: "444149",
        decimals: 6,
    },
    Market {
        id: "DJED",
        display: "DJED",
        q_policy: "6df63e2fdde8b2c3b3396265b0cc824aa4fb999396b1c154280f6b0c",
        under_policy: "8db269c3ec630e06ae29f74bc39edd1f87c819f1056206e879a1cd61",
        under_name: "446a65644d6963726f555344",
        decimals: 6,
    },
    Market {
        id: "ERG",
        display: "rsERG",
        q_policy: "b122b2fc62557df9c3fd0b5c62a4b2c970a0d711560e0a8dd7b264f3",
        under_policy: "04b95368393c821f180deee8229fbd941baaf9bd748ebcdbf7adbb14",
        under_name: "7273455247",
        decimals: 9,
    },
    Market {
        id: "ETH",
        display: "wanETH",
        q_policy: "5f42994532b04f9f5bd4141c69364c5b7d33c85036146ee321799702",
        under_policy: "25c5de5f5b286073c593edfd77b48abc7a48e5a4f3d4cd9d428ff935",
        under_name: "455448",
        decimals: 8,
    },
    Market {
        id: "EURC",
        display: "wanEURC",
        q_policy: "85fa65407b5321fa0e2ef9a3ec98e12a00c35871d7a620be3132003c",
        under_policy: "25c5de5f5b286073c593edfd77b48abc7a48e5a4f3d4cd9d428ff935",
        under_name: "45555243",
        decimals: 6,
    },
    Market {
        id: "IAG",
        display: "IAG",
        q_policy: "f60b7232837203d335cd77494d25c1cc0b218b9a8f3459730c521d13",
        under_policy: "5d16cc1a177b5d9ba9cfa9793b07e60f1fb70fea1f8aef064415d114",
        under_name: "494147",
        decimals: 6,
    },
    Market {
        id: "IUSD",
        display: "iUSD",
        q_policy: "d15c36d6dec655677acb3318294f116ce01d8d9def3cc54cdd78909b",
        under_policy: "f66d78b4a3cb3d37afa0ec36461e51ecbde00f26c8f0a68f94b69880",
        under_name: "69555344",
        decimals: 6,
    },
    Market {
        id: "LQ",
        display: "LQ",
        q_policy: "3883e3e6a24e092d4c14e757fa8ef5c887853060def087d6cf5603f5",
        under_policy: "da8c30857834c6ae7203935b89278c532b3995245295456f993e1d24",
        under_name: "4c51",
        decimals: 6,
    },
    Market {
        id: "MIN",
        display: "MIN",
        q_policy: "a4430a085f45bca6399bec6bd7514eb8c2fce1ed75c7554739cfc32b",
        under_policy: "29d222ce763455e3d7a09a665ce554f00ac89d2e99a1a83d267170c6",
        under_name: "4d494e",
        decimals: 6,
    },
    Market {
        id: "NIGHT",
        display: "NIGHT",
        q_policy: "c45fa8aefc662c003a32be67f6a4652d8ce56bd9e54d7696efd40c86",
        under_policy: "0691b2fecca1ac4f53cb6dfb00b7013e561d1f34403b957cbb5af1fa",
        under_name: "4e49474854",
        decimals: 6,
    },
    Market {
        id: "POL",
        display: "POL",
        q_policy: "6f7d8e31d9256ec27f35d25659dd053cfec098032a5669b2b56798d0",
        // API currently lists LQ as the underlying unit for this market id.
        under_policy: "da8c30857834c6ae7203935b89278c532b3995245295456f993e1d24",
        under_name: "4c51",
        decimals: 6,
    },
    Market {
        id: "PYUSD",
        display: "wanPYUSD",
        q_policy: "b8a327951d579d3537ea175078256bdf9f9899b5387b099d0b58f066",
        under_policy: "25c5de5f5b286073c593edfd77b48abc7a48e5a4f3d4cd9d428ff935",
        under_name: "5059555344",
        decimals: 6,
    },
    Market {
        id: "SHEN",
        display: "SHEN",
        q_policy: "e1ff3557106fe13042ba0f772af6a2e43903ccfaaf03295048882c93",
        under_policy: "8db269c3ec630e06ae29f74bc39edd1f87c819f1056206e879a1cd61",
        under_name: "5368656e4d6963726f555344",
        decimals: 6,
    },
    Market {
        id: "SNEK",
        display: "SNEK",
        q_policy: "4e8c49d610335d139ad7711e0f50315006e29b5221da531e365b4ef8",
        under_policy: "279c909f348e533da5808898f87f9a14bb2c3dfbbacccd631d927a3f",
        under_name: "534e454b",
        decimals: 0,
    },
    Market {
        id: "USDA",
        display: "USDA",
        q_policy: "aa280c98c5b07fdfc8d7a93fb5ba84510b421388e4a18e16efa8eb5f",
        under_policy: "fe7c786ab321f41c654ef6c1af7b3250a613c24e4213e0425a7ae456",
        under_name: "55534441",
        decimals: 6,
    },
    Market {
        id: "USDC",
        display: "wanUSDC",
        q_policy: "aebcb6eaba17dea962008a9d693e39a3160b02b5b89b1c83e537c599",
        under_policy: "25c5de5f5b286073c593edfd77b48abc7a48e5a4f3d4cd9d428ff935",
        under_name: "55534443",
        decimals: 8,
    },
    Market {
        id: "USDCx",
        display: "USDCx",
        q_policy: "d3ff4ac09b0978b1ef7f830fd04d79c4246b53b8bcb08108f4ac5d98",
        under_policy: "1f3aec8bfe7ea4fe14c5f121e2a92e301afe414147860d557cac7e34",
        under_name: "5553444378",
        decimals: 6,
    },
    Market {
        id: "USDCx-USDM-SUNDAE-LP-STABLESWAP",
        display: "LPS-USDCx-USDM",
        q_policy: "2dbe1daa1522e5640331909fbe7458e082fe22cbc047e3c7575fcc8b",
        under_policy: "4de79a0c17180030bff4c36825cb6e99caa007bc632f789561a26d56",
        under_name: "0014df10d7c7a7db47ab71ef07f0aa65e6b0bcf9409977c183e85fe6f0a5feb6",
        decimals: 0,
    },
    Market {
        id: "USDM",
        display: "USDM",
        q_policy: "9e00df0615de0a7b121a7f961d43e23165b8e81b64786c6eb708d370",
        under_policy: "c48cbb3d5e57ed56e276bc45f99ab39abe94e6cd7ac39fb402da47ad",
        under_name: "0014df105553444d",
        decimals: 6,
    },
    Market {
        id: "USDM-USDA-MINSWAP-LP-STABLE",
        display: "LPM-USDM-USDA",
        q_policy: "fcd2d1b8a86cd6dda70553f17e67ba36f8ab0090b5ffbbfa8b2bb8d1",
        under_policy: "5f0d38b3eb8fea72cd3cbdaa9594a74d0db79b5a27e85be5e9015bd6",
        under_name: "5553444d2d555344412d534c50",
        decimals: 0,
    },
    Market {
        id: "USDT",
        display: "wanUSDT",
        q_policy: "7a4d45e6b4e6835c4cea3968f291fab3704949cfd2f2dc1997c4eeec",
        under_policy: "25c5de5f5b286073c593edfd77b48abc7a48e5a4f3d4cd9d428ff935",
        under_name: "55534454",
        decimals: 8,
    },
    Market {
        id: "WMT",
        display: "WMT",
        q_policy: "f2636c8280e49e7ed7a7b1151341130989631b45a08d1b320f016981",
        under_policy: "1d7f33bd23d85e1a25d87d86fac4f199c3197a2f7afeb662a0f34e1e",
        under_name: "776f726c646d6f62696c65746f6b656e",
        decimals: 6,
    },
];

fn market_by_q_policy() -> &'static HashMap<&'static str, &'static Market> {
    static MAP: OnceLock<HashMap<&'static str, &'static Market>> = OnceLock::new();
    MAP.get_or_init(|| MARKETS.iter().map(|m| (m.q_policy, m)).collect())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EventType {
    Supply,
    Withdraw,
}

impl EventType {
    fn as_str(self) -> &'static str {
        match self {
            Self::Supply => "supply",
            Self::Withdraw => "withdraw",
        }
    }

    fn title(self, market: &Market) -> String {
        match self {
            Self::Supply => format!("Supply {} - Liqwid", market.display),
            Self::Withdraw => format!("Withdraw {} - Liqwid", market.display),
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
    let map = market_by_q_policy();
    let mut hits = Vec::new();
    for (policy, names) in mint {
        let Some(m) = map.get(policy.as_str()).copied() else {
            continue;
        };
        let Some(names) = names.as_object() else {
            continue;
        };
        // qTokens mint under the empty asset name.
        let mut delta: i128 = 0;
        for (name, qty) in names {
            if !name.is_empty() {
                continue;
            }
            delta += qty_i128(qty);
        }
        if delta == 0 {
            continue;
        }
        let et = if delta > 0 {
            EventType::Supply
        } else {
            EventType::Withdraw
        };
        let qty = delta.unsigned_abs() as u64;
        if let Some(hit) = hit_for(et, m, qty, tx) {
            hits.push(hit);
        }
    }
    hits
}

fn hit_for(et: EventType, market: &Market, q_qty: u64, tx: &Value) -> Option<DappHit> {
    // Never emit without a user (qToken may sit on a script; fall back to change).
    let actor = actor_for(et, market, tx).or_else(|| actor_from_tx(tx))?;

    let mut data = serde_json::Map::new();
    data.insert("dapp".into(), json!(DAPP));
    data.insert("eventType".into(), json!(et.as_str()));
    data.insert("market".into(), json!(market.display));
    data.insert("marketId".into(), json!(market.id));
    data.insert("qToken".into(), json!(q_qty));
    data.insert("decimals".into(), json!(market.decimals));
    data.insert("qTicker".into(), json!(format!("q{}", market.display)));

    // Surface the underlying movement when we can see it on user outputs
    // (withdraw) or as ADA on the supply path.
    if market.under_policy.is_empty() {
        if let Some(ada) = ada_hint(et, tx) {
            data.insert("ada".into(), json!(ada));
        }
    } else if let Some(under) = under_hint(et, market, tx) {
        data.insert(
            "assets".into(),
            json!({
                "items": [{
                    "unit": format!("{}{}", market.under_policy, market.under_name),
                    "policy": market.under_policy,
                    "nameHex": market.under_name,
                    "name": market.display,
                    "qty": under.to_string(),
                }],
                "more": 0,
            }),
        );
    }

    attach_actor(&mut data, Some(actor.as_str()));

    Some(DappHit {
        kind: "dapp_activity",
        title: et.title(market),
        data: Value::Object(data),
    })
}

fn qty_i128(v: &Value) -> i128 {
    v.as_i64()
        .map(i128::from)
        .or_else(|| v.as_u64().map(i128::from))
        .unwrap_or(0)
}

/// Prefer the wallet that receives minted qTokens (supply) or receives
/// underlying after a burn (withdraw).
fn actor_for(et: EventType, market: &Market, tx: &Value) -> Option<String> {
    let empty = Vec::new();
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    match et {
        EventType::Supply => {
            for o in outputs {
                let addr = o.get("address").and_then(Value::as_str)?;
                if address_has_script_payment(addr) {
                    continue;
                }
                if output_has_asset(o, market.q_policy, "") {
                    return stake_from_address(addr).or_else(|| Some(addr.to_string()));
                }
            }
            None
        }
        EventType::Withdraw => {
            for o in outputs {
                let addr = o.get("address").and_then(Value::as_str)?;
                if address_has_script_payment(addr) {
                    continue;
                }
                if market.under_policy.is_empty() {
                    // ADA withdraw — any non-script output with meaningful ADA.
                    let ada = output_ada(o);
                    if ada >= 1_000_000 {
                        return stake_from_address(addr).or_else(|| Some(addr.to_string()));
                    }
                } else if output_has_asset(o, market.under_policy, market.under_name) {
                    return stake_from_address(addr).or_else(|| Some(addr.to_string()));
                }
            }
            None
        }
    }
}

fn output_has_asset(o: &Value, policy: &str, name: &str) -> bool {
    let Some(val) = o.get("value").and_then(Value::as_object) else {
        return false;
    };
    let Some(names) = val.get(policy).and_then(Value::as_object) else {
        return false;
    };
    qty_i128(names.get(name).unwrap_or(&Value::Null)) > 0
}

fn output_ada(o: &Value) -> u64 {
    o.get("value")
        .and_then(|v| v.get("ada"))
        .and_then(|a| a.get("lovelace"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// Best-effort ADA amount for the ADA market (user-facing output on withdraw,
/// or qToken mint size as a stand-in on supply when exchange rate ≈ 1).
fn ada_hint(et: EventType, tx: &Value) -> Option<u64> {
    let empty = Vec::new();
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    match et {
        EventType::Withdraw => {
            let mut best = 0u64;
            for o in outputs {
                let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
                if address_has_script_payment(addr) {
                    continue;
                }
                best = best.max(output_ada(o));
            }
            (best >= 1_000_000).then_some(best)
        }
        EventType::Supply => None,
    }
}

fn under_hint(et: EventType, market: &Market, tx: &Value) -> Option<u64> {
    let empty = Vec::new();
    let outputs = tx.get("outputs").and_then(Value::as_array).unwrap_or(&empty);
    match et {
        EventType::Withdraw => {
            let mut best = 0u64;
            for o in outputs {
                let addr = o.get("address").and_then(Value::as_str).unwrap_or("");
                if address_has_script_payment(addr) {
                    continue;
                }
                let Some(val) = o.get("value").and_then(Value::as_object) else {
                    continue;
                };
                let Some(names) = val.get(market.under_policy).and_then(Value::as_object) else {
                    continue;
                };
                let q = qty_i128(names.get(market.under_name).unwrap_or(&Value::Null));
                if q > 0 {
                    best = best.max(q as u64);
                }
            }
            (best > 0).then_some(best)
        }
        EventType::Supply => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Real mainnet script (enterprise) + key-payment addresses.
    const SCRIPT: &str = "addr1wx7j4kmg2cs7yf92uat3ed4a3u97kr7axxr4avaz0lhwdsq87ujx7";
    const USER: &str = "addr1q9gqsphqrze8jvgjg84decguy6uwr3se5eqkdqkhp9swz5w482ekxnn442wzke60qe8q242tuyyd4qe40hvyvkkfv0cqeczymg";

    #[test]
    fn detects_qada_supply() {
        let q = "a04ce7a52545e5e33c2867e148898d9e667a69602285f6a1298f9d68";
        let tx = json!({
            "mint": { q: { "": 1_000_000 } },
            "outputs": [
                {
                    "address": SCRIPT,
                    "value": { "ada": { "lovelace": 50_000_000_000u64 } }
                },
                {
                    "address": USER,
                    "value": {
                        "ada": { "lovelace": 2_000_000u64 },
                        q: { "": 1_000_000 }
                    }
                }
            ]
        });
        let hits = scan_tx(&tx);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].data["eventType"], "supply");
        assert_eq!(hits[0].data["market"], "ADA");
        assert_eq!(hits[0].data["qToken"], 1_000_000);
        assert!(hits[0].title.contains("Supply ADA"));
    }

    #[test]
    fn detects_qada_withdraw() {
        let q = "a04ce7a52545e5e33c2867e148898d9e667a69602285f6a1298f9d68";
        let tx = json!({
            "mint": { q: { "": -500_000 } },
            "outputs": [
                {
                    "address": SCRIPT,
                    "value": { "ada": { "lovelace": 49_000_000_000u64 } }
                },
                {
                    "address": USER,
                    "value": { "ada": { "lovelace": 520_000_000u64 } }
                }
            ]
        });
        let hits = scan_tx(&tx);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].data["eventType"], "withdraw");
        assert_eq!(hits[0].data["qToken"], 500_000);
        assert_eq!(hits[0].data["ada"], 520_000_000);
    }
}
