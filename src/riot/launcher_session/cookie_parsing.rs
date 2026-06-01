use super::*;

pub fn parse_set_cookie_headers<'a>(
    headers: impl IntoIterator<Item = &'a str>,
) -> Vec<LauncherCookie> {
    parse_set_cookie_headers_at(headers, OffsetDateTime::now_utc().unix_timestamp())
}

pub(super) fn parse_set_cookie_headers_at<'a>(
    headers: impl IntoIterator<Item = &'a str>,
    now_unix: i64,
) -> Vec<LauncherCookie> {
    headers
        .into_iter()
        .filter_map(|header| parse_set_cookie_header(header, now_unix))
        .collect()
}

fn parse_set_cookie_header(header: &str, now_unix: i64) -> Option<LauncherCookie> {
    let mut parts = header.split(';').map(str::trim);
    let pair = parts.next()?;
    let (name, value) = pair.split_once('=')?;
    let name = name.trim();

    if name.is_empty() {
        return None;
    }

    let mut cookie = LauncherCookie::new(name, value.trim());
    for attribute in parts {
        let (key, value) = attribute
            .split_once('=')
            .map(|(key, value)| (key.trim(), value.trim()))
            .unwrap_or((attribute.trim(), ""));
        let key = key.to_ascii_lowercase();

        match key.as_str() {
            "domain" if !value.is_empty() => {
                cookie.metadata.domain = Some(value.trim_start_matches('.').to_string());
                cookie.metadata.host_only = Some(false);
            }
            "path" if !value.is_empty() => {
                cookie.metadata.path = Some(value.to_string());
            }
            "max-age" => {
                if let Ok(seconds) = value.parse::<i64>() {
                    cookie.metadata.expires_at_unix = Some(now_unix.saturating_add(seconds));
                    cookie.metadata.persistent = Some(true);
                }
            }
            "expires" => {
                if cookie.metadata.expires_at_unix.is_none()
                    && let Some(expires_at) = parse_cookie_expires(value)
                {
                    cookie.metadata.expires_at_unix = Some(expires_at);
                    cookie.metadata.persistent = Some(true);
                }
            }
            "httponly" => {
                cookie.metadata.http_only = Some(true);
            }
            "secure" => {
                cookie.metadata.secure_only = Some(true);
            }
            _ => {}
        }
    }

    Some(cookie)
}

fn parse_cookie_expires(value: &str) -> Option<i64> {
    OffsetDateTime::parse(value, &Rfc2822)
        .ok()
        .map(|datetime| datetime.unix_timestamp())
}

pub fn parse_private_settings_cookies(
    contents: &str,
) -> Result<Vec<LauncherCookie>, LauncherSessionError> {
    let mut cookies = Vec::new();
    let mut pending: Option<PendingCookie> = None;

    for (line_index, line) in contents.lines().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if line
            .chars()
            .take_while(|ch| ch.is_whitespace())
            .any(|ch| ch == '\t')
        {
            return Err(LauncherSessionError::PrivateSettingsFormat {
                line: line_index + 1,
                reason: "tabs are not valid indentation".to_string(),
            });
        }

        let indent = line.chars().take_while(|ch| *ch == ' ').count();
        let starts_new_entry = trimmed.starts_with("- ");

        if starts_new_entry
            || pending
                .as_ref()
                .is_some_and(|pending| indent < pending.item_indent)
        {
            flush_pending_cookie(&mut cookies, &mut pending);
        }

        let normalized = trimmed.strip_prefix("- ").unwrap_or(trimmed);
        let Some((key, value)) = normalized.split_once(':') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();

        if value.is_empty() {
            continue;
        }

        if matches!(key, "name" | "value") {
            if key == "name"
                && pending
                    .as_ref()
                    .is_some_and(|pending| pending.name.is_some() && pending.value.is_some())
            {
                flush_pending_cookie(&mut cookies, &mut pending);
            }

            let pending = pending.get_or_insert_with(|| PendingCookie::new(indent));

            match key {
                "name" => pending.name = Some(unquote_yaml_scalar(value)),
                "value" => pending.value = Some(unquote_yaml_scalar(value)),
                _ => {}
            }
        }
    }

    flush_pending_cookie(&mut cookies, &mut pending);

    Ok(cookies)
}

pub fn cookie_value(cookies: &[LauncherCookie], name: &str) -> Option<String> {
    cookies
        .iter()
        .find(|cookie| cookie.name.eq_ignore_ascii_case(name))
        .map(|cookie| cookie.value.clone())
}

struct PendingCookie {
    item_indent: usize,
    name: Option<String>,
    value: Option<String>,
}

impl PendingCookie {
    fn new(item_indent: usize) -> Self {
        Self {
            item_indent,
            name: None,
            value: None,
        }
    }
}

fn flush_pending_cookie(cookies: &mut Vec<LauncherCookie>, pending: &mut Option<PendingCookie>) {
    let Some(pending) = pending.take() else {
        return;
    };

    if let (Some(name), Some(value)) = (pending.name, pending.value) {
        push_cookie(cookies, name, value);
    }
}

fn push_cookie(cookies: &mut Vec<LauncherCookie>, name: String, value: String) {
    if !name.trim().is_empty() && !value.trim().is_empty() {
        cookies.push(LauncherCookie::new(name, value));
    }
}

pub(super) fn unquote_yaml_scalar(value: &str) -> String {
    let value = value.trim();

    if value.len() >= 2
        && ((value.starts_with('"') && value.ends_with('"'))
            || (value.starts_with('\'') && value.ends_with('\'')))
    {
        value[1..value.len() - 1].to_string()
    } else {
        value.to_string()
    }
}
