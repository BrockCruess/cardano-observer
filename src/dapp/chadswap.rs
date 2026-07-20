//! ChadSwap order-book detection.
//!
//! Uses the generic offer/ask datum shape; settles peer-to-peer.

use super::dex::{parse_offer_ask_want, Role, WantedOut};
use serde_json::Value;

pub const NAME: &str = "ChadSwap";
pub const ORDERBOOK: bool = true;

pub const ORDER_ADDRESSES: &[(&str, Role)] = &[
    ("addr1w84q0y2wwfj5efd9ch3x492edeh6pdwycvt7g030jfzhagg5ftr54", Role::Swap),
];

pub const ORDER_SCRIPT_HASHES: &[(&str, Role)] = &[];
pub const POOL_ADDRESSES: &[&str] = &[];
pub const POOL_SCRIPT_HASHES: &[&str] = &[];
pub const POOL_NFT_POLICIES: &[&str] = &[];
pub const POOL_NFT_UNITS: &[&str] = &[];
pub const POOL_NFT_PREFIXES: &[(&str, &str)] = &[];

pub fn want(output: &Value, tx: &Value) -> Option<WantedOut> {
    parse_offer_ask_want(output, tx)
}
