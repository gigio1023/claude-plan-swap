//! Formatting helpers that are not tied to a domain model.

use std::path::Path;

pub(crate) fn shell_quote(path: &Path) -> String {
    let text = path.to_string_lossy();
    format!("'{}'", text.replace('\'', r#"'\''"#))
}

pub(crate) fn escape_applescript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

pub(crate) fn display_pct(value: Option<u8>) -> String {
    value
        .map(|value| format!("{value}%"))
        .unwrap_or_else(|| "-".to_string())
}

pub(crate) fn display_ts(value: Option<i64>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_shell_paths() {
        assert_eq!(
            shell_quote(Path::new("/tmp/a b/plan's/bin")),
            "'/tmp/a b/plan'\\''s/bin'"
        );
    }
}
