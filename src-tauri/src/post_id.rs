//! Media IDs retain their full integer precision across the JSON/JavaScript boundary.
use serde::{Deserialize, Deserializer, Serialize, Serializer};

const JS_MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

pub fn serialize<S: Serializer>(id: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    if *id <= JS_MAX_SAFE_INTEGER {
        serializer.serialize_u64(*id)
    } else {
        serializer.serialize_str(&id.to_string())
    }
}

pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum WireId { Number(u64), Text(String) }
    match WireId::deserialize(deserializer)? {
        WireId::Number(id) => Ok(id),
        WireId::Text(id) if !id.is_empty() && id.bytes().all(|c| c.is_ascii_digit()) => {
            id.parse().map_err(serde::de::Error::custom)
        }
        WireId::Text(_) => Err(serde::de::Error::custom("invalid media ID")),
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct PostId(#[serde(with = "crate::post_id")] pub u64);

pub mod vec {
    use super::*;
    pub fn serialize<S: Serializer>(ids: &[u64], serializer: S) -> Result<S::Ok, S::Error> {
        ids.iter().copied().map(PostId).collect::<Vec<_>>().serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<u64>, D::Error> {
        Vec::<PostId>::deserialize(deserializer).map(|ids| ids.into_iter().map(|id| id.0).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lossless_ids_accept_legacy_numbers_and_round_trip_as_safe_json() {
        for id in [0, 1000, JS_MAX_SAFE_INTEGER, JS_MAX_SAFE_INTEGER + 1, 9_123_456_789_640_193, i64::MAX as u64] {
            let json = serde_json::to_string(&PostId(id)).unwrap();
            assert_eq!(json.starts_with('"'), id > JS_MAX_SAFE_INTEGER);
            assert_eq!(serde_json::from_str::<PostId>(&json).unwrap().0, id);
            assert_eq!(serde_json::from_str::<PostId>(&id.to_string()).unwrap().0, id);
        }
        for bad in ["-1", "1.5", "\"1e3\"", "\"-1\"", "\"\"", "\"18446744073709551616\""] {
            assert!(serde_json::from_str::<PostId>(bad).is_err(), "{bad}");
        }
    }
}
