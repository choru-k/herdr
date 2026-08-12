use std::io;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::command::{hook_command, legacy_bash_hook_command, trusted_hook_command};
#[cfg(windows)]
use super::file_ops::legacy_bash_hook_path;
use super::{
    HERMES_PLUGIN_INSTALL_NAME, KIMI_CONFIG_BLOCK_BEGIN, KIMI_CONFIG_BLOCK_END, KIMI_HOOK_EVENTS,
    VIBE_CONFIG_BLOCK_BEGIN, VIBE_CONFIG_BLOCK_END, VIBE_HOOK_NAME,
};

pub(crate) fn ensure_hooks_object<'a>(
    settings: &'a mut Value,
    settings_path: &Path,
    root_description: &str,
    hooks_description: &str,
) -> io::Result<&'a mut Map<String, Value>> {
    let root = settings.as_object_mut().ok_or_else(|| {
        io::Error::other(format!(
            "{root_description} at {} must be a JSON object",
            settings_path.display()
        ))
    })?;

    let hooks = root.entry("hooks").or_insert_with(|| json!({}));
    hooks.as_object_mut().ok_or_else(|| {
        io::Error::other(format!(
            "{hooks_description} at {} must be a JSON object",
            settings_path.display()
        ))
    })
}

pub(crate) fn hooks_object_if_present<'a>(
    settings: &'a mut Value,
    settings_path: &Path,
    root_description: &str,
    hooks_description: &str,
) -> io::Result<Option<&'a mut Map<String, Value>>> {
    let root = settings.as_object_mut().ok_or_else(|| {
        io::Error::other(format!(
            "{root_description} at {} must be a JSON object",
            settings_path.display()
        ))
    })?;

    let Some(hooks) = root.get_mut("hooks") else {
        return Ok(None);
    };

    hooks.as_object_mut().map(Some).ok_or_else(|| {
        io::Error::other(format!(
            "{hooks_description} at {} must be a JSON object",
            settings_path.display()
        ))
    })
}

pub(crate) fn ensure_command_hook(
    hooks: &mut Map<String, Value>,
    event: &str,
    command: String,
    timeout: u64,
    matcher: Option<&str>,
) -> io::Result<()> {
    let entries = hooks
        .entry(event.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| io::Error::other(format!("hook entries for {event} must be an array")))?;

    let already_installed = entries.iter().any(|entry| {
        entry
            .get("hooks")
            .and_then(Value::as_array)
            .is_some_and(|hook_entries| {
                hook_entries.iter().any(|hook| {
                    hook.get("type").and_then(Value::as_str) == Some("command")
                        && hook.get("command").and_then(Value::as_str) == Some(command.as_str())
                })
            })
    });
    if already_installed {
        return Ok(());
    }

    let mut entry = Map::new();
    if let Some(matcher) = matcher {
        entry.insert("matcher".to_string(), Value::String(matcher.to_string()));
    }
    entry.insert(
        "hooks".to_string(),
        json!([
            {
                "type": "command",
                "command": command,
                "timeout": timeout,
            }
        ]),
    );

    entries.push(Value::Object(entry));
    Ok(())
}

// Claude and Codex use nested hook groups:
//   { "matcher": "...", "hooks": [{ "type": "command", ... }] }
// Copilot uses the flatter settings shape:
//   { "type": "command", "matcher": "...", "bash": "...", ... }
// Keep the helpers separate so install/uninstall preserves unrelated hooks in
// each agent's native format instead of normalizing user configuration.
pub(crate) fn ensure_flat_command_hook(
    hooks: &mut Map<String, Value>,
    event: &str,
    command: String,
    timeout_ms: u64,
) -> io::Result<()> {
    let entries = hooks
        .entry(event.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| io::Error::other(format!("hook entries for {event} must be an array")))?;

    if entries.iter().any(|entry| {
        entry.get("type").and_then(Value::as_str) == Some("command")
            && entry.get("command").and_then(Value::as_str) == Some(command.as_str())
    }) {
        return Ok(());
    }

    entries.push(json!({
        "type": "command",
        "command": command,
        "timeout": timeout_ms,
        "description": "Report MastraCode agent state to Herdr",
    }));
    Ok(())
}

