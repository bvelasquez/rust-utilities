//! Friendly transcript lines for agent tool calls — bash commands, edits,
//! writes, reads, searches — so the TUI log reads like an agentic CLI.

use serde_json::Value;

/// One-line (or few-line) summary shown when a tool starts.
pub fn format_tool_start(name: &str, args: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    match name {
        "bash" => {
            let cmd = str_field(args, &["command", "cmd"]).unwrap_or_else(|| "?".into());
            lines.push(format!("⚙ bash $ {}", oneline(&cmd, 160)));
        }
        "edit" => {
            let path = path_field(args).unwrap_or_else(|| "?".into());
            let edits = args.get("edits").and_then(|e| e.as_array());
            let n = edits.map(|a| a.len()).unwrap_or_else(|| {
                if args.get("oldText").is_some() || args.get("old_str").is_some() {
                    1
                } else {
                    0
                }
            });
            lines.push(format!(
                "⚙ edit {path} · {} hunk{}",
                n,
                if n == 1 { "" } else { "s" }
            ));
            push_edit_previews(&mut lines, args, 3);
        }
        "write" => {
            let path = path_field(args).unwrap_or_else(|| "?".into());
            let content = str_field(args, &["content"]).unwrap_or_default();
            let nlines = content.lines().count();
            let nbytes = content.len();
            lines.push(format!("⚙ write {path} · {nlines} lines ({nbytes} B)"));
            for preview in content.lines().take(2) {
                lines.push(format!("  │ {}", oneline(preview, 100)));
            }
            if nlines > 2 {
                lines.push(format!("  │ … {} more lines", nlines - 2));
            }
        }
        "read" => {
            let path = path_field(args).unwrap_or_else(|| "?".into());
            let mut extra = String::new();
            if let Some(o) = args.get("offset").and_then(|v| v.as_u64()) {
                extra.push_str(&format!(" offset={o}"));
            }
            if let Some(l) = args.get("limit").and_then(|v| v.as_u64()) {
                extra.push_str(&format!(" limit={l}"));
            }
            lines.push(format!("⚙ read {path}{extra}"));
        }
        "grep" => {
            let pat = str_field(args, &["pattern", "query"]).unwrap_or_else(|| "?".into());
            let path = path_field(args).unwrap_or_default();
            let glob = str_field(args, &["glob"]).unwrap_or_default();
            let mut bits = vec![format!("⚙ grep {}", oneline(&pat, 80))];
            if !path.is_empty() {
                bits.push(format!("in {path}"));
            }
            if !glob.is_empty() {
                bits.push(format!("glob={glob}"));
            }
            lines.push(bits.join(" · "));
        }
        "find" => {
            let pat = str_field(args, &["pattern", "glob"]).unwrap_or_else(|| "?".into());
            let path = path_field(args).unwrap_or_default();
            if path.is_empty() {
                lines.push(format!("⚙ find {}", oneline(&pat, 100)));
            } else {
                lines.push(format!("⚙ find {} in {path}", oneline(&pat, 80)));
            }
        }
        "ls" => {
            let path = path_field(args).unwrap_or_else(|| ".".into());
            lines.push(format!("⚙ ls {path}"));
        }
        other => {
            let summary = generic_args_summary(args);
            if summary.is_empty() {
                lines.push(format!("⚙ {other}"));
            } else {
                lines.push(format!("⚙ {other} · {summary}"));
            }
        }
    }
    lines
}

/// Short label for the agent roster `current_tool` column.
pub fn tool_short_label(name: &str, args: &Value) -> String {
    match name {
        "bash" => {
            let cmd = str_field(args, &["command", "cmd"]).unwrap_or_else(|| "bash".into());
            format!("bash: {}", oneline(&cmd, 36))
        }
        "edit" => format!("edit: {}", path_field(args).unwrap_or_else(|| "…".into())),
        "write" => format!("write: {}", path_field(args).unwrap_or_else(|| "…".into())),
        "read" => format!("read: {}", path_field(args).unwrap_or_else(|| "…".into())),
        "grep" => {
            let pat = str_field(args, &["pattern"]).unwrap_or_default();
            format!("grep: {}", oneline(&pat, 28))
        }
        "find" => {
            let pat = str_field(args, &["pattern"]).unwrap_or_default();
            format!("find: {}", oneline(&pat, 28))
        }
        "ls" => format!("ls: {}", path_field(args).unwrap_or_else(|| ".".into())),
        other => other.to_string(),
    }
}

