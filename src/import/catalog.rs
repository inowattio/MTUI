use crate::import::mbmap_index::{Entry, sha256_hex};
use crate::import::{solarman, sunspec};
use serde::Deserialize;

pub const SUNSPEC_LISTING_URL: &str =
    "https://api.github.com/repos/sunspec/models/contents/json?ref=master";
pub const SOLARMAN_LISTING_URL: &str = "https://api.github.com/repos/davidrapan/ha-solarman/contents/custom_components/solarman/inverter_definitions?ref=main";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ItemKind {
    Mbmap(Entry),
    SunSpec(u16),
    Solarman,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub vendor: String,
    pub model: String,
    pub models: Vec<String>,
    pub description: String,
    pub status: String,
    pub registers: Option<usize>,
    pub path: String,
    pub url: String,
    pub kind: ItemKind,
}

impl Item {
    pub fn title(&self) -> String {
        format!("{} {}", self.vendor, self.model)
    }

    pub const fn sunspec_id(&self) -> Option<u16> {
        match self.kind {
            ItemKind::SunSpec(id) => Some(id),
            _ => None,
        }
    }
}

impl From<Entry> for Item {
    fn from(entry: Entry) -> Self {
        Self {
            vendor: entry.vendor.clone(),
            model: entry.model.clone(),
            models: entry.models.clone(),
            description: entry.description.clone(),
            status: entry.status.clone(),
            registers: Some(entry.registers),
            path: entry.path.clone(),
            url: entry.url(),
            kind: ItemKind::Mbmap(entry),
        }
    }
}

#[derive(Deserialize)]
struct ListedFile {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    download_url: Option<String>,
}

pub fn listing(json: &[u8], extensions: &[&str]) -> Result<Vec<(String, String)>, String> {
    let files: Vec<ListedFile> = serde_json::from_slice(json)
        .map_err(|error| format!("the file listing is invalid: {error}"))?;
    Ok(files
        .into_iter()
        .filter(|file| file.kind == "file")
        .filter(|file| {
            extensions
                .iter()
                .any(|extension| file.name.ends_with(extension))
        })
        .filter_map(|file| Some((file.name, file.download_url?)))
        .collect())
}

pub fn sunspec_file_id(name: &str) -> Option<u16> {
    name.strip_prefix("model_")?
        .strip_suffix(".json")?
        .parse()
        .ok()
}

pub fn sunspec_item(name: String, url: String, info: sunspec::ModelInfo) -> Item {
    Item {
        vendor: "SunSpec".to_string(),
        model: info.label,
        models: Vec::new(),
        description: info.description,
        status: format!("model {}", info.id),
        registers: Some(info.registers),
        path: name,
        url,
        kind: ItemKind::SunSpec(info.id),
    }
}

pub fn solarman_item(name: String, url: String) -> Item {
    let (vendor, model) = solarman::title(&name);
    Item {
        vendor,
        model,
        models: Vec::new(),
        description: String::new(),
        status: String::new(),
        registers: None,
        path: name,
        url,
        kind: ItemKind::Solarman,
    }
}

pub fn cache_file_name(url: &str) -> String {
    let base = url
        .split(['?', '#'])
        .next()
        .and_then(|path| path.rsplit('/').next())
        .unwrap_or_default();
    let base: String = base
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        .take(64)
        .collect();
    let hash = sha256_hex(url.as_bytes());
    let hash = hash.get(..16).unwrap_or_default();
    if base.is_empty() || base.chars().all(|c| c == '.') {
        hash.to_string()
    } else {
        format!("{hash}-{base}")
    }
}

pub fn search(items: &[Item], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut ranked: Vec<(u8, usize)> = items
        .iter()
        .enumerate()
        .filter_map(|(i, item)| Some((rank(item, &words)?, i)))
        .collect();
    ranked.sort_unstable();
    ranked.into_iter().map(|(_, i)| i).collect()
}

