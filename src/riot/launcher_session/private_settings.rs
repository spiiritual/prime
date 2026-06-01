use super::*;

pub(super) fn update_private_settings_cookie_values(
    contents: &str,
    refreshed_cookies: &[LauncherCookie],
) -> Result<String, LauncherSessionError> {
    if refreshed_cookies
        .iter()
        .all(|cookie| cookie.name.trim().is_empty() || cookie.value.trim().is_empty())
    {
        return Ok(contents.to_string());
    }

    let newline = if contents.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let trailing_newline = contents.ends_with('\n');
    let mut lines = contents
        .lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line).to_string())
        .collect::<Vec<_>>();
    let mut output = Vec::with_capacity(lines.len());
    let mut cookies_indent = None;
    let mut pending: Option<PendingCookieEntry> = None;

    for (line_index, line) in lines.drain(..).enumerate() {
        let trimmed = line.trim();

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

        let leaves_cookie_list = if let Some(active_indent) = cookies_indent {
            !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && (indent < active_indent || indent == active_indent && !trimmed.starts_with("- "))
        } else {
            false
        };

        if leaves_cookie_list {
            flush_pending_cookie_entry(&mut output, &mut pending, refreshed_cookies);
            cookies_indent = None;
        }

        if cookies_indent.is_none() {
            if yaml_key_value(trimmed)
                .is_some_and(|(key, value)| key == "cookies" && value.is_empty())
            {
                cookies_indent = Some(indent);
            }
            output.push(line);
            continue;
        }

        if trimmed.is_empty() || trimmed.starts_with('#') {
            if let Some(entry) = pending.as_mut() {
                entry.lines.push(line);
            } else {
                output.push(line);
            }
            continue;
        }

        let starts_new_entry = trimmed.starts_with("- ");
        if starts_new_entry {
            flush_pending_cookie_entry(&mut output, &mut pending, refreshed_cookies);
            pending = Some(PendingCookieEntry {
                item_indent: indent,
                lines: vec![line],
            });
        } else if pending
            .as_ref()
            .is_some_and(|pending| indent < pending.item_indent)
        {
            flush_pending_cookie_entry(&mut output, &mut pending, refreshed_cookies);
            output.push(line);
        } else if let Some(entry) = pending.as_mut() {
            entry.lines.push(line);
        } else {
            output.push(line);
        }
    }

    flush_pending_cookie_entry(&mut output, &mut pending, refreshed_cookies);

    let mut updated = output.join(newline);
    if trailing_newline {
        updated.push_str(newline);
    }

    Ok(updated)
}

fn yaml_key_value(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    Some((key.trim(), value.trim()))
}

struct PendingCookieEntry {
    item_indent: usize,
    lines: Vec<String>,
}

#[derive(Default)]
struct ParsedCookieEntry {
    name: Option<String>,
    fields: Vec<CookieFieldLine>,
}

struct CookieFieldLine {
    key: String,
    line_index: usize,
}

fn flush_pending_cookie_entry(
    output: &mut Vec<String>,
    pending: &mut Option<PendingCookieEntry>,
    refreshed_cookies: &[LauncherCookie],
) {
    let Some(entry) = pending.take() else {
        return;
    };

    output.extend(updated_cookie_entry_lines(entry, refreshed_cookies));
}

fn updated_cookie_entry_lines(
    entry: PendingCookieEntry,
    refreshed_cookies: &[LauncherCookie],
) -> Vec<String> {
    let parsed = parse_cookie_entry(&entry.lines);
    let Some(name) = parsed.name.as_deref() else {
        return entry.lines;
    };
    let Some(cookie) = latest_refreshed_cookie(refreshed_cookies, name) else {
        return entry.lines;
    };

    let updates = cookie_yaml_updates(cookie);
    if updates.is_empty() {
        return entry.lines;
    }

    let field_indent = cookie_field_indent(&entry.lines, entry.item_indent);
    let mut updated = entry.lines;
    let mut applied = Vec::new();

    for field in &parsed.fields {
        let Some(value) = yaml_update_value(&updates, &field.key) else {
            continue;
        };

        updated[field.line_index] = replace_yaml_value_line(&updated[field.line_index], value);
        applied.push(field.key.clone());
    }

    for (key, value) in updates {
        if !applied.iter().any(|applied| applied == key) {
            updated.push(format!(
                "{}{}: {}",
                " ".repeat(field_indent),
                key,
                value.render()
            ));
        }
    }

    updated
}

