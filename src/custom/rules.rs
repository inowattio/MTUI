use super::{Continuation, CustomRule};
use crate::register::RegisterCell;
use std::collections::BTreeMap;
use std::ops::Deref;

#[derive(Debug, Default, PartialEq)]
pub struct CustomRules {
    rules: BTreeMap<RegisterCell, CustomRule>,
    owners: BTreeMap<RegisterCell, u16>,
}

impl CustomRules {
    pub fn insert(&mut self, cell: RegisterCell, rule: CustomRule) {
        self.rules.insert(cell, rule);
        self.index();
    }

    pub fn remove(&mut self, cell: &RegisterCell) -> Option<CustomRule> {
        let removed = self.rules.remove(cell);
        self.index();
        removed
    }

    pub fn extend(&mut self, rules: impl IntoIterator<Item = (RegisterCell, CustomRule)>) {
        self.rules.extend(rules);
        self.index();
    }

    pub fn clear(&mut self) {
        self.rules.clear();
        self.owners.clear();
    }

    pub fn part_of(&self, (kind, address): RegisterCell) -> Option<(u16, Continuation)> {
        let owner = *self.owners.get(&(kind, address))?;
        let continuation = self.rules.get(&(kind, owner))?.continuation(address)?;
        Some((owner, continuation))
    }

    fn index(&mut self) {
        self.owners.clear();
        for (&(kind, owner), rule) in &self.rules {
            for address in rule.word_addresses().skip(1) {
                self.owners.entry((kind, address)).or_insert(owner);
            }
        }
    }
}

impl Deref for CustomRules {
    type Target = BTreeMap<RegisterCell, CustomRule>;

    fn deref(&self) -> &Self::Target {
        &self.rules
    }
}

impl From<BTreeMap<RegisterCell, CustomRule>> for CustomRules {
    fn from(rules: BTreeMap<RegisterCell, CustomRule>) -> Self {
        let mut indexed = Self {
            rules,
            owners: BTreeMap::new(),
        };
        indexed.index();
        indexed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::custom::CustomRepr;
    use crate::custom::tests::rule;
    use crate::register::RegisterType;

    type Placed = (RegisterCell, CustomRule);

    fn placed(kind: RegisterType, address: u16, repr: CustomRepr, next: &[u16]) -> Placed {
        let mut r = rule(repr);
        r.address = address;
        r.next = next.to_vec();
        ((kind, address), r)
    }

    #[test]
    fn indexed_rules_find_the_owner_of_a_continuation_word() {
        let rules = CustomRules::from(BTreeMap::from([
            placed(RegisterType::Holding, 100, CustomRepr::F64, &[]),
            placed(RegisterType::Holding, 200, CustomRepr::U32, &[520]),
            placed(RegisterType::Holding, 300, CustomRepr::U16, &[]),
        ]));
        let part = |address| rules.part_of((RegisterType::Holding, address));
        assert_eq!(part(100), None, "the owner is not its own continuation");
        assert_eq!(part(101), Some((100, Continuation::Middle)));
        assert_eq!(part(103), Some((100, Continuation::End)));
        assert_eq!(part(104), None);
        assert_eq!(part(520), Some((200, Continuation::Detached)));
        assert_eq!(part(201), None, "the jump skips the next address");
        assert_eq!(part(301), None, "single register rules own nothing else");
        assert_eq!(rules.part_of((RegisterType::Input, 101)), None);
    }

    #[test]
    fn overlapping_rules_resolve_to_the_lowest_owner() {
        let rules = CustomRules::from(BTreeMap::from([
            placed(RegisterType::Holding, 102, CustomRepr::U32, &[]),
            placed(RegisterType::Holding, 100, CustomRepr::F64, &[]),
        ]));
        assert_eq!(
            rules.part_of((RegisterType::Holding, 103)),
            Some((100, Continuation::End))
        );
        assert_eq!(
            rules.part_of((RegisterType::Holding, 102)),
            Some((100, Continuation::Middle)),
            "a word can be both an owner and part of another rule"
        );
    }

    #[test]
    fn the_index_follows_every_edit() {
        let holding = |address| (RegisterType::Holding, address);
        let mut rules = CustomRules::default();
        assert_eq!(rules.part_of(holding(101)), None);

        let (cell, r) = placed(RegisterType::Holding, 100, CustomRepr::U32, &[]);
        rules.insert(cell, r);
        assert_eq!(rules.part_of(holding(101)), Some((100, Continuation::End)));

        let (cell, r) = placed(RegisterType::Holding, 100, CustomRepr::U16, &[]);
        rules.insert(cell, r);
        assert_eq!(
            rules.part_of(holding(101)),
            None,
            "replaced by a narrower rule"
        );

        rules.extend([
            placed(RegisterType::Holding, 100, CustomRepr::U32, &[]),
            placed(RegisterType::Holding, 200, CustomRepr::U32, &[]),
        ]);
        assert_eq!(rules.part_of(holding(101)), Some((100, Continuation::End)));
        assert_eq!(rules.part_of(holding(201)), Some((200, Continuation::End)));

        assert!(rules.remove(&holding(100)).is_some());
        assert_eq!(rules.part_of(holding(101)), None);
        assert_eq!(rules.part_of(holding(201)), Some((200, Continuation::End)));
        assert!(rules.remove(&holding(100)).is_none());

        rules.clear();
        assert_eq!(rules.part_of(holding(201)), None);
        assert!(rules.is_empty());
    }
}
