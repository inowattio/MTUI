use super::{StatusMessage, clamp_pick};
use crate::register::RegisterType;

field_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SweepField {
        From,
        To,
        Mode,
        Action,
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SweepConfigParams {
    pub from: u16,
    pub to: u16,
    pub continuous: bool,
    pub selected: u16,
}

impl SweepConfigParams {
    pub fn current_field(&self) -> SweepField {
        clamp_pick(self.selected, &SweepField::ALL)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitField {
    Id,
    From,
    To,
    Mode,
    Repr,
    Exceptions,
    Scan,
    Hit(usize),
}

impl UnitField {
    pub const fn is_toggle(self) -> bool {
        matches!(self, Self::Mode | Self::Repr | Self::Exceptions)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct UnitScanHit {
    pub unit_id: u8,
    pub result: Result<Vec<u16>, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ScanState {
    #[default]
    Idle,
    Probing,
    Done,
    Stopped,
    Failed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct UnitParams {
    pub id: u8,
    pub selected: u16,
    pub from: u8,
    pub to: u8,
    pub stop_at_first: bool,
    pub ascii: bool,
    pub show_exceptions: bool,
    pub scan: ScanState,
    pub current: u8,
    pub register_type: RegisterType,
    pub address: u16,
    pub amount: u16,
    pub hits: Vec<UnitScanHit>,
    pub status: Option<StatusMessage>,
}

impl Default for UnitParams {
    fn default() -> Self {
        Self {
            id: 0,
            selected: 0,
            from: 1,
            to: 247,
            stop_at_first: false,
            ascii: false,
            show_exceptions: true,
            scan: ScanState::Idle,
            current: 0,
            register_type: RegisterType::default(),
            address: 0,
            amount: 1,
            hits: Vec::new(),
            status: None,
        }
    }
}

impl UnitParams {
    const FIXED: [UnitField; 7] = [
        UnitField::Id,
        UnitField::From,
        UnitField::To,
        UnitField::Mode,
        UnitField::Repr,
        UnitField::Exceptions,
        UnitField::Scan,
    ];

    pub fn visible_hits(&self) -> impl Iterator<Item = (usize, &UnitScanHit)> {
        self.hits
            .iter()
            .enumerate()
            .filter(|(_, hit)| self.show_exceptions || hit.result.is_ok())
    }

    pub fn fields(&self) -> Vec<UnitField> {
        let mut fields = Self::FIXED.to_vec();
        fields.extend(self.visible_hits().map(|(i, _)| UnitField::Hit(i)));
        fields
    }

    pub fn current_field(&self) -> UnitField {
        let fields = self.fields();
        fields[(self.selected as usize).min(fields.len() - 1)]
    }

    pub fn switch_column(&mut self) {
        let first_hit = Self::FIXED.len() as u16;
        if matches!(self.current_field(), UnitField::Hit(_)) {
            self.selected = 0;
        } else if self.fields().len() as u16 > first_hit {
            self.selected = first_hit;
        }
    }

    pub fn suspended(mut self) -> Self {
        if self.active() {
            self.scan = ScanState::Stopped;
        }
        self.status = None;
        self
    }

    pub const fn resumed(
        mut self,
        id: u8,
        register_type: RegisterType,
        address: u16,
        amount: u16,
    ) -> Self {
        self.id = id;
        self.register_type = register_type;
        self.address = address;
        self.amount = amount;
        self
    }

    pub fn active(&self) -> bool {
        self.scan == ScanState::Probing
    }

    pub fn scanned(&self) -> bool {
        self.scan != ScanState::Idle
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn hidden_exception_hits_leave_the_field_list_but_keep_their_index() {
        use super::{UnitField, UnitParams, UnitScanHit};
        let hit = |id: u8, result: Result<Vec<u16>, String>| UnitScanHit {
            unit_id: id,
            result,
        };
        let mut params = UnitParams {
            hits: vec![
                hit(1, Ok(vec![1])),
                hit(2, Err("IllegalDataAddress".into())),
                hit(3, Ok(vec![3])),
            ],
            ..UnitParams::default()
        };
        let hits = |p: &UnitParams| -> Vec<UnitField> {
            p.fields()
                .into_iter()
                .filter(|f| matches!(f, UnitField::Hit(_)))
                .collect()
        };
        assert_eq!(
            hits(&params),
            vec![UnitField::Hit(0), UnitField::Hit(1), UnitField::Hit(2)]
        );

        params.show_exceptions = false;
        assert_eq!(hits(&params), vec![UnitField::Hit(0), UnitField::Hit(2)]);
    }

    #[test]
    fn a_remembered_scan_keeps_its_hits_but_follows_the_current_request() {
        use super::{ScanState, StatusMessage, UnitParams, UnitScanHit};
        use crate::constants::message;
        use crate::register::RegisterType;
        let params = UnitParams {
            scan: ScanState::Probing,
            from: 3,
            to: 9,
            status: Some(StatusMessage::info(message::DEVICE_BUSY)),
            hits: vec![UnitScanHit {
                unit_id: 5,
                result: Ok(vec![1]),
            }],
            ..UnitParams::default()
        };
        let resumed = params.suspended().resumed(7, RegisterType::Input, 40, 2);
        assert_eq!(
            resumed.scan,
            ScanState::Stopped,
            "a closed popup ends its scan"
        );
        assert_eq!(resumed.status, None);
        assert_eq!((resumed.from, resumed.to), (3, 9));
        assert_eq!(resumed.hits.len(), 1);
        assert_eq!(resumed.id, 7);
        assert_eq!(
            (resumed.register_type, resumed.address, resumed.amount),
            (RegisterType::Input, 40, 2)
        );

        let done = UnitParams {
            scan: ScanState::Done,
            ..UnitParams::default()
        };
        assert_eq!(done.suspended().scan, ScanState::Done);
    }

    #[test]
    fn tab_jumps_between_the_form_and_the_hit_list() {
        use super::{UnitField, UnitParams, UnitScanHit};
        let mut params = UnitParams {
            selected: 3,
            ..UnitParams::default()
        };
        params.switch_column();
        assert_eq!(params.current_field(), UnitField::Mode, "no hits, stay put");

        params.hits.push(UnitScanHit {
            unit_id: 5,
            result: Ok(vec![1]),
        });
        params.switch_column();
        assert_eq!(params.current_field(), UnitField::Hit(0));
        params.switch_column();
        assert_eq!(params.current_field(), UnitField::Id);
    }
}
