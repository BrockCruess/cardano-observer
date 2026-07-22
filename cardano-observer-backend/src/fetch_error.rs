//! Classification of off-chain metadata fetch failures into the stable error
//! codes exposed by the API, keyed on the failure text recorded by db-sync.

use serde_json::{json, Value};

/// Builds the `{ code, message }` error envelope for a recorded fetch failure.
pub fn envelope(message: &str) -> Value {
    let m = message.trim();
    let lower = m.to_lowercase();
    let code = if lower.contains("hash mismatch") {
        "HASH_MISMATCH"
    } else if lower.contains("size error") {
        "SIZE_EXCEEDED"
    } else if lower.contains("decode error") {
        "DECODE_ERROR"
    } else if lower.contains("http response error") {
        "HTTP_RESPONSE_ERROR"
    } else if lower.contains("connection failure error")
        || lower.contains("url parse error")
        || lower.contains("timeout error")
        || lower.contains("http exception error")
    {
        "CONNECTION_ERROR"
    } else {
        "UNKNOWN_ERROR"
    };
    json!({ "code": code, "message": m })
}

/// Moves a non-null `fetch_error` key inside a metadata object into the
/// `error` envelope; removes the key either way.
pub fn transform_metadata(metadata: &mut Value) {
    let Some(obj) = metadata.as_object_mut() else {
        return;
    };
    match obj.remove("fetch_error") {
        Some(Value::String(msg)) => {
            obj.insert("error".into(), envelope(&msg));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_known_failures() {
        assert_eq!(envelope("Hash mismatch from url ...")["code"], "HASH_MISMATCH");
        assert_eq!(envelope("Timeout error from ...")["code"], "CONNECTION_ERROR");
        assert_eq!(envelope("something odd")["code"], "UNKNOWN_ERROR");
    }

    #[test]
    fn transform_moves_fetch_error() {
        let mut m = json!({ "ticker": null, "fetch_error": "Hash mismatch x" });
        transform_metadata(&mut m);
        assert!(m.get("fetch_error").is_none());
        assert_eq!(m["error"]["code"], "HASH_MISMATCH");

        let mut clean = json!({ "ticker": "T", "fetch_error": null });
        transform_metadata(&mut clean);
        assert!(clean.get("fetch_error").is_none());
        assert!(clean.get("error").is_none());
    }
}