/// Result / completion lines after a tool finishes.
pub fn format_tool_end(name: &str, is_error: bool, result: &Value) -> Vec<String> {
    let mut lines = Vec::new();
    let status = if is_error { "✗" } else { "✓" };

    if is_error {
        let err = result_text(result);
        lines.push(format!(
            "  {status} {name} failed{}",
            if err.is_empty() {
                String::new()
            } else {
                format!(": {}", oneline(&err, 120))
            }
        ));
        return lines;
    }

    match name {
        "bash" => {
            let text = result_text(result);
            let nlines = text.lines().filter(|l| !l.trim().is_empty()).count();
            lines.push(format!("  {status} bash done · {nlines} output lines"));
            for (i, l) in text.lines().filter(|l| !l.trim().is_empty()).take(4).enumerate() {
                let _ = i;
                lines.push(format!("  │ {}", oneline(l, 110)));
            }
            if nlines > 4 {
                lines.push(format!("  │ … {} more", nlines - 4));
            }
        }
        "edit" => {
            if let Some(diff) = result
                .get("details")
                .and_then(|d| d.get("diff"))
                .and_then(|d| d.as_str())
            {
                let (plus, minus) = count_diff_stats(diff);
                lines.push(format!("  {status} edited · +{plus} −{minus}"));
                for l in diff_preview_lines(diff, 6) {
                    lines.push(format!("  {l}"));
                }
            } else if let Some(patch) = result
                .get("details")
                .and_then(|d| d.get("patch"))
                .and_then(|d| d.as_str())
            {
                let (plus, minus) = count_diff_stats(patch);
                lines.push(format!("  {status} edited · +{plus} −{minus}"));
                for l in diff_preview_lines(patch, 6) {
                    lines.push(format!("  {l}"));
                }
            } else {
                lines.push(format!("  {status} edited"));
            }
        }
        "write" => {
            lines.push(format!("  {status} wrote file"));
        }
        "read" => {
            let text = result_text(result);
            let n = text.lines().count();
            lines.push(format!("  {status} read · {n} lines"));
        }
        "grep" | "find" | "ls" => {
            let text = result_text(result);
            let n = text.lines().filter(|l| !l.trim().is_empty()).count();
            lines.push(format!("  {status} {name} · {n} hits"));
            for l in text.lines().filter(|l| !l.trim().is_empty()).take(3) {
                lines.push(format!("  │ {}", oneline(l, 100)));
            }
            if n > 3 {
                lines.push(format!("  │ … {} more", n - 3));
            }
        }
        other => {
            let text = result_text(result);
            if text.is_empty() {
                lines.push(format!("  {status} {other} done"));
            } else {
                lines.push(format!(
                    "  {status} {other}: {}",
                    oneline(&text, 100)
                ));
            }
        }
    }
    lines
}

fn push_edit_previews(lines: &mut Vec<String>, args: &Value, max_hunks: usize) {
    let edits = if let Some(arr) = args.get("edits").and_then(|e| e.as_array()) {
        arr.iter().collect::<Vec<_>>()
    } else if args.get("oldText").is_some() || args.get("old_str").is_some() {
        vec![args]
    } else {
        Vec::new()
    };
    let total = edits.len();
    for (i, edit) in edits.into_iter().take(max_hunks).enumerate() {
        let old = str_field(edit, &["oldText", "old_str", "old_string"]).unwrap_or_default();
        let new = str_field(edit, &["newText", "new_str", "new_string"]).unwrap_or_default();
        if !old.is_empty() {
            lines.push(format!("  − [{}] {}", i + 1, oneline(old.lines().next().unwrap_or(""), 90)));
        }
        if !new.is_empty() {
            lines.push(format!("  + [{}] {}", i + 1, oneline(new.lines().next().unwrap_or(""), 90)));
        }
    }
    if total > max_hunks {
        lines.push(format!("  · … {} more hunks", total - max_hunks));
    }
}