pub(crate) fn ensure_direct_command_hook(
    hooks: &mut Map<String, Value>,
    event: &str,
    command: String,
    timeout_sec: u64,
    matcher: Option<&str>,
) -> io::Result<()> {
    let entries = hooks
        .entry(event.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| io::Error::other(format!("hook entries for {event} must be an array")))?;

    let command_field = direct_command_field();
    if let Some(entry) = entries.iter_mut().find(|entry| {
        entry.get("type").and_then(Value::as_str) == Some("command")
            && is_matching_direct_command_entry(entry, command.as_str())
    }) {
        let Some(entry_object) = entry.as_object_mut() else {
            return Ok(());
        };
        entry_object.remove("command");
        entry_object.remove("bash");
        entry_object.remove("powershell");
        entry_object.insert(command_field.to_string(), Value::String(command.clone()));
        entry_object.insert("timeoutSec".to_string(), Value::Number(timeout_sec.into()));
        match matcher {
            Some(matcher) => {
                entry_object.insert("matcher".to_string(), Value::String(matcher.to_string()));
            }
            None => {
                entry_object.remove("matcher");
            }
        }
        return Ok(());
    }

    let mut entry = Map::new();
    entry.insert("type".to_string(), Value::String("command".to_string()));
    if let Some(matcher) = matcher {
        entry.insert("matcher".to_string(), Value::String(matcher.to_string()));
    }
    entry.insert(command_field.to_string(), Value::String(command));
    entry.insert("timeoutSec".to_string(), Value::Number(timeout_sec.into()));
    entries.push(Value::Object(entry));
    Ok(())
}

pub(crate) fn direct_command_field() -> &'static str {
    if cfg!(windows) {
        "powershell"
    } else {
        "bash"
    }
}

pub(crate) fn is_matching_direct_command_entry(entry: &Value, command: &str) -> bool {
    entry.get("command").and_then(Value::as_str) == Some(command)
        || entry.get("bash").and_then(Value::as_str) == Some(command)
        || entry.get("powershell").and_then(Value::as_str) == Some(command)
}

pub(crate) fn remove_command_hook(
    hooks: &mut Map<String, Value>,
    event: &str,
    command: &str,
) -> io::Result<bool> {
    let Some(entries_value) = hooks.get_mut(event) else {
        return Ok(false);
    };

    let entries = entries_value
        .as_array_mut()
        .ok_or_else(|| io::Error::other(format!("hook entries for {event} must be an array")))?;

    let mut removed = false;
    entries.retain_mut(|entry| {
        let Some(entry_object) = entry.as_object_mut() else {
            return true;
        };
        let Some(hook_entries) = entry_object.get_mut("hooks") else {
            return true;
        };
        let Some(hook_entries) = hook_entries.as_array_mut() else {
            return true;
        };

        let before = hook_entries.len();
        hook_entries.retain(|hook| !is_matching_command_hook(hook, command));
        if hook_entries.len() != before {
            removed = true;
        }

        !hook_entries.is_empty()
    });

    let remove_event = entries.is_empty();
    if remove_event {
        hooks.remove(event);
    }

    Ok(removed)
}

pub(crate) fn remove_flat_command_hook(
    hooks: &mut Map<String, Value>,
    event: &str,
    command: &str,
) -> io::Result<bool> {
    let Some(entries_value) = hooks.get_mut(event) else {
        return Ok(false);
    };

    let entries = entries_value
        .as_array_mut()
        .ok_or_else(|| io::Error::other(format!("hook entries for {event} must be an array")))?;

    let before = entries.len();
    entries.retain(|entry| {
        !(entry.get("type").and_then(Value::as_str) == Some("command")
            && entry.get("command").and_then(Value::as_str) == Some(command))
    });
    let removed = entries.len() != before;
    if entries.is_empty() {
        hooks.remove(event);
    }
    Ok(removed)
}

pub(crate) fn remove_direct_command_hook(
    hooks: &mut Map<String, Value>,
    event: &str,
    command: &str,
) -> io::Result<bool> {
    let Some(entries_value) = hooks.get_mut(event) else {
        return Ok(false);
    };

    let entries = entries_value
        .as_array_mut()
        .ok_or_else(|| io::Error::other(format!("hook entries for {event} must be an array")))?;

    let before = entries.len();
    entries.retain(|entry| {
        !(entry.get("type").and_then(Value::as_str) == Some("command")
            && is_matching_direct_command_entry(entry, command))
    });
    let removed = entries.len() != before;
    if entries.is_empty() {
        hooks.remove(event);
    }
    Ok(removed)
}

