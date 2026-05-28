use std::error::Error as StdError;

use url::Url;

pub(crate) fn format_reqwest_error(error: &reqwest::Error) -> String {
    let mut parts = Vec::new();
    push_unique(&mut parts, redact_error_text(&error.to_string()));

    if error.is_timeout() {
        push_unique(&mut parts, "request timed out".to_string());
    } else if error.is_connect() {
        push_unique(&mut parts, "connection failed".to_string());
    } else if error.is_decode() {
        push_unique(&mut parts, "response decode failed".to_string());
    } else if error.is_body() {
        push_unique(&mut parts, "request or response body failed".to_string());
    } else if error.is_redirect() {
        push_unique(&mut parts, "redirect failed".to_string());
    } else if error.is_builder() {
        push_unique(&mut parts, "HTTP client configuration failed".to_string());
    } else if error.is_request() {
        push_unique(
            &mut parts,
            "request failed before receiving a response".to_string(),
        );
    } else if error.is_upgrade() {
        push_unique(&mut parts, "protocol upgrade failed".to_string());
    }

    if let Some(status) = error.status() {
        let detail = format!("HTTP status {status}");
        if !parts.iter().any(|part| part.contains(status.as_str())) {
            push_unique(&mut parts, detail);
        }
    }

    if let Some(cause) = source_chain_summary(error) {
        push_unique(&mut parts, format!("cause: {cause}"));
    }

    parts.join("; ")
}

fn source_chain_summary(error: &(dyn StdError + 'static)) -> Option<String> {
    let mut causes = Vec::new();
    let mut source = error.source();

    while let Some(error) = source {
        push_unique(&mut causes, redact_error_text(&error.to_string()));
        source = error.source();
    }

    (!causes.is_empty()).then(|| causes.join(": "))
}

fn push_unique(parts: &mut Vec<String>, part: String) {
    let part = part.trim();

    if !part.is_empty() && !parts.iter().any(|existing| existing == part) {
        parts.push(part.to_string());
    }
}

fn redact_error_text(value: &str) -> String {
    value
        .split_whitespace()
        .map(redact_error_word)
        .collect::<Vec<_>>()
        .join(" ")
}

fn redact_error_word(word: &str) -> String {
    let prefix_len = word
        .find(|ch: char| ch.is_ascii_alphanumeric())
        .unwrap_or(word.len());
    let suffix_start = word
        .rfind(|ch: char| {
            ch.is_ascii_alphanumeric() || matches!(ch, '/' | '?' | '&' | '=' | '#' | '%' | '-')
        })
        .map(|index| index + 1)
        .unwrap_or(prefix_len);

    if prefix_len >= suffix_start {
        return word.to_string();
    }

    let (prefix, rest) = word.split_at(prefix_len);
    let (candidate, suffix) = rest.split_at(suffix_start - prefix_len);

    let Some(redacted) = redact_url(candidate) else {
        return word.to_string();
    };

    format!("{prefix}{redacted}{suffix}")
}

fn redact_url(value: &str) -> Option<String> {
    if !value.contains("://") {
        return None;
    }

    let mut url = Url::parse(value).ok()?;

    if !url.username().is_empty() {
        let _ = url.set_username("redacted");
    }

    if url.password().is_some() {
        let _ = url.set_password(Some("redacted"));
    }

    if url.query().is_some() {
        url.set_query(Some("redacted"));
    }

    if url.fragment().is_some() {
        url.set_fragment(Some("redacted"));
    }

    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use std::fmt;

    use super::*;

    #[derive(Debug)]
    struct OuterError(MiddleError);

    impl fmt::Display for OuterError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("outer")
        }
    }

    impl StdError for OuterError {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            Some(&self.0)
        }
    }

    #[derive(Debug)]
    struct MiddleError(InnerError);

    impl fmt::Display for MiddleError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("middle")
        }
    }

    impl StdError for MiddleError {
        fn source(&self) -> Option<&(dyn StdError + 'static)> {
            Some(&self.0)
        }
    }

    #[derive(Debug)]
    struct InnerError;

    impl fmt::Display for InnerError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("inner")
        }
    }

    impl StdError for InnerError {}

    #[test]
    fn source_chain_summary_includes_nested_causes() {
        let error = OuterError(MiddleError(InnerError));

        assert_eq!(
            source_chain_summary(&error).as_deref(),
            Some("middle: inner")
        );
    }

    #[test]
    fn error_text_redacts_url_secrets() {
        let text =
            "proxy (http://user:secret@example.com/path?token=abc#frag) could not be reached";

        let redacted = redact_error_text(text);

        assert!(!redacted.contains("user"));
        assert!(!redacted.contains("secret"));
        assert!(!redacted.contains("token=abc"));
        assert!(!redacted.contains("#frag"));
        assert!(redacted.contains("http://redacted:redacted@example.com/path?redacted#redacted"));
    }
}