fn path_field(args: &Value) -> Option<String> {
    str_field(args, &["path", "file_path", "filePath", "filename"])
}

fn str_field(v: &Value, keys: &[&str]) -> Option<String> {
    for k in keys {
        if let Some(s) = v.get(*k).and_then(|x| x.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

fn generic_args_summary(args: &Value) -> String {
    if let Some(obj) = args.as_object() {
        let mut parts = Vec::new();
        for (k, v) in obj.iter().take(4) {
            let val = match v {
                Value::String(s) => oneline(s, 40),
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                Value::Array(a) => format!("[{}]", a.len()),
                Value::Object(_) => "{…}".into(),
                Value::Null => "null".into(),
            };
            parts.push(format!("{k}={val}"));
        }
        return parts.join(" ");
    }
    String::new()
}

fn result_text(result: &Value) -> String {
    if let Some(arr) = result.get("content").and_then(|c| c.as_array()) {
        let mut parts = Vec::new();
        for block in arr {
            if let Some(t) = block.get("text").and_then(|t| t.as_str()) {
                parts.push(t.to_string());
            }
        }
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }
    if let Some(s) = result.get("output").and_then(|o| o.as_str()) {
        return s.to_string();
    }
    if let Some(s) = result.get("text").and_then(|t| t.as_str()) {
        return s.to_string();
    }
    if let Some(s) = result.as_str() {
        return s.to_string();
    }
    String::new()
}

fn count_diff_stats(diff: &str) -> (usize, usize) {
    let mut plus = 0usize;
    let mut minus = 0usize;
    for l in diff.lines() {
        if l.starts_with("+++") || l.starts_with("---") || l.starts_with("@@") {
            continue;
        }
        if l.starts_with('+') {
            plus += 1;
        } else if l.starts_with('-') {
            minus += 1;
        }
    }
    (plus, minus)
}

fn diff_preview_lines(diff: &str, max: usize) -> Vec<String> {
    let mut out = Vec::new();
    for l in diff.lines() {
        if l.starts_with("+++") || l.starts_with("---") || l.starts_with("@@") {
            continue;
        }
        if l.starts_with('+') || l.starts_with('-') {
            let prefix = if l.starts_with('+') { "+" } else { "−" };
            out.push(format!("{prefix} {}", oneline(&l[1..], 100)));
            if out.len() >= max {
                break;
            }
        }
    }
    out
}

fn oneline(s: &str, cap: usize) -> String {
    let flat: String = s
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .collect();
    let flat = flat.trim();
    let t: String = flat.chars().take(cap).collect();
    if flat.chars().count() > cap {
        format!("{t}…")
    } else {
        t.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bash_start_shows_command() {
        let lines = format_tool_start("bash", &json!({"command": "pnpm test"}));
        assert!(lines[0].contains("pnpm test"));
        assert!(lines[0].contains("bash $"));
    }

    #[test]
    fn edit_start_shows_hunks() {
        let lines = format_tool_start(
            "edit",
            &json!({
                "path": "src/foo.rs",
                "edits": [{"oldText": "let a = 1;", "newText": "let a = 2;"}]
            }),
        );
        assert!(lines[0].contains("src/foo.rs"));
        assert!(lines.iter().any(|l| l.contains("let a = 1")));
        assert!(lines.iter().any(|l| l.contains("let a = 2")));
    }

    #[test]
    fn edit_end_shows_diff_stats() {
        let lines = format_tool_end(
            "edit",
            false,
            &json!({
                "details": {"diff": "--- a\n+++ b\n@@\n-old\n+new\n+extra\n"}
            }),
        );
        assert!(lines[0].contains("+2"));
        assert!(lines[0].contains("−1"));
    }

    #[test]
    fn short_label_truncates() {
        let label = tool_short_label(
            "bash",
            &json!({"command": "cargo test --all-features -- --nocapture something long"}),
        );
        assert!(label.starts_with("bash:"));
        assert!(label.len() < 50);
    }
}