fn parse_cookie_entry(lines: &[String]) -> ParsedCookieEntry {
    let mut entry = ParsedCookieEntry::default();

    for (line_index, line) in lines.iter().enumerate() {
        let Some((key, value)) = cookie_entry_key_value(line) else {
            continue;
        };

        entry.fields.push(CookieFieldLine {
            key: key.to_string(),
            line_index,
        });

        if key == "name" && !value.is_empty() {
            entry.name = Some(unquote_yaml_scalar(value));
        }
    }

    entry
}

fn latest_refreshed_cookie<'a>(
    refreshed_cookies: &'a [LauncherCookie],
    name: &str,
) -> Option<&'a LauncherCookie> {
    refreshed_cookies
        .iter()
        .rev()
        .find(|cookie| cookie.name.eq_ignore_ascii_case(name) && !cookie.value.trim().is_empty())
}

fn cookie_entry_key_value(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim();
    let normalized = trimmed
        .strip_prefix('-')
        .map(str::trim_start)
        .unwrap_or(trimmed);

    yaml_key_value(normalized)
}

fn cookie_field_indent(lines: &[String], item_indent: usize) -> usize {
    lines
        .iter()
        .find_map(|line| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('-') {
                None
            } else {
                Some(line.chars().take_while(|ch| *ch == ' ').count())
            }
        })
        .unwrap_or(item_indent + 2)
}

fn cookie_yaml_updates(cookie: &LauncherCookie) -> Vec<(&'static str, YamlScalar<'_>)> {
    let mut updates = Vec::new();

    if !cookie.value.trim().is_empty() {
        updates.push(("value", YamlScalar::String(cookie.value.trim())));
    }

    if let Some(domain) = cookie
        .metadata
        .domain
        .as_deref()
        .filter(|domain| !domain.trim().is_empty())
    {
        updates.push(("domain", YamlScalar::String(domain.trim())));
    }

    if let Some(path) = cookie
        .metadata
        .path
        .as_deref()
        .filter(|path| !path.trim().is_empty())
    {
        updates.push(("path", YamlScalar::String(path.trim())));
    }

    if let Some(persistent) = cookie.metadata.persistent {
        updates.push(("persistent", YamlScalar::Bool(persistent)));
    }

    if let Some(expires_at_unix) = cookie.metadata.expires_at_unix {
        updates.push(("expiryTime", YamlScalar::Integer(expires_at_unix)));
    }

    if let Some(host_only) = cookie.metadata.host_only {
        updates.push(("hostOnly", YamlScalar::Bool(host_only)));
    }

    if let Some(http_only) = cookie.metadata.http_only {
        updates.push(("httpOnly", YamlScalar::Bool(http_only)));
    }

    if let Some(secure_only) = cookie.metadata.secure_only {
        updates.push(("secureOnly", YamlScalar::Bool(secure_only)));
    }

    updates
}

fn yaml_update_value<'a>(
    updates: &'a [(&'static str, YamlScalar<'a>)],
    key: &str,
) -> Option<&'a YamlScalar<'a>> {
    updates
        .iter()
        .find(|(update_key, _)| *update_key == key)
        .map(|(_, value)| value)
}

enum YamlScalar<'a> {
    String(&'a str),
    Bool(bool),
    Integer(i64),
}

impl YamlScalar<'_> {
    fn render(&self) -> String {
        match self {
            Self::String(value) => quote_yaml_scalar(value),
            Self::Bool(value) => value.to_string(),
            Self::Integer(value) => value.to_string(),
        }
    }
}

fn replace_yaml_value_line(line: &str, value: &YamlScalar<'_>) -> String {
    let indent_len = line.chars().take_while(|ch| *ch == ' ').count();
    let indent = &line[..indent_len];
    let trimmed = line.trim_start();
    let (entry_prefix, field) = split_cookie_field_prefix(trimmed);
    let key = field.split_once(':').map(|(key, _)| key).unwrap_or("value");

    format!("{indent}{entry_prefix}{key}: {}", value.render())
}

fn split_cookie_field_prefix(line: &str) -> (&str, &str) {
    let Some(after_dash) = line.strip_prefix('-') else {
        return ("", line);
    };
    let spaces_len = after_dash
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(char::len_utf8)
        .sum::<usize>();
    let field_start = 1 + spaces_len;

    (&line[..field_start], &line[field_start..])
}

fn quote_yaml_scalar(value: &str) -> String {
    let escaped = value
        .chars()
        .flat_map(|ch| match ch {
            '\\' => ['\\', '\\'].into_iter().collect::<Vec<_>>(),
            '"' => ['\\', '"'].into_iter().collect::<Vec<_>>(),
            _ => [ch].into_iter().collect::<Vec<_>>(),
        })
        .collect::<String>();

    format!("\"{escaped}\"")
}
