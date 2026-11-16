use crate::register::RegisterType;
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigChange {
    pub register: Option<(RegisterType, u16)>,
    pub path: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

const SECTIONS: [(&str, RegisterType); 4] = [
    ("holdings", RegisterType::Holding),
    ("inputs", RegisterType::Input),
    ("coils", RegisterType::Coil),
    ("discretes", RegisterType::Discrete),
];

pub fn config_changes(saved: &Value, current: &Value) -> Vec<ConfigChange> {
    let mut changes = Vec::new();
    let (empty_saved, empty_current) = (Map::new(), Map::new());
    let saved_root = saved.as_object().unwrap_or(&empty_saved);
    let current_root = current.as_object().unwrap_or(&empty_current);
    for key in union(saved_root, current_root) {
        if key != "registers" {
            walk(
                key,
                saved_root.get(key),
                current_root.get(key),
                None,
                &mut changes,
            );
        }
    }
    for (section, kind) in SECTIONS {
        let old = entries(saved, section);
        let new = entries(current, section);
        let addresses: BTreeSet<u16> = old.keys().chain(new.keys()).copied().collect();
        for address in addresses {
            walk(
                "",
                old.get(&address),
                new.get(&address),
                Some((kind, address)),
                &mut changes,
            );
        }
    }
    changes
}

fn union<'a>(a: &'a Map<String, Value>, b: &'a Map<String, Value>) -> BTreeSet<&'a String> {
    a.keys().chain(b.keys()).collect()
}

fn entries(config: &Value, section: &str) -> BTreeMap<u16, Value> {
    config
        .get("registers")
        .and_then(|r| r.get(section))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let mut entry = entry.clone();
            let address = entry.as_object_mut()?.remove("address")?.as_u64()?;
            Some((u16::try_from(address).ok()?, entry))
        })
        .collect()
}

const fn as_map(value: Option<&Value>) -> Option<Option<&Map<String, Value>>> {
    match value {
        None => Some(None),
        Some(Value::Object(m)) => Some(Some(m)),
        Some(_) => None,
    }
}

fn walk(
    path: &str,
    old: Option<&Value>,
    new: Option<&Value>,
    register: Option<(RegisterType, u16)>,
    out: &mut Vec<ConfigChange>,
) {
    if old == new {
        return;
    }
    if let (Some(o), Some(n)) = (as_map(old), as_map(new)) {
        let empty = Map::new();
        let (o, n) = (o.unwrap_or(&empty), n.unwrap_or(&empty));
        for key in union(o, n) {
            let path = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            walk(&path, o.get(key), n.get(key), register, out);
        }
        return;
    }
    out.push(ConfigChange {
        register,
        path: path.to_string(),
        old: old.map(Value::to_string),
        new: new.map(Value::to_string),
    });
}

#[cfg(test)]
mod tests {
    use super::ConfigChange;
    use crate::register::RegisterType;

    fn config_changes(saved: &str, current: &str) -> Vec<ConfigChange> {
        let parse = |s: &str| serde_json::from_str(s).unwrap();
        super::config_changes(&parse(saved), &parse(current))
    }

    fn change(path: &str, old: Option<&str>, new: Option<&str>) -> ConfigChange {
        ConfigChange {
            register: None,
            path: path.to_string(),
            old: old.map(str::to_string),
            new: new.map(str::to_string),
        }
    }

    #[test]
    fn identical_configs_have_no_changes() {
        let config = r#"{"name":"a","batch":{"size":16},"registers":{"holdings":[{"address":1,"label":"x"}]}}"#;
        assert!(config_changes(config, config).is_empty());
    }

    #[test]
    fn nested_settings_are_listed_by_path() {
        let saved = r#"{"name":"a","batch":{"size":16,"anchor":"start"},"keybinds":{"pin":"p"}}"#;
        let current = r#"{"name":"b","batch":{"size":32,"anchor":"start"},"keybinds":{"pin":"P"}}"#;
        assert_eq!(
            config_changes(saved, current),
            vec![
                change("batch.size", Some("16"), Some("32")),
                change("keybinds.pin", Some("\"p\""), Some("\"P\"")),
                change("name", Some("\"a\""), Some("\"b\"")),
            ]
        );
    }

    #[test]
    fn registers_are_matched_by_address_not_position() {
        let saved = r#"{"registers":{"holdings":[{"address":5,"label":"five"}]}}"#;
        let current = r#"{"registers":{"holdings":[{"address":2,"pinned":true},{"address":5,"label":"five"}]}}"#;
        assert_eq!(
            config_changes(saved, current),
            vec![ConfigChange {
                register: Some((RegisterType::Holding, 2)),
                ..change("pinned", None, Some("true"))
            }]
        );
    }

    #[test]
    fn removed_entries_and_custom_rule_fields_are_listed_per_field() {
        let saved = r#"{"registers":{"inputs":[{"address":7,"label":"t","custom":{"repr":"u16","ops":["/10"]}}],"coils":[{"address":1,"pinned":true}]}}"#;
        let current = r#"{"registers":{"inputs":[{"address":7,"label":"t","custom":{"repr":"u16","ops":["/100"]}}]}}"#;
        assert_eq!(
            config_changes(saved, current),
            vec![
                ConfigChange {
                    register: Some((RegisterType::Input, 7)),
                    ..change("custom.ops", Some("[\"/10\"]"), Some("[\"/100\"]"))
                },
                ConfigChange {
                    register: Some((RegisterType::Coil, 1)),
                    ..change("pinned", Some("true"), None)
                },
            ]
        );
    }

    #[test]
    fn added_keys_are_sorted_with_existing_ones() {
        let saved = r#"{"registers":{"holdings":[{"address":1,"pinned":true}]}}"#;
        let current = r#"{"registers":{"holdings":[{"address":1,"label":"x"}]}}"#;
        let paths: Vec<String> = config_changes(saved, current)
            .into_iter()
            .map(|c| c.path)
            .collect();
        assert_eq!(paths, ["label", "pinned"]);
    }

    #[test]
    fn object_replaced_by_a_value_is_one_change() {
        assert_eq!(
            config_changes(r#"{"x":{"a":1}}"#, r#"{"x":5}"#),
            vec![change("x", Some(r#"{"a":1}"#), Some("5"))]
        );
    }

    #[test]
    fn register_entries_without_an_address_are_ignored() {
        let saved = r#"{"registers":{"holdings":[]}}"#;
        let current = r#"{"registers":{"holdings":[{"label":"x"},{"address":70000,"label":"y"}]}}"#;
        assert!(config_changes(saved, current).is_empty());
    }

    #[test]
    fn non_object_configs_compare_as_empty() {
        assert_eq!(
            config_changes("null", r#"{"name":"a"}"#),
            vec![change("name", None, Some("\"a\""))]
        );
    }
}
