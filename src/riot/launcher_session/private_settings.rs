const PSL_REFRESH_TOKEN_PATH: [&str; 4] = ["psl", "authorization", "riot-client", "refresh_token"];

/// Reads the remembered Riot Client refresh token from `RiotGamesPrivateSettings.yaml`.
pub fn private_settings_refresh_token(contents: &str) -> Option<String> {
    let lines = contents.split('\n').collect::<Vec<_>>();
    let index = find_yaml_path_line(&lines, &PSL_REFRESH_TOKEN_PATH)?;
    let (_, value) = lines[index].trim().split_once(':')?;
    let token = unquote_yaml_scalar(value);

    (!token.is_empty() && token != "null" && token != "~").then_some(token)
}

/// True when the file holds a login saved by older Riot Client versions: an `ssid` cookie in the
/// `riot-login` block and no refresh token. Those sessions can no longer be restored.
pub(super) fn private_settings_is_legacy_ssid_login(contents: &str) -> bool {
    let has_ssid_cookie = contents.lines().any(|line| {
        let entry = line.trim().trim_start_matches("- ").trim();
        entry
            .strip_prefix("name:")
            .is_some_and(|name| unquote_yaml_scalar(name) == "ssid")
    });

    has_ssid_cookie
        && contents
            .lines()
            .any(|line| line.trim_end() == "riot-login:")
        && private_settings_refresh_token(contents).is_none()
}

/// Replaces the remembered refresh token without rewriting unrelated YAML.
pub(super) fn update_private_settings_refresh_token(
    contents: &str,
    refresh_token: &str,
) -> Option<String> {
    let mut lines = contents.split('\n').collect::<Vec<_>>();
    let index = find_yaml_path_line(&lines, &PSL_REFRESH_TOKEN_PATH)?;
    let line = lines[index];
    let indent = &line[..line.len() - line.trim_start().len()];
    let carriage_return = if line.ends_with('\r') { "\r" } else { "" };
    let updated = format!("{indent}refresh_token: \"{refresh_token}\"{carriage_return}");
    lines[index] = &updated;

    Some(lines.join("\n"))
}

fn find_yaml_path_line(lines: &[&str], path: &[&str]) -> Option<usize> {
    let (leaf, parent_path) = path.split_last()?;
    let mut parents: Vec<(usize, &str)> = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let indent = line.len() - line.trim_start().len();
        while parents
            .last()
            .is_some_and(|(parent_indent, _)| *parent_indent >= indent)
        {
            parents.pop();
        }

        if trimmed.starts_with('-') {
            continue;
        }

        let Some((key, value)) = trimmed.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches('"');

        if value.trim().is_empty() {
            parents.push((indent, key));
        } else if key == *leaf
            && parents.len() == parent_path.len()
            && parents
                .iter()
                .zip(parent_path)
                .all(|((_, parent), expected)| parent == expected)
        {
            return Some(index);
        }
    }

    None
}

fn unquote_yaml_scalar(value: &str) -> String {
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