// Cursor hooks.json uses the minimal shape `{ "command": "..." }` documented at
// https://cursor.com/docs/hooks. Keep this separate from the nested codex and
// flat copilot helpers so install/uninstall does not rewrite unrelated hooks.
pub(crate) fn ensure_simple_command_hook(
    hooks: &mut Map<String, Value>,
    event: &str,
    command: String,
) -> io::Result<()> {
    let entries = hooks
        .entry(event.to_string())
        .or_insert_with(|| Value::Array(Vec::new()))
        .as_array_mut()
        .ok_or_else(|| io::Error::other(format!("hook entries for {event} must be an array")))?;

    if entries
        .iter()
        .any(|entry| entry.get("command").and_then(Value::as_str) == Some(command.as_str()))
    {
        return Ok(());
    }

    entries.push(json!({ "command": command }));
    Ok(())
}

pub(crate) fn remove_simple_command_hook(
    hooks: &mut Map<String, Value>,
    event: &str,
    command: &str,
) -> io::Result<bool> {
    let Some(entries_value) = hooks.get_mut(event) else {
        return Ok(false);
    };

    let entries = entries_value
        .as_array_mut()
        .ok_or_else(|| io::Error::other(format!("hook entries for {event} must be an array")))?;

    let before = entries.len();
    entries.retain(|entry| entry.get("command").and_then(Value::as_str) != Some(command));
    let removed = entries.len() != before;
    if entries.is_empty() {
        hooks.remove(event);
    }
    Ok(removed)
}

pub(crate) fn remove_hook_commands(
    hooks: &mut Map<String, Value>,
    event: &str,
    hook_path: &Path,
    action: Option<&str>,
) -> io::Result<bool> {
    let mut removed = false;
    for command in hook_command_variants(hook_path, action) {
        removed |= remove_command_hook(hooks, event, &command)?;
    }
    Ok(removed)
}

pub(crate) fn remove_direct_hook_commands(
    hooks: &mut Map<String, Value>,
    event: &str,
    hook_path: &Path,
    action: Option<&str>,
) -> io::Result<bool> {
    let mut removed = false;
    for command in hook_command_variants(hook_path, action) {
        removed |= remove_direct_command_hook(hooks, event, &command)?;
    }
    Ok(removed)
}

pub(crate) fn hook_command_variants(hook_path: &Path, action: Option<&str>) -> Vec<String> {
    let mut commands = vec![hook_command(hook_path, action)];
    push_unique_command(&mut commands, legacy_bash_hook_command(hook_path, action));

    #[cfg(windows)]
    {
        push_unique_command(
            &mut commands,
            legacy_bash_hook_command(&legacy_bash_hook_path(hook_path), action),
        );
    }

    commands
}

pub(crate) fn push_unique_command(commands: &mut Vec<String>, command: String) {
    if !commands.iter().any(|existing| existing == &command) {
        commands.push(command);
    }
}

pub(crate) fn is_matching_command_hook(hook: &Value, command: &str) -> bool {
    hook.get("type").and_then(Value::as_str) == Some("command")
        && hook.get("command").and_then(Value::as_str) == Some(command)
}

pub(crate) fn ensure_hermes_plugin_enabled(content: &str) -> String {
    update_hermes_enabled_plugin(content, true)
}

pub(crate) fn remove_hermes_plugin_enabled(content: &str) -> String {
    update_hermes_enabled_plugin(content, false)
}

