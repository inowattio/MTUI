use crate::custom::{BitEntry, CustomOp, CustomRepr, EnumEntry, ScaleMode, ScaleRepr};
use crate::modbus::WordOrder;
use crate::num_ops::wrap_index;
use crate::register::RegisterType;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CustomField {
    Repr,
    Registers,
    WordOrder,
    Next,
    Scale,
    ScaleType,
    ScaleRegister,
    ScaleWordOrder,
    Ops,
    Enum,
    Bits,
    Decimals,
    Prefix,
    Suffix,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomParams {
    pub address: u16,
    pub register_type: RegisterType,
    pub repr: CustomRepr,
    pub registers: u8,
    pub word_order: Option<WordOrder>,
    pub next: Vec<u16>,
    pub scale_mode: Option<ScaleMode>,
    pub scale_repr: ScaleRepr,
    pub scale_register: String,
    pub scale_word_order: Option<WordOrder>,
    pub ops: Vec<CustomOp>,
    pub enum_map: Vec<EnumEntry>,
    pub bits: Vec<BitEntry>,
    pub decimals: String,
    pub prefix: String,
    pub suffix: String,
    pub op_buffer: String,
    pub enum_buffer: String,
    pub bit_buffer: String,
    pub next_buffer: String,
    pub selected: u16,
    pub list_index: Option<u16>,
    pub existed: bool,
    pub error: Option<String>,
}

impl CustomParams {
    pub fn is_ascii(&self) -> bool {
        self.repr == CustomRepr::Ascii
    }

    pub fn register_count(&self) -> usize {
        if self.is_ascii() {
            usize::from(self.registers.max(1))
        } else {
            self.repr.register_count()
        }
    }

    pub fn fields(&self) -> Vec<CustomField> {
        use CustomField::*;
        let ascii = self.is_ascii();
        let multi = self.register_count() > 1;
        let bits_active = !self.bits.is_empty();

        let mut fields = vec![Repr];
        if ascii {
            fields.push(Registers);
        }
        fields.push(WordOrder);
        if multi || !self.next.is_empty() {
            fields.push(Next);
        }
        if ascii {
            fields.push(Prefix);
            fields.push(Suffix);
            return fields;
        }
        if !bits_active || self.scale_mode.is_some() {
            fields.push(Scale);
            if self.scale_mode.is_some() {
                fields.push(ScaleType);
                fields.push(ScaleRegister);
                fields.push(ScaleWordOrder);
            }
        }
        if !bits_active || !self.ops.is_empty() {
            fields.push(Ops);
        }
        fields.push(Enum);
        fields.push(Bits);
        if !bits_active || !self.decimals.is_empty() {
            fields.push(Decimals);
        }
        fields.push(Prefix);
        fields.push(Suffix);
        fields
    }

    pub fn current_field(&self) -> CustomField {
        let fields = self.fields();
        fields[(self.selected as usize).min(fields.len() - 1)]
    }

    pub fn reselect(&mut self, field: CustomField) {
        let fields = self.fields();
        self.selected = match fields.iter().position(|&f| f == field) {
            Some(i) => i as u16,
            None => self.selected.min(fields.len() as u16 - 1),
        };
    }

    pub fn list_len(&self) -> usize {
        match self.current_field() {
            CustomField::Enum => self.enum_map.len(),
            CustomField::Bits => self.bits.len(),
            _ => 0,
        }
    }

    pub fn toggle_list(&mut self) {
        self.list_index = match (self.list_index, self.list_len()) {
            (None, len) if len > 0 => Some(len as u16 - 1),
            _ => None,
        };
    }

    pub fn list_move(&mut self, down: bool) -> bool {
        let Some(i) = self.list_index else {
            return false;
        };
        self.list_index = Some(wrap_index(i, self.list_len() as u16, down));
        true
    }

    pub fn list_remove(&mut self) -> bool {
        let Some(i) = self.list_index else {
            return false;
        };
        let i = (i as usize).min(self.list_len().saturating_sub(1));
        let field = self.current_field();
        match field {
            CustomField::Enum if !self.enum_map.is_empty() => {
                self.enum_map.remove(i);
            }
            CustomField::Bits if !self.bits.is_empty() => {
                self.bits.remove(i);
            }
            _ => {}
        }
        self.reselect(field);
        let len = self.list_len();
        self.list_index = (len > 0).then(|| i.min(len - 1) as u16);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{CustomField, CustomParams};
    use crate::custom::{BitEntry, CustomRepr, EnumEntry, ScaleMode, ScaleRepr};

    #[test]
    fn scale_type_and_register_show_only_once_a_scale_is_chosen() {
        use CustomField::*;
        let mut c = CustomParams::default();
        assert!(c.fields().contains(&Scale));
        assert!(!c.fields().contains(&ScaleType));
        assert!(!c.fields().contains(&ScaleRegister));
        assert!(!c.fields().contains(&ScaleWordOrder));

        for mode in ScaleMode::ALL {
            c.scale_mode = Some(mode);
            let fields = c.fields();
            let at = |f| fields.iter().position(|&x| x == f).unwrap();
            assert_eq!(at(ScaleType), at(Scale) + 1);
            assert_eq!(at(ScaleRegister), at(ScaleType) + 1);
            assert_eq!(at(ScaleWordOrder), at(ScaleRegister) + 1);
            assert!(at(ScaleWordOrder) < at(Ops));
        }

        c.bits = vec![BitEntry {
            bit: 0,
            name: "b".into(),
        }];
        assert!(c.fields().contains(&ScaleWordOrder));
        c.scale_mode = None;
        assert!(!c.fields().contains(&ScaleWordOrder));
        c.bits.clear();
        c.scale_mode = Some(ScaleMode::Pow10);

        c.repr = CustomRepr::Ascii;
        assert!(!c.fields().contains(&Scale));
    }

    #[test]
    fn every_rule_offers_a_word_order() {
        use CustomField::*;
        let mut c = CustomParams {
            scale_mode: Some(ScaleMode::Pow10),
            scale_repr: ScaleRepr::Plain(CustomRepr::F32),
            ..CustomParams::default()
        };
        assert!(c.fields().contains(&WordOrder));
        c.scale_repr = ScaleRepr::Sunspec;
        assert!(c.fields().contains(&WordOrder));
        c.scale_mode = None;
        for repr in [
            CustomRepr::U16,
            CustomRepr::I16,
            CustomRepr::F16,
            CustomRepr::U64,
        ] {
            c.repr = repr;
            assert!(c.fields().contains(&WordOrder), "{repr:?}");
        }
    }

    #[test]
    fn ascii_offers_only_decode_and_affixes() {
        use CustomField::*;
        let mut c = params();
        c.repr = CustomRepr::Ascii;
        c.registers = 1;
        assert_eq!(c.fields(), vec![Repr, Registers, WordOrder, Prefix, Suffix]);
        c.registers = 8;
        assert_eq!(c.register_count(), 8);
        assert_eq!(
            c.fields(),
            vec![Repr, Registers, WordOrder, Next, Prefix, Suffix]
        );
    }

    fn params() -> CustomParams {
        let mut c = CustomParams {
            enum_map: (0..3)
                .map(|i| EnumEntry {
                    value: i,
                    text: format!("s{i}"),
                })
                .collect(),
            bits: vec![BitEntry {
                bit: 0,
                name: "run".into(),
            }],
            ..CustomParams::default()
        };
        c.reselect(CustomField::Enum);
        c
    }

    #[test]
    fn tab_enters_the_list_at_its_end_and_leaves_it_again() {
        let mut c = params();
        c.toggle_list();
        assert_eq!(c.list_index, Some(2));
        c.toggle_list();
        assert_eq!(c.list_index, None);
    }

    #[test]
    fn tab_does_nothing_off_a_list_field_or_on_an_empty_list() {
        let mut c = params();
        c.reselect(CustomField::Prefix);
        c.toggle_list();
        assert_eq!(c.list_index, None);
        c.enum_map.clear();
        c.reselect(CustomField::Enum);
        c.toggle_list();
        assert_eq!(c.list_index, None);
    }

    #[test]
    fn moving_wraps_inside_the_list_and_is_ignored_outside_it() {
        let mut c = params();
        assert!(!c.list_move(true));
        c.toggle_list();
        assert!(c.list_move(true));
        assert_eq!(c.list_index, Some(0));
        c.list_move(false);
        assert_eq!(c.list_index, Some(2));
    }

    #[test]
    fn removing_drops_the_selected_entry_and_keeps_a_valid_index() {
        let mut c = params();
        c.toggle_list();
        c.list_move(true);
        c.list_move(true);
        assert!(c.list_remove());
        assert_eq!(
            c.enum_map.iter().map(|e| e.value).collect::<Vec<_>>(),
            [0, 2]
        );
        assert_eq!(c.list_index, Some(1));
        c.list_remove();
        assert_eq!(c.list_index, Some(0));
        c.list_remove();
        assert!(c.enum_map.is_empty());
        assert_eq!(c.list_index, None);
    }

    #[test]
    fn removing_on_bits_uses_the_bits_list() {
        let mut c = params();
        c.reselect(CustomField::Bits);
        c.toggle_list();
        c.list_remove();
        assert!(c.bits.is_empty());
        assert_eq!(c.enum_map.len(), 3);
    }

    #[test]
    fn removing_the_last_bit_stays_on_bits_and_leaves_the_list() {
        let mut c = params();
        c.reselect(CustomField::Bits);
        c.toggle_list();
        c.list_remove();
        assert_eq!(c.current_field(), CustomField::Bits);
        assert_eq!(c.list_index, None);

        c.list_remove();
        assert_eq!(
            c.enum_map.len(),
            3,
            "a second removal must not reach the enum list"
        );
    }
}
