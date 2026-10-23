use super::{App, build_custom_rule};
use crate::custom::{
    ASCII_REGISTERS_DEFAULT, ASCII_REGISTERS_MAX, CustomCell, CustomRepr, parse_bit, parse_enum,
    parse_op,
};
use crate::interpretator::fmt_num;
use crate::modbus::WordOrder;
use crate::num_ops::{cycle, wrap_index};
use crate::register::RegisterCell;
use crate::state::{CustomField, CustomParams, Popup};

const PREVIEW_WORDS: usize = 8;

impl App {
    pub fn open_custom(&mut self) {
        let (register_type, address) = self.cursor_cell();

        let params = match self.custom_rules.get(&(register_type, address)) {
            Some(rule) => CustomParams {
                address,
                register_type,
                repr: rule.repr,
                registers: rule.registers.unwrap_or_default(),
                word_order: rule.word_order,
                next: rule.next.clone(),
                ops: rule.ops.clone(),
                enum_map: rule.enum_map.clone(),
                bits: rule.bits.clone(),
                decimals: rule.decimals.map(|d| d.to_string()).unwrap_or_default(),
                prefix: rule.prefix.clone(),
                suffix: rule.suffix.clone(),
                existed: true,
                ..Default::default()
            },
            None => CustomParams {
                address,
                register_type,
                ..Default::default()
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
            if c.list_move(down) {
                return;
            }
            let n = c.fields().len() as u16;
            c.selected = wrap_index(c.selected, n, down);
        });
    }

    pub fn custom_toggle_list(&mut self) {
        self.with_custom(|c| {
            c.error = None;
            c.toggle_list();
        });
    }

    pub fn custom_list_remove(&mut self) {
        self.with_custom(|c| {
            c.error = None;
            c.list_remove();
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
                CustomField::Repr => {
                    c.repr = cycle(&CustomRepr::ALL, c.repr, forward);
                    if c.is_ascii() && c.registers == 0 {
                        c.registers = ASCII_REGISTERS_DEFAULT;
                    }
                }
                CustomField::Registers => {
                    c.registers = if forward {
                        c.registers.saturating_add(1).min(ASCII_REGISTERS_MAX)
                    } else {
                        c.registers.saturating_sub(1).max(1)
                    };
                }
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
            c.list_index = None;
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
                CustomField::Registers => {
                    if let Some(digit) = ch.to_digit(10) {
                        let typed = u32::from(c.registers) * 10 + digit;
                        if typed <= u32::from(ASCII_REGISTERS_MAX) {
                            c.registers = typed as u8;
                        }
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
                CustomField::Registers => {
                    c.registers /= 10;
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
                    if c.next.len() + 1 >= c.register_count() {
                        c.error = Some(format!(
                            "next: {} uses {} register(s)",
                            c.repr.label(),
                            c.register_count()
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
                self.sync_auto_widths();
                self.refresh_dirty();
                self.read_mut().popup = None;
                log::info!("Custom rule set | {:?}@{}", cell.0, cell.1);
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
            self.sync_auto_widths();
            self.refresh_dirty();
            log::info!("Custom rule removed | {:?}@{}", cell.0, cell.1);
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
        at: &impl Fn(u16) -> Option<u16>,
    ) -> Option<CustomCell> {
        let (kind, address) = cell;
        let Some(rule) = self.custom_rules.get(&cell) else {
            if !self.config.display.rule_continuation {
                return None;
            }
            return self
                .custom_rules
                .range((kind, 0)..=(kind, u16::MAX))
                .find_map(|(&(_, owner), r)| {
                    let continuation = r.continuation(address)?;
                    Some(CustomCell::PartOf {
                        owner,
                        continuation,
                    })
                });
        };
        let mut words = vec![value];
        for word_address in rule.word_addresses().skip(1) {
            match at(word_address) {
                Some(n) => words.push(n),
                None => break,
            }
        }
        let formatted = rule.evaluate(&words, self.config.device.word_order);
        (!formatted.is_empty()).then_some(CustomCell::Value(formatted))
    }

    pub(super) fn custom_marker(&self, cell: CustomCell) -> String {
        match cell {
            CustomCell::Value(text) => text,
            CustomCell::PartOf {
                owner,
                continuation,
            } => continuation.marker(self.interpreter.address_mode().display(owner)),
        }
    }

    pub(super) fn custom_description(&self, cell: CustomCell) -> String {
        match cell {
            CustomCell::Value(text) => text,
            CustomCell::PartOf { owner, .. } => {
                format!("part of {}", self.interpreter.address_mode().display(owner))
            }
        }
    }

    pub fn custom_preview(&self, c: &CustomParams) -> Result<CustomPreview, String> {
        let (cell, rule) = build_custom_rule(c)?;
        let mut words = Vec::with_capacity(rule.register_count());
        let mut sources = Vec::with_capacity(rule.register_count());
        for word_address in rule.word_addresses() {
            let addr = self.interpreter.address_mode().display(word_address);
            match self.read_log.get(&(cell.0, word_address)) {
                Some(entry) => {
                    words.push(entry.value);
                    sources.push(format!("{addr}:{}", entry.value));
                }
                None => {
                    return Err(format!("waiting for register {addr}"));
                }
            }
        }

        let order = self.config.device.word_order;
        let output = rule.evaluate(&words, order);
        if output.is_empty() {
            return Err("no output".to_string());
        }

        let transforms = !rule.ops.is_empty() || !rule.enum_map.is_empty() || !rule.bits.is_empty();
        let base = (rule.register_count() > 1 || transforms)
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
                    let hex_width = 2 + rule.register_count() * 4;
                    Some(format!("{raw:#0hex_width$x}"))
                }
            })
            .flatten();

        let hidden = sources.len().saturating_sub(PREVIEW_WORDS);
        let mut words = sources[..sources.len() - hidden].join("  ");
        if hidden > 0 {
            words.push_str(&format!("  +{hidden} more"));
        }

        Ok(CustomPreview {
            words,
            base,
            output,
        })
    }

    pub fn clear_custom(&mut self) {
        let n = self.custom_rules.len();
        self.custom_rules.clear();
        self.sync_auto_widths();
        self.note_cleared(n, "custom rule");
    }
}

pub struct CustomPreview {
    pub words: String,
    pub base: Option<String>,
    pub output: String,
}