pub(crate) fn update_hermes_enabled_plugin(content: &str, enabled: bool) -> String {
    let trailing_newline = content.ends_with('\n');
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let Some(plugins_index) = top_level_yaml_key_index(&lines, "plugins") else {
        if !enabled {
            return content.to_string();
        }
        let mut result = content.trim_end_matches('\n').to_string();
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str("plugins:\n  enabled:\n    - herdr-agent-state\n");
        return result;
    };

    let plugins_end =
        next_top_level_yaml_key_index(&lines, plugins_index + 1).unwrap_or(lines.len());
    let plugins_inline_items = yaml_key_value_at_indent(&lines[plugins_index], 0, "plugins")
        .and_then(yaml_flow_sequence_items);
    let enabled_index = lines[plugins_index + 1..plugins_end]
        .iter()
        .position(|line| yaml_key_at_indent(line, 2) == Some("enabled"))
        .map(|offset| plugins_index + 1 + offset);
    let flat_list_start = lines[plugins_index + 1..plugins_end]
        .iter()
        .position(|line| yaml_list_item_value_at_indent(line, 2).is_some())
        .map(|offset| plugins_index + 1 + offset);

    if let Some(enabled_index) = enabled_index {
        let line = lines[enabled_index].trim();
        if line == "enabled: []" || line == "enabled: [] # herdr" {
            if enabled {
                lines[enabled_index] = "  enabled:".to_string();
                lines.insert(enabled_index + 1, "    - herdr-agent-state".to_string());
            }
            return join_yaml_lines(lines, trailing_newline);
        }

        let list_start = enabled_index + 1;
        let list_end = lines[list_start..plugins_end]
            .iter()
            .position(|line| {
                yaml_indent(line).is_some_and(|indent| indent <= 2) && yaml_key_name(line).is_some()
            })
            .map(|offset| list_start + offset)
            .unwrap_or(plugins_end);
        let existing_item_index = lines[list_start..list_end]
            .iter()
            .position(|line| yaml_list_item_matches(line, HERMES_PLUGIN_INSTALL_NAME))
            .map(|offset| list_start + offset);

        match (enabled, existing_item_index) {
            (true, Some(_)) | (false, None) => return content.to_string(),
            (true, None) => lines.insert(list_start, "    - herdr-agent-state".to_string()),
            (false, Some(index)) => {
                lines.remove(index);
            }
        }
        return join_yaml_lines(lines, trailing_newline);
    }

    if let Some(mut items) = plugins_inline_items {
        let existing_item_index = items
            .iter()
            .position(|item| item == HERMES_PLUGIN_INSTALL_NAME);

        match (enabled, existing_item_index) {
            (true, Some(_)) | (false, None) => return content.to_string(),
            (true, None) => items.insert(0, HERMES_PLUGIN_INSTALL_NAME.to_string()),
            (false, Some(index)) => {
                items.remove(index);
            }
        }

        let replacement = hermes_flat_plugin_lines(&items);
        lines.splice(plugins_index..plugins_end, replacement);
        return join_yaml_lines(lines, trailing_newline);
    }

    if let Some(flat_list_start) = flat_list_start {
        let existing_item_index = lines[plugins_index + 1..plugins_end]
            .iter()
            .position(|line| yaml_list_item_matches_at_indent(line, 2, HERMES_PLUGIN_INSTALL_NAME))
            .map(|offset| plugins_index + 1 + offset);

        match (enabled, existing_item_index) {
            (true, Some(_)) | (false, None) => return content.to_string(),
            (true, None) => lines.insert(flat_list_start, "  - herdr-agent-state".to_string()),
            (false, Some(index)) => {
                lines.remove(index);
            }
        }
        return join_yaml_lines(lines, trailing_newline);
    }

    if enabled {
        lines.insert(plugins_index + 1, "  enabled:".to_string());
        lines.insert(plugins_index + 2, "    - herdr-agent-state".to_string());
        return join_yaml_lines(lines, trailing_newline);
    }

    content.to_string()
}

pub(crate) fn hermes_flat_plugin_lines(items: &[String]) -> Vec<String> {
    if items.is_empty() {
        return vec!["plugins: []".to_string()];
    }

    let mut lines = vec!["plugins:".to_string()];
    lines.extend(items.iter().map(|item| format!("  - {item}")));
    lines
}

pub(crate) fn top_level_yaml_key_index(lines: &[String], key: &str) -> Option<usize> {
    lines
        .iter()
        .position(|line| yaml_key_at_indent(line, 0) == Some(key))
}

pub(crate) fn next_top_level_yaml_key_index(lines: &[String], start: usize) -> Option<usize> {
    lines[start..]
        .iter()
        .position(|line| yaml_indent(line) == Some(0) && yaml_key_name(line).is_some())
        .map(|offset| start + offset)
}

pub(crate) fn yaml_key_at_indent(line: &str, indent: usize) -> Option<&str> {
    if yaml_indent(line)? != indent {
        return None;
    }
    yaml_key_name(line)
}

pub(crate) fn yaml_key_value_at_indent<'a>(
    line: &'a str,
    indent: usize,
    key: &str,
) -> Option<&'a str> {
    if yaml_indent(line)? != indent {
        return None;
    }
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
        return None;
    }
    let (line_key, value) = trimmed.split_once(':')?;
    (line_key.trim() == key).then_some(value.trim())
}

