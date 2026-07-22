//! Bech32 identifier handling: pool ids, stake addresses, DRep ids
//! (CIP-105 legacy and CIP-129), and governance action ids (CIP-129).

use bech32::{Bech32, Hrp};

/// CIP-129 header byte: DRep key-hash credential.
const DREP_KEY_HEADER: u8 = 0x22;
/// CIP-129 header byte: DRep script-hash credential.
const DREP_SCRIPT_HEADER: u8 = 0x23;

pub const SPECIAL_DREPS: [&str; 2] = ["drep_always_abstain", "drep_always_no_confidence"];

/// Accepts a bech32 `pool1...` id or a 56-char hex pool hash and returns the
/// bech32 form used by the database's `pool_hash.view` column.
pub fn normalize_pool_id(input: &str) -> Option<String> {
    let input = input.trim();
    if let Ok((hrp, data)) = bech32::decode(input) {
        if hrp.as_str() == "pool" && data.len() == 28 {
            return Some(input.to_string());
        }
        return None;
    }
    if input.len() == 56 && input.chars().all(|c| c.is_ascii_hexdigit()) {
        let bytes = hex::decode(input).ok()?;
        let hrp = Hrp::parse("pool").ok()?;
        return bech32::encode::<Bech32>(hrp, &bytes).ok();
    }
    None
}

/// True when `input` is a well-formed stake address for the given bech32 hrp
/// (`stake` on mainnet, `stake_test` on test networks).
pub fn is_valid_stake_address(input: &str, expected_hrp: &str) -> bool {
    match bech32::decode(input.trim()) {
        Ok((hrp, data)) => hrp.as_str() == expected_hrp && data.len() == 29,
        Err(_) => false,
    }
}

/// True when `input` is a 64-char hex transaction hash.
pub fn is_valid_tx_hash(input: &str) -> bool {
    input.len() == 64 && input.chars().all(|c| c.is_ascii_hexdigit())
}

/// A DRep id resolved to the forms needed for database lookups and responses.
pub struct DrepId {
    /// 28-byte credential hash; `None` for the special always-* dreps.
    pub raw: Option<Vec<u8>>,
    /// Legacy (CIP-105) bech32 form, as stored in `drep_hash.view`.
    pub view: String,
    pub has_script: bool,
    /// Whether the caller supplied the CIP-129 form.
    pub is_cip129: bool,
    /// CIP-129 bech32 form (equal to `view` for special dreps).
    pub cip129: String,
    /// CIP-129 hex (header byte + credential hash); `None` for special dreps.
    pub cip129_hex: Option<String>,
}

/// Parses a DRep id in CIP-105 (`drep1...` / `drep_script1...`, 28-byte
/// payload), CIP-129 (`drep1...`, 29-byte payload with header), or special
/// (`drep_always_*`) form.
pub fn parse_drep_id(input: &str) -> Option<DrepId> {
    let input = input.trim();
    if SPECIAL_DREPS.contains(&input) {
        return Some(DrepId {
            raw: None,
            view: input.to_string(),
            has_script: false,
            is_cip129: false,
            cip129: input.to_string(),
            cip129_hex: None,
        });
    }
    let (hrp, data) = bech32::decode(input).ok()?;
    match (hrp.as_str(), data.len()) {
        ("drep" | "drep_script", 28) => {
            let has_script = hrp.as_str() == "drep_script";
            let (cip129, cip129_hex) = cip129_from_raw(&data, has_script)?;
            Some(DrepId {
                raw: Some(data),
                view: input.to_string(),
                has_script,
                is_cip129: false,
                cip129,
                cip129_hex: Some(cip129_hex),
            })
        }
        ("drep", 29) if data[0] == DREP_KEY_HEADER || data[0] == DREP_SCRIPT_HEADER => {
            let has_script = data[0] == DREP_SCRIPT_HEADER;
            let raw = data[1..].to_vec();
            let view_hrp = Hrp::parse(if has_script { "drep_script" } else { "drep" }).ok()?;
            let view = bech32::encode::<Bech32>(view_hrp, &raw).ok()?;
            Some(DrepId {
                raw: Some(raw),
                view,
                has_script,
                is_cip129: true,
                cip129: input.to_string(),
                cip129_hex: Some(hex::encode(&data)),
            })
        }
        _ => None,
    }
}

/// CIP-129 bech32 + hex for a 28-byte DRep credential hash.
fn cip129_from_raw(raw: &[u8], has_script: bool) -> Option<(String, String)> {
    let mut payload = Vec::with_capacity(29);
    payload.push(if has_script {
        DREP_SCRIPT_HEADER
    } else {
        DREP_KEY_HEADER
    });
    payload.extend_from_slice(raw);
    let hrp = Hrp::parse("drep").ok()?;
    let bech = bech32::encode::<Bech32>(hrp, &payload).ok()?;
    Some((bech, hex::encode(&payload)))
}

