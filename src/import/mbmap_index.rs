use serde::{Deserialize, Deserializer};
use sha2::{Digest, Sha256};
use std::fmt::Write;

pub const INDEX_URL: &str =
    "https://raw.githubusercontent.com/nobrega8/mbmap-library/main/index.json";
const MAPS_URL: &str = "https://raw.githubusercontent.com/nobrega8/mbmap-library/main/";
const INDEX_VERSION: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Entry {
    pub path: String,
    pub sha256: String,
    pub size: usize,
    pub vendor: String,
    pub model: String,
    #[serde(default, deserialize_with = "or_default")]
    pub description: String,
    #[serde(default, deserialize_with = "or_default")]
    pub status: String,
    #[serde(default, deserialize_with = "or_default")]
    pub registers: usize,
    #[serde(default, deserialize_with = "or_default")]
    pub models: Vec<String>,
}

fn or_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Option::<T>::deserialize(deserializer).map(Option::unwrap_or_default)
}

impl Entry {
    pub fn url(&self) -> String {
        format!("{MAPS_URL}{}", self.path)
    }

    pub fn title(&self) -> String {
        format!("{} {}", self.vendor, self.model)
    }
}

#[derive(Deserialize)]
struct Index {
    mbmap_index: i64,
    maps: Vec<Entry>,
}

pub fn parse_index(index: &[u8]) -> Result<Vec<Entry>, String> {
    let index: Index = serde_json::from_slice(index)
        .map_err(|error| format!("the library index is invalid: {error}"))?;
    if index.mbmap_index != INDEX_VERSION {
        return Err(format!(
            "library index version {} is not supported",
            index.mbmap_index
        ));
    }
    Ok(index.maps)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut hex, byte| {
            let _ = write!(hex, "{byte:02x}");
            hex
        })
}

pub fn verify_map(entry: &Entry, bytes: Vec<u8>) -> Result<String, String> {
    if bytes.len() != entry.size || !sha256_hex(&bytes).eq_ignore_ascii_case(&entry.sha256) {
        return Err(format!("{} does not match the library index", entry.path));
    }
    String::from_utf8(bytes).map_err(|_| format!("{} is not valid UTF-8", entry.path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(vendor: &str, model: &str) -> Entry {
        Entry {
            path: format!("maps/{vendor}/{model}.mbmap.json").to_lowercase(),
            sha256: String::new(),
            size: 0,
            vendor: vendor.to_string(),
            model: model.to_string(),
            description: String::new(),
            status: String::new(),
            registers: 0,
            models: Vec::new(),
        }
    }

    const INDEX: &str = r#"{"mbmap_index": 1, "vendors": 1, "maps": [
        {"path": "maps/abb/acs880.mbmap.json", "sha256": "ab", "size": 2,
         "vendor": "ABB", "model": "ACS880", "registers": 12, "models": ["ACS880"]},
        {"path": "maps/x/y.mbmap.json", "sha256": "cd", "size": 3,
         "vendor": "X", "model": "Y", "description": null, "models": null}
    ]}"#;

    #[test]
    fn the_index_lists_its_maps() {
        let maps = parse_index(INDEX.as_bytes()).expect("index");
        assert_eq!(maps.len(), 2);
        assert_eq!(maps[0].title(), "ABB ACS880");
        assert_eq!(maps[0].registers, 12);
        assert_eq!(
            maps[0].url(),
            "https://raw.githubusercontent.com/nobrega8/mbmap-library/main/maps/abb/acs880.mbmap.json"
        );
        assert_eq!(maps[1].description, "");
        assert!(maps[1].models.is_empty());
    }

    #[test]
    fn an_unknown_index_version_is_refused() {
        let index = INDEX.replace("\"mbmap_index\": 1", "\"mbmap_index\": 2");
        let error = parse_index(index.as_bytes()).expect_err("version 2");
        assert!(error.contains("version 2"));
    }

    #[test]
    fn something_other_than_an_index_is_refused() {
        assert!(parse_index(b"").is_err());
        assert!(parse_index(b"{\"maps\": []}").is_err());
        assert!(parse_index(b"<html>rate limited</html>").is_err());
    }

    #[test]
    fn a_map_must_match_its_size_and_hash() {
        let body = b"{\"mbmap\": 2}".to_vec();
        let mut map = entry("V", "M");
        map.size = body.len();
        map.sha256 = sha256_hex(&body).to_uppercase();
        assert_eq!(
            verify_map(&map, body.clone()).as_deref(),
            Ok("{\"mbmap\": 2}")
        );

        let mut altered = body.clone();
        altered[1] = b'\'';
        assert!(verify_map(&map, altered).is_err());

        map.size += 1;
        assert!(verify_map(&map, body).is_err());
    }

    #[test]
    fn the_hash_is_lowercase_hex() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }
}