pub(crate) fn yaml_key_name(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('-') {
        return None;
    }
    let (key, _) = trimmed.split_once(':')?;
    let key = key.trim();
    (!key.is_empty()).then_some(key)
}

pub(crate) fn yaml_indent(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    Some(line.len() - trimmed.len())
}

pub(crate) fn yaml_list_item_value(line: &str) -> Option<&str> {
    line.trim().strip_prefix("- ").map(str::trim)
}

pub(crate) fn yaml_list_item_matches(line: &str, value: &str) -> bool {
    yaml_list_item_value(line).is_some_and(|item| yaml_scalar_value(item) == value)
}

pub(crate) fn yaml_list_item_value_at_indent(line: &str, indent: usize) -> Option<&str> {
    if yaml_indent(line)? != indent {
        return None;
    }
    yaml_list_item_value(line)
}

pub(crate) fn yaml_list_item_matches_at_indent(line: &str, indent: usize, value: &str) -> bool {
    yaml_list_item_value_at_indent(line, indent)
        .is_some_and(|item| yaml_scalar_value(item) == value)
}

pub(crate) fn yaml_flow_sequence_items(value: &str) -> Option<Vec<String>> {
    let value = strip_yaml_inline_comment(value).trim();
    let inner = value.strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.is_empty() {
        return Some(Vec::new());
    }

    let mut items = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;

    for ch in inner.chars() {
        if let Some(quote_char) = quote {
            current.push(ch);
            if quote_char == '"' && ch == '\\' && !escaped {
                escaped = true;
                continue;
            }
            if ch == quote_char && !escaped {
                quote = None;
            }
            escaped = false;
            continue;
        }

        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                current.push(ch);
            }
            ',' => {
                items.push(yaml_scalar_value(&current));
                current.clear();
            }
            _ => current.push(ch),
        }
    }

    if quote.is_some() {
        return None;
    }

    items.push(yaml_scalar_value(&current));
    Some(items)
}