fn rank(item: &Item, words: &[String]) -> Option<u8> {
    let names: Vec<String> = [&item.vendor, &item.model, &item.status, &item.path]
        .into_iter()
        .chain(&item.models)
        .map(|name| name.to_lowercase())
        .collect();
    let description = item.description.to_lowercase();
    words.iter().try_fold(0, |tier, word| {
        let word_tier = if names.iter().any(|name| name.starts_with(word.as_str())) {
            0
        } else if names.iter().any(|name| name.contains(word.as_str())) {
            1
        } else if description.contains(word.as_str()) {
            2
        } else {
            return None;
        };
        Some(u8::max(tier, word_tier))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(vendor: &str, model: &str, description: &str) -> Item {
        Item {
            vendor: vendor.to_string(),
            model: model.to_string(),
            models: Vec::new(),
            description: description.to_string(),
            status: String::new(),
            registers: None,
            path: String::new(),
            url: String::new(),
            kind: ItemKind::Solarman,
        }
    }

    #[test]
    fn an_empty_query_lists_every_item_in_order() {
        let items = [item("B", "1", ""), item("A", "2", "")];
        assert_eq!(search(&items, ""), [0, 1]);
        assert_eq!(search(&items, "   "), [0, 1]);
    }

    #[test]
    fn every_word_must_match_case_insensitively() {
        let items = [
            item("ABB", "ACS880", "Drive"),
            item("Schneider", "PM5100", "Power meter"),
            item("ABB", "M4M", "Power meter"),
        ];
        assert_eq!(search(&items, "abb"), [0, 2]);
        assert_eq!(search(&items, "ABB meter"), [2]);
        assert_eq!(search(&items, "abb pm5100"), Vec::<usize>::new());
    }

    #[test]
    fn names_starting_with_the_query_come_first() {
        let items = [
            item("A. Eberle", "PQI-DA smart", ""),
            item("SMA", "Sunny Boy", ""),
        ];
        assert_eq!(search(&items, "sma"), [1, 0]);
        assert_eq!(search(&items, "sma sunny"), [1]);
    }

    #[test]
    fn name_matches_come_before_description_matches() {
        let mut aliased = item("Acme", "X1", "");
        aliased.models = vec!["SolarMax".to_string()];
        let items = [
            item("Other", "Z9", "Works with the solarmax gateway"),
            aliased,
        ];
        assert_eq!(search(&items, "solarmax"), [1, 0]);
    }

    #[test]
    fn the_status_and_file_name_are_searchable() {
        let mut model = item("SunSpec", "Inverter (Three Phase)", "");
        model.status = "model 103".to_string();
        let mut definition = item("Deye", "Hybrid", "");
        definition.path = "deye_hybrid.yaml".to_string();
        let items = [model, definition];
        assert_eq!(search(&items, "103"), [0]);
        assert_eq!(search(&items, "deye_hy"), [1]);
    }

    #[test]
    fn listings_keep_files_with_the_wanted_extensions() {
        let json = br#"[
            {"name": "model_1.json", "type": "file", "download_url": "https://raw/model_1.json"},
            {"name": "Makefile", "type": "file", "download_url": "https://raw/Makefile"},
            {"name": "sub", "type": "dir", "download_url": null},
            {"name": "deye.yaml", "type": "file", "download_url": "https://raw/deye.yaml"}
        ]"#;
        assert_eq!(
            listing(json, &[".json"]),
            Ok(vec![(
                "model_1.json".to_string(),
                "https://raw/model_1.json".to_string()
            )])
        );
        assert_eq!(listing(json, &[".yaml", ".yml"]).map(|l| l.len()), Ok(1));
        assert!(listing(b"{\"message\": \"rate limited\"}", &[".json"]).is_err());
    }

    #[test]
    fn cache_files_are_named_by_a_hash_of_the_whole_url_and_its_file_name() {
        let name = cache_file_name("https://raw.example/json/model_103.json");
        assert_eq!(name.len(), 16 + 1 + "model_103.json".len());
        assert!(name.ends_with("-model_103.json"));
        assert_ne!(
            cache_file_name("https://api.example/contents/json?ref=master"),
            cache_file_name("https://api.example/contents/json?ref=main")
        );
        assert!(cache_file_name("https://api.example/contents/json?ref=main").ends_with("-json"));
        assert_eq!(cache_file_name("https://x/../..").len(), 16);
        assert_eq!(
            cache_file_name("https://x/a b/../c%2F..").len(),
            16 + 1 + "c2F..".len()
        );
        assert!(!cache_file_name("https://x/a/..%2F..%2Fetc").contains('/'));
    }

    #[test]
    fn sunspec_file_names_carry_the_model_id() {
        assert_eq!(sunspec_file_id("model_103.json"), Some(103));
        assert_eq!(sunspec_file_id("model_64110.json"), Some(64110));
        assert_eq!(sunspec_file_id("model_x.json"), None);
        assert_eq!(sunspec_file_id("schema.json"), None);
    }

    #[test]
    fn mbmap_entries_become_items_with_their_download_url() {
        let entry = Entry {
            path: "maps/abb/acs880.mbmap.json".to_string(),
            sha256: String::new(),
            size: 0,
            vendor: "ABB".to_string(),
            model: "ACS880".to_string(),
            description: String::new(),
            status: "official".to_string(),
            registers: 12,
            models: Vec::new(),
        };
        let item = Item::from(entry);
        assert_eq!(item.title(), "ABB ACS880");
        assert_eq!(item.registers, Some(12));
        assert!(item.url.ends_with("/maps/abb/acs880.mbmap.json"));
        assert_eq!(item.sunspec_id(), None);
    }
}
