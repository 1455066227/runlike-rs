//! POSIX shell quoting compatible with Python's `shlex.quote`.

/// Characters that never need quoting. This is the set used by
/// `shlex.quote` (ASCII `\w` plus `@%+=:,./-`).
fn is_safe(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(c, '_' | '@' | '%' | '+' | '=' | ':' | ',' | '.' | '/' | '-')
}

/// Quote a value so that it can be embedded in a POSIX shell command line.
pub fn quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    if value.chars().all(is_safe) {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Quote a single command argument the way runlike historically printed
/// commands: single quotes, but escaped quotes rendered as `\'` for looks.
pub fn quote_command_arg(value: &str) -> String {
    quote(value).replace("'\"'\"'", "\\'")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_safe_strings_alone() {
        assert_eq!(quote("hello"), "hello");
        assert_eq!(quote("a=b:c,d.e/f-g+h%i@j_k"), "a=b:c,d.e/f-g+h%i@j_k");
    }

    #[test]
    fn quotes_empty_string() {
        assert_eq!(quote(""), "''");
    }

    #[test]
    fn quotes_spaces_and_specials() {
        assert_eq!(quote("hello world"), "'hello world'");
        assert_eq!(quote("$KEEP_DOLLAR"), "'$KEEP_DOLLAR'");
        assert_eq!(quote("\"quoted\""), "'\"quoted\"'");
        assert_eq!(quote("ユーザー別サイト"), "'ユーザー別サイト'");
    }

    #[test]
    fn escapes_single_quotes() {
        assert_eq!(quote("it's"), "'it'\"'\"'s'");
        assert_eq!(quote_command_arg("it's"), "'it\\'s'");
    }
}