pub(crate) fn yaml_scalar_value(value: &str) -> String {
    let value = strip_yaml_inline_comment(value).trim();
    if value.len() >= 2 {
        let bytes = value.as_bytes();
        let quoted = (bytes[0] == b'"' && bytes[value.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[value.len() - 1] == b'\'');
        if quoted {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

pub(crate) fn strip_yaml_inline_comment(value: &str) -> &str {
    let mut quote = None;
    let mut escaped = false;

    for (index, ch) in value.char_indices() {
        if let Some(quote_char) = quote {
            if quote_char == '"' && ch == '\\' && !escaped {
                escaped = true;
                continue;
            }
            if ch == quote_char && !escaped {
                quote = None;
            }
            escaped = false;
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '#' if index == 0 || value[..index].ends_with(char::is_whitespace) => {
                return value[..index].trim_end();
            }
            _ => {}
        }
    }

    value
}

pub(crate) fn join_yaml_lines(lines: Vec<String>, trailing_newline: bool) -> String {
    let mut result = lines.join("\n");
    if trailing_newline || result.is_empty() {
        result.push('\n');
    }
    result
}

pub(crate) fn build_codex_config_with_hooks(content: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let trailing_newline = content.ends_with('\n');
    let mut in_top_level_features = false;
    let mut features_header_index = None;
    let mut hooks_index = None;
    let mut deprecated_hooks_indexes = Vec::new();

    for (index, line) in lines.iter().enumerate() {
        if let Some(header) = toml_table_header(line) {
            in_top_level_features = header == "[features]";
            if in_top_level_features && features_header_index.is_none() {
                features_header_index = Some(index);
            }
            continue;
        }

        if !in_top_level_features {
            continue;
        }

        if is_toml_key(line, "codex_hooks") {
            deprecated_hooks_indexes.push(index);
        } else if is_toml_key(line, "hooks") {
            hooks_index = Some(index);
        }
    }

    if let Some(index) = hooks_index {
        lines[index] = "hooks = true".to_string();
    }

    for index in deprecated_hooks_indexes.into_iter().rev() {
        lines.remove(index);
    }

    if hooks_index.is_none() {
        if let Some(index) = features_header_index {
            lines.insert(index + 1, "hooks = true".to_string());
            return join_toml_lines(lines, trailing_newline);
        }

        let mut result = content.trim_end_matches('\n').to_string();
        if !result.is_empty() {
            result.push('\n');
            result.push('\n');
        }
        result.push_str("[features]\nhooks = true\n");
        return result;
    }

    join_toml_lines(lines, trailing_newline)
}

pub(crate) fn build_kimi_config_with_hooks(content: &str, hook_path: &Path) -> String {
    let mut result = remove_kimi_config_block(content)
        .trim_end_matches('\n')
        .to_string();
    if !result.is_empty() {
        result.push('\n');
        result.push('\n');
    }

    result.push_str(KIMI_CONFIG_BLOCK_BEGIN);
    result.push('\n');
    for (event, matcher, action) in KIMI_HOOK_EVENTS {
        result.push_str(&kimi_hook_table(event, matcher, hook_path, action));
    }
    result.push_str(KIMI_CONFIG_BLOCK_END);
    result.push('\n');
    result
}

pub(crate) fn kimi_hook_table(
    event: &str,
    matcher: Option<&str>,
    hook_path: &Path,
    action: &str,
) -> String {
    let command = hook_command(hook_path, Some(action));
    let matcher = matcher
        .map(|matcher| format!("matcher = {}\n", toml_basic_string(matcher)))
        .unwrap_or_default();
    format!(
        "[[hooks]]\nevent = {}\n{matcher}command = {}\ntimeout = 10\n\n",
        toml_basic_string(event),
        toml_basic_string(&command)
    )
}

pub(crate) fn remove_kimi_config_block(content: &str) -> String {
    let trailing_newline = content.ends_with('\n');
    let mut lines = Vec::new();
    let mut in_block = false;
    let mut removed_block = false;

    for line in content.lines() {
        if line.trim() == KIMI_CONFIG_BLOCK_BEGIN {
            in_block = true;
            removed_block = true;
            continue;
        }
        if in_block {
            if line.trim() == KIMI_CONFIG_BLOCK_END {
                in_block = false;
            }
            continue;
        }
        lines.push(line.to_string());
    }

    if !removed_block {
        return content.to_string();
    }

    let mut result = join_toml_lines(lines, trailing_newline);
    while result.ends_with("\n\n") {
        result.pop();
    }
    if result == "\n" {
        String::new()
    } else {
        result
    }
}

pub(crate) fn build_vibe_hooks_config_with_hook(
    content: &str,
    hook_path: &Path,
) -> io::Result<String> {
    validate_vibe_hooks_toml(content)?;
    let unmanaged = remove_vibe_hooks_config_block(content)?;

    let mut result = unmanaged.trim_end_matches('\n').to_string();
    if !result.is_empty() {
        result.push('\n');
        result.push('\n');
    }
    result.push_str(VIBE_CONFIG_BLOCK_BEGIN);
    result.push('\n');
    result.push_str("[[hooks]]\n");
    result.push_str(&format!("name = {}\n", toml_basic_string(VIBE_HOOK_NAME)));
    result.push_str("type = \"post_agent\"\n");
    result.push_str(&format!(
        "command = {}\n",
        toml_basic_string(&trusted_hook_command(hook_path, None)?)
    ));
    result.push_str("timeout = 10.0\n");
    result.push_str(VIBE_CONFIG_BLOCK_END);
    result.push('\n');
    validate_vibe_hooks_toml(&result)?;
    Ok(result)
}

pub(crate) fn remove_vibe_hooks_config_block(content: &str) -> io::Result<String> {
    validate_vibe_hooks_toml(content)?;
    let lines: Vec<&str> = content.lines().collect();
    let unmanaged = if let Some((start, end)) = vibe_marker_block_range(content, &lines)? {
        let trailing_newline = content.ends_with('\n');
        let kept = lines
            .iter()
            .enumerate()
            .filter(|(index, _)| *index < start || *index > end)
            .map(|(_, line)| (*line).to_string())
            .collect();
        let mut result = join_toml_lines(kept, trailing_newline);
        while result.ends_with("\n\n") {
            result.pop();
        }
        if result == "\n" {
            String::new()
        } else {
            result
        }
    } else {
        content.to_string()
    };
    let unmanaged_config = parse_vibe_hooks_toml(&unmanaged)?;
    if vibe_named_hook_count(&unmanaged_config) != 0 {
        return Err(io::Error::other(format!(
            "vibe hooks config already contains an unowned hook named {VIBE_HOOK_NAME}"
        )));
    }
    Ok(unmanaged)
}

pub(crate) fn vibe_managed_hooks_config_block(content: &str) -> io::Result<Option<String>> {
    validate_vibe_hooks_toml(content)?;
    let lines: Vec<&str> = content.lines().collect();
    let Some((start, end)) = vibe_marker_block_range(content, &lines)? else {
        return Ok(None);
    };
    Ok(Some(lines[start + 1..end].join("\n")))
}

fn validate_vibe_hooks_toml(content: &str) -> io::Result<()> {
    parse_vibe_hooks_toml(content).map(|_| ())
}

fn parse_vibe_hooks_toml(content: &str) -> io::Result<toml::Value> {
    toml::from_str(content)
        .map_err(|err| io::Error::other(format!("failed to parse vibe hooks config: {err}")))
}

fn vibe_named_hook_count(config: &toml::Value) -> usize {
    config
        .get("hooks")
        .and_then(toml::Value::as_array)
        .map(|hooks| {
            hooks
                .iter()
                .filter(|hook| {
                    hook.get("name").and_then(toml::Value::as_str) == Some(VIBE_HOOK_NAME)
                })
                .count()
        })
        .unwrap_or(0)
}

fn vibe_marker_block_range(content: &str, lines: &[&str]) -> io::Result<Option<(usize, usize)>> {
    let mut open = None;
    let mut block = None;
    for (index, line) in lines.iter().enumerate() {
        let marker = line.trim();
        if marker != VIBE_CONFIG_BLOCK_BEGIN && marker != VIBE_CONFIG_BLOCK_END {
            continue;
        }
        if !vibe_marker_line_is_comment(content, index) {
            continue;
        }
        if marker == VIBE_CONFIG_BLOCK_BEGIN {
            if open.is_some() {
                return Err(io::Error::other(
                    "vibe hooks config contains a nested herdr marker block",
                ));
            }
            if block.is_some() {
                return Err(io::Error::other(
                    "vibe hooks config contains multiple herdr marker blocks",
                ));
            }
            open = Some(index);
        } else {
            let Some(start) = open.take() else {
                return Err(io::Error::other(
                    "vibe hooks config contains an unmatched herdr marker",
                ));
            };
            block = Some((start, index));
        }
    }
    if open.is_some() {
        return Err(io::Error::other(
            "vibe hooks config contains an unterminated herdr marker block",
        ));
    }
    Ok(block)
}

fn vibe_marker_line_is_comment(content: &str, target_line: usize) -> bool {
    // The config has already parsed successfully. Replacing a comment with an
    // invalid TOML token makes parsing fail, while the same replacement inside
    // a multiline string remains valid string content.
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();
    let indentation = lines[target_line]
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .collect::<String>();
    lines[target_line] = format!("{indentation}@");
    parse_vibe_hooks_toml(&lines.join("\n")).is_err()
}

pub(crate) fn toml_basic_string(value: &str) -> String {
    let mut result = String::with_capacity(value.len() + 2);
    result.push('"');
    for ch in value.chars() {
        match ch {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\u{08}' => result.push_str("\\b"),
            '\t' => result.push_str("\\t"),
            '\n' => result.push_str("\\n"),
            '\u{0c}' => result.push_str("\\f"),
            '\r' => result.push_str("\\r"),
            ch if ch <= '\u{1f}' || ch == '\u{7f}' => {
                result.push_str(&format!("\\u{:04X}", ch as u32));
            }
            ch => result.push(ch),
        }
    }
    result.push('"');
    result
}

pub(crate) fn join_toml_lines(lines: Vec<String>, trailing_newline: bool) -> String {
    let mut result = lines.join("\n");
    if trailing_newline || result.is_empty() {
        result.push('\n');
    }
    result
}

pub(crate) fn toml_table_header(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') || !trimmed.starts_with('[') {
        return None;
    }

    let header_end = if trimmed.starts_with("[[") {
        trimmed.find("]]").map(|index| index + 2)?
    } else {
        trimmed.find(']').map(|index| index + 1)?
    };
    let header = &trimmed[..header_end];
    let rest = trimmed[header_end..].trim_start();
    if !rest.is_empty() && !rest.starts_with('#') {
        return None;
    }

    Some(header)
}

pub(crate) fn is_toml_key(line: &str, key: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.starts_with('#') || !trimmed.starts_with(key) {
        return false;
    }

    trimmed[key.len()..].trim_start().starts_with('=')
}