/// Converts a database `drep_hash.view` value to the CIP-129 form returned by
/// the API. Special dreps pass through unchanged with a null hex.
pub fn drep_view_to_cip129(view: &str, has_script: bool) -> (String, Option<String>) {
    if SPECIAL_DREPS.contains(&view) {
        return (view.to_string(), None);
    }
    match bech32::decode(view) {
        Ok((hrp, data))
            if matches!(hrp.as_str(), "drep" | "drep_script") && data.len() == 28 =>
        {
            match cip129_from_raw(&data, has_script) {
                Some((bech, hex)) => (bech, Some(hex)),
                None => (view.to_string(), None),
            }
        }
        _ => (view.to_string(), None),
    }
}

/// CIP-129 governance action id: bech32 of tx hash bytes plus the
/// minimal-length big-endian certificate index.
pub fn gov_action_id(tx_hash_hex: &str, cert_index: u64) -> Option<String> {
    let mut payload = hex::decode(tx_hash_hex).ok()?;
    if payload.len() != 32 {
        return None;
    }
    let index_bytes = cert_index.to_be_bytes();
    let first = index_bytes
        .iter()
        .position(|b| *b != 0)
        .unwrap_or(index_bytes.len() - 1);
    payload.extend_from_slice(&index_bytes[first..]);
    let hrp = Hrp::parse("gov_action").ok()?;
    bech32::encode::<Bech32>(hrp, &payload).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_id_roundtrip() {
        let raw = [7u8; 28];
        let hrp = Hrp::parse("pool").unwrap();
        let bech = bech32::encode::<Bech32>(hrp, &raw).unwrap();
        assert_eq!(normalize_pool_id(&bech).as_deref(), Some(bech.as_str()));
        assert_eq!(
            normalize_pool_id(&hex::encode(raw)).as_deref(),
            Some(bech.as_str())
        );
        assert_eq!(normalize_pool_id("pool1nonsense"), None);
        assert_eq!(normalize_pool_id("stake1abc"), None);
    }

    #[test]
    fn drep_cip105_to_cip129_and_back() {
        let raw = [42u8; 28];
        let hrp = Hrp::parse("drep").unwrap();
        let legacy = bech32::encode::<Bech32>(hrp, &raw).unwrap();

        let parsed = parse_drep_id(&legacy).unwrap();
        assert!(!parsed.is_cip129);
        assert!(!parsed.has_script);
        assert_eq!(parsed.view, legacy);
        assert_eq!(parsed.raw.as_deref(), Some(&raw[..]));

        let parsed129 = parse_drep_id(&parsed.cip129).unwrap();
        assert!(parsed129.is_cip129);
        assert_eq!(parsed129.view, legacy);
        assert_eq!(parsed129.raw.as_deref(), Some(&raw[..]));
        assert_eq!(
            parsed129.cip129_hex.as_deref(),
            Some(format!("22{}", hex::encode(raw)).as_str())
        );
    }

    #[test]
    fn drep_script_gets_script_header() {
        let raw = [9u8; 28];
        let hrp = Hrp::parse("drep_script").unwrap();
        let legacy = bech32::encode::<Bech32>(hrp, &raw).unwrap();
        let parsed = parse_drep_id(&legacy).unwrap();
        assert!(parsed.has_script);
        assert_eq!(
            parsed.cip129_hex.as_deref(),
            Some(format!("23{}", hex::encode(raw)).as_str())
        );
        let (bech, hex_form) = drep_view_to_cip129(&legacy, true);
        assert_eq!(bech, parsed.cip129);
        assert_eq!(hex_form, parsed.cip129_hex);
    }

    #[test]
    fn special_dreps_pass_through() {
        let parsed = parse_drep_id("drep_always_abstain").unwrap();
        assert!(parsed.raw.is_none());
        assert_eq!(parsed.cip129, "drep_always_abstain");
        let (bech, hex_form) = drep_view_to_cip129("drep_always_no_confidence", false);
        assert_eq!(bech, "drep_always_no_confidence");
        assert!(hex_form.is_none());
    }

    #[test]
    fn gov_action_id_encoding() {
        let tx = "aa".repeat(32);
        let id = gov_action_id(&tx, 0).unwrap();
        assert!(id.starts_with("gov_action1"));
        // Index 0 still contributes one byte.
        let (_, data) = bech32::decode(&id).unwrap();
        assert_eq!(data.len(), 33);
        assert_eq!(data[32], 0);

        let id17 = gov_action_id(&tx, 17).unwrap();
        let (_, data17) = bech32::decode(&id17).unwrap();
        assert_eq!(data17[32], 17);
        assert!(gov_action_id("beef", 0).is_none());
    }

    #[test]
    fn stake_address_validation() {
        let raw = [1u8; 29];
        let hrp = Hrp::parse("stake").unwrap();
        let addr = bech32::encode::<Bech32>(hrp, &raw).unwrap();
        assert!(is_valid_stake_address(&addr, "stake"));
        assert!(!is_valid_stake_address(&addr, "stake_test"));
        assert!(!is_valid_stake_address("stake1short", "stake"));
    }
}
