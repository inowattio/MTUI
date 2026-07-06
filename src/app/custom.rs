use super::{build_custom_rule, App};
use crate::custom::{parse_bit, parse_enum, parse_op, CustomRepr};
use crate::interpretator::fmt_num;
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
                word_order: rule.word_order,
                next: rule.next.clone(),
                ops: rule.ops.clone(),
                enum_map: rule.enum_map.clone(),
                bits: rule.bits.clone(),
                decimals: rule.decimals.map(|d| d.to_string()).unwrap_or_default(),
                prefix: rule.prefix.clone(),
                suffix: rule.suffix.clone(),
                op_buffer: String::new(),
                enum_buffer: String::new(),
                bit_buffer: String::new(),
                next_buffer: String::new(),
                selected: 0,
                existed: true,
                error: None,
            },
            None => CustomParams {
                address,
                register_type,
                repr: CustomRepr::default(),
                word_order: None,
                next: Vec::new(),
                ops: Vec::new(),
                enum_map: Vec::new(),
                bits: Vec::new(),
                decimals: String::new(),
                prefix: String::new(),
                suffix: String::new(),
                op_buffer: String::new(),
                enum_buffer: String::new(),
                bit_buffer: String::new(),
                next_buffer: String::new(),
                selected: 0,
                existed: false,
                error: None,
            },
        };
        self.read_mut().popup = Some(Popup::Custom(params));
    }

    fn with_custom(&mut self, f: impl FnOnce(&mut CustomParams)) {
        if let Some(c) = self.popup_as_mut::<CustomParams>() {
            f(c);
        }
    }

    pub fn custom_move(&mut self, down: bool) {
        self.with_custom(|c| {
            c.error = None;
            let n = c.fields().len() as u16;
            c.selected = wrap_index(c.selected.min(n - 1), n, down);
        });
    }

    pub fn custom_cycle(&mut self, field: CustomField, forward: bool) {
        const ORDER_CHOICES: [Option<WordOrder>; 5] = [
            None,
            Some(WordOrder::ABCD),
            Some(WordOrder::BADC),
            Some(WordOrder::CDAB),
            Some(WordOrder::DCBA),
        ];
        self.with_custom(|c| {
            c.error = None;
            match field {
                CustomField::Repr => c.repr = cycle(&CustomRepr::ALL, c.repr, forward),
                CustomField::WordOrder => {
                    c.word_order = cycle(&ORDER_CHOICES, c.word_order, forward);
                }
                _ => {}
            }
            c.reselect(field);
        });
    }

    pub fn custom_char(&mut self, field: CustomField, ch: char) {
        self.with_custom(|c| {
            c.error = None;
            match field {
                CustomField::Ops => c.op_buffer.push(ch),
                CustomField::Enum => c.enum_buffer.push(ch),
                CustomField::Bits => c.bit_buffer.push(ch),
                CustomField::Next => {
                    if ch.is_ascii_digit() && c.next_buffer.len() < 5 {
                        c.next_buffer.push(ch);
                    }
                }
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
                CustomField::Bits => {
                    if c.bit_buffer.pop().is_none() {
                        c.bits.pop();
                    }
                }
                CustomField::Next => {
                    if c.next_buffer.pop().is_none() {
                        c.next.pop();
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
            c.reselect(field);
        });
    }

    pub fn custom_enter(&mut self, field: CustomField) {
        let mut adding = false;
        self.with_custom(|c| {
            adding = match field {
                CustomField::Ops => !c.op_buffer.trim().is_empty(),
                CustomField::Enum => !c.enum_buffer.trim().is_empty(),
                CustomField::Bits => !c.bit_buffer.trim().is_empty(),
                CustomField::Next => !c.next_buffer.trim().is_empty(),
                _ => false,
            };
            if !adding {
                return;
            }
            match field {
                CustomField::Ops => match parse_op(&c.op_buffer) {
                    Ok(op) => {
                        c.ops.push(op);
                        c.op_buffer.clear();
                    }
                    Err(e) => c.error = Some(format!("op: {e}")),
                },
                CustomField::Enum => match parse_enum(&c.enum_buffer) {
                    Ok(entry) => {
                        c.enum_map.push(entry);
                        c.enum_buffer.clear();
                    }
                    Err(e) => c.error = Some(format!("enum: {e}")),
                },
                CustomField::Bits => match parse_bit(&c.bit_buffer) {
                    Ok(entry) => {
                        c.bits.push(entry);
                        c.bit_buffer.clear();
                    }
                    Err(e) => c.error = Some(format!("bit: {e}")),
                },
                CustomField::Next => {
                    if c.next.len() + 1 >= c.repr.register_count() {
                        c.error = Some(format!(
                            "next: {} uses {} register(s)",
                            c.repr.label(),
                            c.repr.register_count()
                        ));
                    } else {
                        match c.next_buffer.trim().parse::<u16>() {
                            Ok(address) => {
                                c.next.push(address);
                                c.next_buffer.clear();
                            }
                            Err(_) => c.error = Some("next: invalid address".to_string()),
                        }
                    }
                }
                _ => {}
            }
            c.reselect(field);
        });
        if !adding {
            self.commit_custom();
        }
    }

    pub fn commit_custom(&mut self) {
        let Some(built) = self.popup_as::<CustomParams>().map(build_custom_rule) else {
            return;
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
        let Some(cell) = self
            .popup_as::<CustomParams>()
            .map(|c| (c.register_type, c.address))
        else {
            return;
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
        at: &impl Fn(u16) -> Option<u16>,
    ) -> Option<String> {
        let (kind, address) = cell;
        let Some(rule) = self.custom_rules.get(&cell) else {
            if !self.config.custom_rules.show_continuation {
                return None;
            }
            return self
                .custom_rules
                .range((kind, 0)..=(kind, u16::MAX))
                .find_map(|(&(_, owner), r)| {
                    let position = r
                        .word_addresses()
                        .into_iter()
                        .skip(1)
                        .position(|a| a == address)? as u16;
                    Some(if address == owner.wrapping_add(position + 1) {
                        "part of \u{2191}".to_string()
                    } else {
                        format!("part of {owner}")
                    })
                });
        };
        let mut words = vec![value];
        for word_address in rule.word_addresses().into_iter().skip(1) {
            match at(word_address) {
                Some(n) => words.push(n),
                None => break,
            }
        }
        let formatted = rule.evaluate(&words, word_order);
        (!formatted.is_empty()).then_some(formatted)
    }

    pub fn custom_preview(&self, c: &CustomParams) -> Result<CustomPreview, String> {
        let (cell, rule) = build_custom_rule(c)?;
        let mut words = Vec::with_capacity(rule.repr.register_count());
        let mut sources = Vec::with_capacity(rule.repr.register_count());
        for word_address in rule.word_addresses() {
            match self.read_log.get(&(cell.0, word_address)) {
                Some(&(n, _)) => {
                    words.push(n);
                    sources.push(format!("{word_address}:{n}"));
                }
                None => return Err(format!("waiting for register {word_address}")),
            }
        }

        let order = self.config.device.word_order;
        let output = rule.evaluate(&words, order);
        if output.is_empty() {
            return Err("no output".to_string());
        }

        let transforms = !rule.ops.is_empty() || !rule.enum_map.is_empty() || !rule.bits.is_empty();
        let base = (rule.repr.register_count() > 1 || transforms)
            .then(|| {
                if rule.bits.is_empty() {
                    let b = rule.base(&words, order)?;
                    Some(format!(
                        "{} {}",
                        rule.repr.label(),
                        fmt_num(b, b.fract() != 0.0)
                    ))
                } else {
                    let raw = rule.raw(&words, order)?;
                    let hex_width = 2 + rule.repr.register_count() * 4;
                    Some(format!("{raw:#0hex_width$x}"))
                }
            })
            .flatten();

        Ok(CustomPreview {
            words: sources.join("  "),
            base,
            output,
        })
    }
}

pub struct CustomPreview {
    pub words: String,
    pub base: Option<String>,
    pub output: String,
}
