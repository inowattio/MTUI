pub mod discovery;
pub mod logs;
pub mod popups;
pub mod read;
pub mod settings;

pub(crate) fn marker(selected: bool) -> &'static str {
    if selected {
        "> "
    } else {
        "  "
    }
}

pub(crate) fn cyclable(value: &str) -> String {
    format!("\u{2039} {value} \u{203a}")
}
