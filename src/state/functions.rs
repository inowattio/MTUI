use super::StatusMessage;
use crate::functions::DiagnosticSub;
use crate::modbus::DeviceIdAccess;

field_enum! {
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub enum OtherFunction {
        #[default]
        ReadExceptionStatus,
        Diagnostics,
        CommEventCounter,
        CommEventLog,
        ReportServerId,
        ReadFileRecord,
        ReadFifoQueue,
        DeviceIdentification,
    }
}

impl OtherFunction {
    pub const fn code(self) -> &'static str {
        match self {
            Self::ReadExceptionStatus => "0x07",
            Self::Diagnostics => "0x08",
            Self::CommEventCounter => "0x0B",
            Self::CommEventLog => "0x0C",
            Self::ReportServerId => "0x11",
            Self::ReadFileRecord => "0x14",
            Self::ReadFifoQueue => "0x18",
            Self::DeviceIdentification => "0x2B/0x0E",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::ReadExceptionStatus => "Read exception status",
            Self::Diagnostics => "Diagnostics",
            Self::CommEventCounter => "Get comm event counter",
            Self::CommEventLog => "Get comm event log",
            Self::ReportServerId => "Report server ID",
            Self::ReadFileRecord => "Read file record",
            Self::ReadFifoQueue => "Read FIFO queue",
            Self::DeviceIdentification => "Device identification",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionField {
    SubFunction,
    Data,
    File,
    Record,
    Length,
    Address,
    Access,
}

impl FunctionField {
    pub const fn label(self) -> &'static str {
        match self {
            Self::SubFunction => "Sub-function",
            Self::Data => "Data (hex)",
            Self::File => "File number",
            Self::Record => "Record number",
            Self::Length => "Record length",
            Self::Address => "FIFO address",
            Self::Access => "Access",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct FunctionParams {
    pub function: OtherFunction,
    pub field: usize,
    pub sub: DiagnosticSub,
    pub access: DeviceIdAccess,
    pub data: String,
    pub file: String,
    pub record: String,
    pub length: String,
    pub address: String,
    pub rows: Vec<(String, String)>,
    pub scroll: u16,
    pub status: Option<StatusMessage>,
    pub loading: bool,
    pub pending: bool,
}

impl FunctionParams {
    pub fn new(function: OtherFunction) -> Self {
        Self {
            function,
            field: 0,
            sub: DiagnosticSub::default(),
            access: DeviceIdAccess::default(),
            data: "0000".to_string(),
            file: "1".to_string(),
            record: "0".to_string(),
            length: "1".to_string(),
            address: "0".to_string(),
            rows: Vec::new(),
            scroll: 0,
            status: None,
            loading: false,
            pending: false,
        }
    }

    pub const fn fields(&self) -> &'static [FunctionField] {
        match self.function {
            OtherFunction::Diagnostics => match self.sub {
                DiagnosticSub::ReturnQueryData => {
                    &[FunctionField::SubFunction, FunctionField::Data]
                }
                _ => &[FunctionField::SubFunction],
            },
            OtherFunction::ReadFileRecord => &[
                FunctionField::File,
                FunctionField::Record,
                FunctionField::Length,
            ],
            OtherFunction::ReadFifoQueue => &[FunctionField::Address],
            OtherFunction::DeviceIdentification => &[FunctionField::Access],
            _ => &[],
        }
    }

    pub fn current_field(&self) -> Option<FunctionField> {
        let fields = self.fields();
        fields
            .get(self.field.min(fields.len().saturating_sub(1)))
            .copied()
    }

    pub const fn text_mut(&mut self, field: FunctionField) -> Option<&mut String> {
        match field {
            FunctionField::SubFunction | FunctionField::Access => None,
            FunctionField::Data => Some(&mut self.data),
            FunctionField::File => Some(&mut self.file),
            FunctionField::Record => Some(&mut self.record),
            FunctionField::Length => Some(&mut self.length),
            FunctionField::Address => Some(&mut self.address),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct FunctionsParams {
    pub selected: usize,
    pub detail: bool,
    pub forms: Vec<FunctionParams>,
}

impl Default for FunctionsParams {
    fn default() -> Self {
        Self {
            selected: 0,
            detail: false,
            forms: OtherFunction::ALL.map(FunctionParams::new).into(),
        }
    }
}

impl FunctionsParams {
    const fn index(&self) -> usize {
        let last = self.forms.len().saturating_sub(1);
        if self.selected < last {
            self.selected
        } else {
            last
        }
    }

    pub fn current(&self) -> &FunctionParams {
        &self.forms[self.index()]
    }

    pub fn current_mut(&mut self) -> &mut FunctionParams {
        let index = self.index();
        &mut self.forms[index]
    }

    pub fn form_mut(&mut self, function: OtherFunction) -> Option<&mut FunctionParams> {
        self.forms.iter_mut().find(|form| form.function == function)
    }
}
