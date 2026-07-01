use super::{build_custom_rule, App};
use crate::custom::{parse_enum, parse_op, CustomRepr};
use crate::modbus::WordOrder;
use crate::num_ops::{cycle, wrap_index};
use crate::register::RegisterCell;
use crate::state::{CustomField, CustomParams, Popup};

impl App {
    pub fn open_custom(&mut self) {
        let (register_type, address) = self.cursor_cell();

        let params = match self.custom_rules.get(&(register_type, address)) {
            Some(rule) => CustomParams {
                address,
                register_type,
                repr: rule.repr,
                ops: rule.ops.clone(),
                enum_map: rule.enum_map.clone(),
                decimals: rule.decimals.map(|d| d.to_string()).unwrap_or_default(),
                prefix: rule.prefix.clone(),
                suffix: rule.suffix.clone(),
                op_buffer: String::new(),
                enum_buffer: String::new(),
                selected: 0,
                existed: true,
                error: None,
            },
            None => CustomParams {
                address,
                register_type,
                repr: CustomRepr::default(),
                ops: Vec::new(),
                enum_map: Vec::new(),
                decimals: String::new(),
                prefix: String::new(),
                suffix: String::new(),
                op_buffer: String::new(),
                enum_buffer: String::new(),
                selected: 0,
                existed: false,
                error: None,
            },
        };
        self.read_mut().popup = Some(Popup::Custom(params));
    }

    fn with_custom(&mut self, f: impl FnOnce(&mut CustomParams)) {
        if let Some(Popup::Custom(c)) = &mut self.read_mut().popup {
            f(c);
        }
    }

    pub fn custom_move(&mut self, down: bool) {
        let n = CustomField::ALL.len() as u16;
        self.with_custom(|c| {
            c.error = None;
            c.selected = wrap_index(c.selected, n, down);
        });
    }

    pub fn custom_cycle(&mut self, field: CustomField, forward: bool) {
        self.with_custom(|c| {
            c.error = None;
            if field == CustomField::Repr {
                c.repr = cycle(&CustomRepr::ALL, c.repr, forward);
            }
        });
    }

    pub fn custom_char(&mut self, field: CustomField, ch: char) {
        self.with_custom(|c| {
            c.error = None;
            match field {
                CustomField::Ops => c.op_buffer.push(ch),
                CustomField::Enum => c.enum_buffer.push(ch),
                CustomField::Decimals => {
                    if ch.is_ascii_digit() && c.decimals.len() < 2 {
                        c.decimals.push(ch);
                    }
                }
                CustomField::Prefix => c.prefix.push(ch),
                CustomField::Suffix => c.suffix.push(ch),
                _ => {}
            }
        });
    }

    pub fn custom_backspace(&mut self, field: CustomField) {
        self.with_custom(|c| {
            c.error = None;
            match field {
                CustomField::Ops => {
                    if c.op_buffer.pop().is_none() {
                        c.ops.pop();
                    }
                }
                CustomField::Enum => {
                    if c.enum_buffer.pop().is_none() {
                        c.enum_map.pop();
                    }
                }
                CustomField::Decimals => {
                    c.decimals.pop();
                }
                CustomField::Prefix => {
                    c.prefix.pop();
                }
                CustomField::Suffix => {
                    c.suffix.pop();
                }
                _ => {}
            }
        });
    }

    pub fn custom_enter(&mut self, field: CustomField) {
        match field {
            CustomField::Ops => self.with_custom(|c| {
                if c.op_buffer.trim().is_empty() {
                    return;
                }
                match parse_op(&c.op_buffer) {
                    Ok(op) => {
                        c.ops.push(op);
                        c.op_buffer.clear();
                    }
                    Err(e) => c.error = Some(format!("op: {e}")),
                }
            }),
            CustomField::Enum => self.with_custom(|c| {
                if c.enum_buffer.trim().is_empty() {
                    return;
                }
                match parse_enum(&c.enum_buffer) {
                    Ok(entry) => {
                        c.enum_map.push(entry);
                        c.enum_buffer.clear();
                    }
                    Err(e) => c.error = Some(format!("enum: {e}")),
                }
            }),
            CustomField::Save => self.commit_custom(),
            CustomField::Remove => self.remove_custom(),
            _ => {}
        }
    }

    pub fn commit_custom(&mut self) {
        let built = match &self.read().popup {
            Some(Popup::Custom(c)) => build_custom_rule(c),
            _ => return,
        };
        match built {
            Ok((cell, rule)) => {
                self.custom_rules.insert(cell, rule);
                self.dirty = true;
                self.read_mut().popup = None;
                log::info!("Custom rule set \u{b7} {:?}@{}", cell.0, cell.1);
            }
            Err(e) => self.with_custom(|c| c.error = Some(e)),
        }
    }

    pub fn remove_custom(&mut self) {
        let cell = match &self.read().popup {
            Some(Popup::Custom(c)) => (c.register_type, c.address),
            _ => return,
        };
        if self.custom_rules.remove(&cell).is_some() {
            self.dirty = true;
            log::info!("Custom rule removed \u{b7} {:?}@{}", cell.0, cell.1);
        }
        self.read_mut().popup = None;
    }

    pub fn custom_rule(&self, cell: RegisterCell) -> Option<&crate::custom::CustomRule> {
        self.custom_rules.get(&cell)
    }

    pub(super) fn custom_value(
        &self,
        cell: RegisterCell,
        value: u16,
        word_order: WordOrder,
        neighbor: &impl Fn(u16) -> Option<u16>,
    ) -> Option<String> {
        let (kind, address) = cell;
        let Some(rule) = self.custom_rules.get(&cell) else {
            if !self.config.custom_rules.show_continuation {
                return None;
            }
            let prev = address.checked_sub(1)?;
            let prev_rule = self.custom_rules.get(&(kind, prev))?;
            return (prev_rule.repr.register_count() == 2).then(|| "part of \u{2191}".to_string());
        };
        let mut words = vec![value];
        if rule.repr.register_count() == 2 {
            if let Some(n) = neighbor(1) {
                words.push(n);
            }
        }
        let formatted = rule.evaluate(&words, word_order);
        (!formatted.is_empty()).then_some(formatted)
    }

    pub fn custom_preview(&self, c: &CustomParams) -> Result<(String, String), String> {
        let (cell, rule) = build_custom_rule(c)?;
        let Some(&(value, _)) = self.read_log.get(&cell) else {
            return Err("no value read yet".to_string());
        };
        let mut words = vec![value];
        if rule.repr.register_count() == 2 {
            match self.read_log.get(&(cell.0, cell.1.saturating_add(1))) {
                Some(&(n, _)) => words.push(n),
                None => return Err("waiting for second register".to_string()),
            }
        }
        let output = rule.evaluate(&words, self.config.device.word_order);
        if output.is_empty() {
            return Err("no output".to_string());
        }
        let input = words
            .iter()
            .map(|w| w.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        Ok((input, output))
    }
}
