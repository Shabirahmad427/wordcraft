//! Shared validation for document hyperlink destinations.

/// Whether a destination is safe to preserve as a hyperlink in an exported document.
///
/// Web and email schemes are allowed. Fragment links and ordinary relative paths are also
/// allowed. Other explicit schemes (including executable and local-file schemes) are rejected.
/// Control characters are rejected before scheme parsing so embedded CR/LF/tab characters
/// cannot disguise a dangerous scheme.
pub fn safe_hyperlink_target(target: &str) -> bool {
    let target = target.trim();
    if target.is_empty() || target.chars().any(char::is_control) || target.starts_with(['\\']) || target.starts_with("//") {
        return false;
    }
    if target.starts_with('#') {
        return true;
    }
    let prefix = target.split(['/', '?', '#']).next().unwrap_or(target);
    if let Some((scheme, _)) = prefix.split_once(':') {
        if scheme.is_empty() || scheme.chars().any(char::is_whitespace) {
            return false;
        }
        return ["http", "https", "mailto"].iter().any(|allowed| scheme.eq_ignore_ascii_case(allowed));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::safe_hyperlink_target;

    #[test]
    fn hyperlink_allow_list_rejects_obfuscated_and_local_schemes() {
        for target in [
            "javascript:alert(1)",
            " JAVASCRIPT:alert(1)",
            "java\nscript:alert(1)",
            "java\tscript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,hi",
            "custom:launch",
            "//example.test/path",
            "https:\n//example.test",
        ] {
            assert!(!safe_hyperlink_target(target), "accepted {target:?}");
        }
        for target in
            ["https://example.test/a", "HTTP://example.test", "mailto:a@example.test", "#section", "chapter/one", "../notes.docx", "relative?x=1"]
        {
            assert!(safe_hyperlink_target(target), "rejected {target:?}");
        }
    }
}
