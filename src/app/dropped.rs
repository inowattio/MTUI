use super::diff::PastedDump;
use crate::config::AddressMode;
use crate::snapshot::{self, SnapshotError};
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};

const DUMP_HEAD_BYTES: u64 = 4096;
const DUMP_FILE_LIMIT: u64 = 256 * 1024 * 1024;

pub(super) fn dropped_dump(text: &str, mode: AddressMode) -> PastedDump {
    let Some(path) = dropped_path(text) else {
        return PastedDump::Other;
    };
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let parsed = match dump_file_text(&path) {
        Err(error) => return PastedDump::Invalid(format!("Couldn't read {name}: {error}")),
        Ok(None) => Err(SnapshotError::NotADump),
        Ok(Some(content)) => snapshot::parse_dump(&content, mode),
    };
    match parsed {
        Ok(snapshot) => PastedDump::Snapshot(snapshot),
        Err(error) => PastedDump::Invalid(format!("{name}: {error}")),
    }
}

fn dump_file_text(path: &Path) -> io::Result<Option<String>> {
    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(DUMP_HEAD_BYTES)
        .read_to_end(&mut bytes)?;
    if !snapshot::is_dump(&String::from_utf8_lossy(&bytes)) {
        return Ok(None);
    }
    if file.metadata()?.len() > DUMP_FILE_LIMIT {
        return Err(io::ErrorKind::FileTooLarge.into());
    }
    file.read_to_end(&mut bytes)?;
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

fn dropped_path(text: &str) -> Option<PathBuf> {
    if text.contains(['\n', '\r']) {
        return None;
    }
    path_candidates(text)
        .into_iter()
        .find(|path| path.is_file())
}

fn path_candidates(text: &str) -> Vec<PathBuf> {
    let text = unquoted(text.trim());
    if let Some(uri) = text.strip_prefix("file://") {
        return percent_decoded(uri)
            .map(PathBuf::from)
            .into_iter()
            .collect();
    }
    let unescaped = unescaped(text);
    let mut candidates = vec![PathBuf::from(text)];
    if unescaped != text {
        candidates.push(PathBuf::from(unescaped));
    }
    candidates
}

fn unquoted(text: &str) -> &str {
    ['"', '\'']
        .into_iter()
        .find_map(|quote| text.strip_prefix(quote)?.strip_suffix(quote))
        .unwrap_or(text)
}

fn unescaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match chars.peek() {
            Some(&next) if c == '\\' && is_shell_escaped(next) => {
                out.push(next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

fn is_shell_escaped(c: char) -> bool {
    c.is_whitespace() || "!\"#$&'()*,;<=>?[\\]^`{|}~".contains(c)
}

fn percent_decoded(text: &str) -> Option<String> {
    let mut bytes = text.bytes();
    let mut out = Vec::with_capacity(text.len());
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let high = char::from(bytes.next()?).to_digit(16)?;
            let low = char::from(bytes.next()?).to_digit(16)?;
            out.push(u8::try_from(high * 16 + low).ok()?);
        } else {
            out.push(byte);
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::path_candidates;
    use std::path::PathBuf;

    #[test]
    fn dropped_paths_are_unquoted_decoded_and_unescaped() {
        let paths = |text: &str| -> Vec<PathBuf> { path_candidates(text) };
        assert_eq!(paths("/tmp/dump.csv"), vec![PathBuf::from("/tmp/dump.csv")]);
        assert_eq!(
            paths("'/tmp/my dump.csv'"),
            vec![PathBuf::from("/tmp/my dump.csv")]
        );
        assert_eq!(
            paths("\"/tmp/my dump.csv\""),
            vec![PathBuf::from("/tmp/my dump.csv")]
        );
        assert_eq!(
            paths("file:///tmp/my%20dump%2Bfinal.csv"),
            vec![PathBuf::from("/tmp/my dump+final.csv")]
        );
        assert!(paths("file:///tmp/broken%2").is_empty());
        assert_eq!(
            paths("/tmp/my\\ dump\\(1\\).csv"),
            vec![
                PathBuf::from("/tmp/my\\ dump\\(1\\).csv"),
                PathBuf::from("/tmp/my dump(1).csv"),
            ],
            "the literal path is tried first for Windows separators"
        );
        assert_eq!(
            paths("C:\\Users\\me\\my\\ dump\\(1\\).csv"),
            vec![
                PathBuf::from("C:\\Users\\me\\my\\ dump\\(1\\).csv"),
                PathBuf::from("C:\\Users\\me\\my dump(1).csv"),
            ],
            "separators before plain names are kept"
        );
        assert_eq!(
            paths("/tmp/back\\\\slash\\ it.csv"),
            vec![
                PathBuf::from("/tmp/back\\\\slash\\ it.csv"),
                PathBuf::from("/tmp/back\\slash it.csv"),
            ]
        );
    }
}
