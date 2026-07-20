//! MuesliSwap order-book + AMM batch-order detection.
//!
//! Order datums are not decoded, so orders show the deposited side only.

use super::dex::{Role, WantedOut};
use serde_json::Value;

pub const NAME: &str = "MuesliSwap";
pub const ORDERBOOK: bool = false;

pub const ORDER_ADDRESSES: &[(&str, Role)] = &[
    ("addr1w84psng20ejqcj6a4gljemu9re65waefct7cnahlhmtcwnq63kxyq", Role::Swap),
    ("addr1wy2mjh76em44qurn5x73nzqrxua7ataasftql0u2h6g88lc3gtgpz", Role::Swap),
    ("addr1z8c7eyxnxgy80qs5ehrl4yy93tzkyqjnmx0cfsgrxkfge27q47h8tv3jp07j8yneaxj7qc63zyzqhl933xsglcsgtqcqxzc2je", Role::Swap),
    ("addr1z8l28a6jsx4870ulrfygqvqqdnkdjc5sa8f70ys6dvgvjqc3r6dxnzml343sx8jweqn4vn3fz2kj8kgu9czghx0jrsyqxyrhvq", Role::Swap),
    ("addr1zyq0kyrml023kwjk8zr86d5gaxrt5w8lxnah8r6m6s4jp4g3r6dxnzml343sx8jweqn4vn3fz2kj8kgu9czghx0jrsyqqktyhv", Role::Swap),
    ("addr1w9e7m6yn74r7m0f9mf548ldr8j4v6q05gprey2lhch8tj5gsvyte9", Role::Swap),
];

pub const ORDER_SCRIPT_HASHES: &[(&str, Role)] = &[];
pub const POOL_ADDRESSES: &[&str] = &[];
pub const POOL_SCRIPT_HASHES: &[&str] = &[];

pub const POOL_NFT_POLICIES: &[&str] = &[
    "909133088303c49f3a30f1cc8ed553a73857a29779f6c6561cd8093f",
    "7a8041a0693e6605d010d5185b034d55c79eaf7ef878aae3bdcdbf67",
];

pub const POOL_NFT_UNITS: &[&str] = &[];
pub const POOL_NFT_PREFIXES: &[(&str, &str)] = &[];

pub fn want(_output: &Value, _tx: &Value) -> Option<WantedOut> {
    None
}
