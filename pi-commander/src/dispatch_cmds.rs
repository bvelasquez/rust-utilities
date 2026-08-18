//! Commands available in the watch TUI input line and natural `send`/`dispatch` text.
//! Type `/` or `/help` in the TUI to see the menu; free text is sent as a prompt.

use regex::Regex;
use std::sync::OnceLock;

pub struct DispatchCmd {
    pub name: &'static str,
    pub aliases: &'static [&'static str],
    pub usage: &'static str,
    pub description: &'static str,
    /// When true, the TUI fills in the selected worker when you omit the target.
    pub uses_worker: bool,
}

pub const COMMANDS: &[DispatchCmd] = &[
    DispatchCmd {
        name: "help",
        aliases: &["?", "commands"],
        usage: "/help",
        description: "Show this command list",
        uses_worker: false,
    },
    DispatchCmd {
        name: "dispatch",
        aliases: &["d"],
        usage: "/dispatch <text>",
        description: "Natural route (on/in/for <project>) to least-busy agent",
        uses_worker: false,
    },
    DispatchCmd {
        name: "steer",
        aliases: &[],
        usage: "/steer [worker] <message>",
        description: "Interrupt a running agent with a new direction",
        uses_worker: true,
    },
    DispatchCmd {
        name: "fup",
        aliases: &["follow", "follow_up", "follow-up"],
        usage: "/fup [worker] <message>",
        description: "Queue a follow-up message (does not interrupt)",
        uses_worker: true,
    },
    DispatchCmd {
        name: "abort",
        aliases: &[],
        usage: "/abort [worker]",
        description: "Abort the agent's current run",
        uses_worker: true,
    },
    DispatchCmd {
        name: "model",
        aliases: &[],
        usage: "/model [worker] <provider/id>",
        description: "Switch model (e.g. anthropic/claude-sonnet-4)",
        uses_worker: true,
    },
    DispatchCmd {
        name: "thinking",
        aliases: &["th"],
        usage: "/thinking [worker] <level>",
        description: "Set thinking level: off|minimal|low|medium|high|xhigh|max",
        uses_worker: true,
    },
    DispatchCmd {
        name: "compact",
        aliases: &[],
        usage: "/compact [worker]",
        description: "Compact the agent's context",
        uses_worker: true,
    },
    DispatchCmd {
        name: "bash",
        aliases: &[],
        usage: "/bash [worker] <shell command>",
        description: "Run a shell command in the agent's context",
        uses_worker: true,
    },
    DispatchCmd {
        name: "stop",
        aliases: &[],
        usage: "/stop [worker]",
        description: "Shut down the agent's pi process",
        uses_worker: true,
    },
    DispatchCmd {
        name: "new",
        aliases: &[],
        usage: "/new <project>",
        description: "Spawn another parallel agent for a project",
        uses_worker: false,
    },
    DispatchCmd {
        name: "status",
        aliases: &[],
        usage: "/status",
        description: "Refresh agent status snapshot",
        uses_worker: false,
    },
];

/// Parsed TUI / send command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedCmd {
    Help,
    /// Natural dispatch text (route to least-busy agent of a project).
    Dispatch { text: String },
    /// Prompt selected (or named) worker — daemon picks steer vs followUp by phase.
    Prompt { worker: Option<String>, message: String },
    /// Named verb with resolved worker (if needed) and remainder.
    Verb {
        name: &'static str,
        worker: Option<String>,
        rest: String,
    },
}

fn worker_id_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[A-Za-z0-9._-]+#\d+$").expect("worker id regex"))
}

fn natural_cue_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\b(?:on|in|for|against)\s+[A-Za-z0-9._-]+(?:#\d+)?")
            .expect("natural cue regex")
    })
}

/// True if token looks like `project#0`.
pub fn looks_like_worker_id(token: &str) -> bool {
    worker_id_re().is_match(token)
}

/// True if free text has natural-routing cues (`on|in|for <project>`).
pub fn looks_like_natural_dispatch(text: &str) -> bool {
    natural_cue_re().is_match(text)
}

/// Strip one leading `/` and trim.
pub fn strip_slash(text: &str) -> &str {
    let t = text.trim();
    t.strip_prefix('/').unwrap_or(t).trim()
}

/// Resolve alias → canonical command name, or None if unknown.
pub fn resolve_verb(token: &str) -> Option<&'static DispatchCmd> {
    let lower = token.to_ascii_lowercase();
    COMMANDS.iter().find(|c| {
        c.name == lower || c.aliases.iter().any(|a| a.eq_ignore_ascii_case(&lower))
    })
}

/// User typed `/`, `/help`, `help`, or `?`.
pub fn is_help_command(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t == "?" || t == "/" {
        return true;
    }
    let stripped = strip_slash(t).to_ascii_lowercase();
    matches!(stripped.as_str(), "help" | "?" | "commands")
        || stripped.starts_with("help ")
        || stripped.starts_with("? ")
}

pub fn filter_prefix(input: &str) -> Vec<&'static DispatchCmd> {
    let trimmed = input.trim();
    let query = strip_slash(trimmed).to_ascii_lowercase();
    // Only the verb token for filtering (before first space).
    let query = query.split_whitespace().next().unwrap_or(&query);
    if query.is_empty() {
        return COMMANDS.iter().collect();
    }
    COMMANDS
        .iter()
        .filter(|c| {
            c.name.starts_with(query) || c.aliases.iter().any(|a| a.starts_with(query))
        })
        .collect()
}

/// Parse a TUI command line.
///
/// `default_worker` is the currently selected agent id (if any).
/// `known_projects` helps decide whether a bare token is a project id for `/new`.
pub fn parse_command(
    text: &str,
    default_worker: Option<&str>,
    known_projects: &[String],
) -> ParsedCmd {
    let trimmed = text.trim();
    if is_help_command(trimmed) {
        return ParsedCmd::Help;
    }

    let stripped = strip_slash(trimmed);
    let mut parts = stripped.splitn(2, char::is_whitespace);
    let verb_tok = parts.next().unwrap_or("").trim();
    let after_verb = parts.next().unwrap_or("").trim();

    if let Some(cmd) = resolve_verb(verb_tok) {
        match cmd.name {
            "help" => return ParsedCmd::Help,
            "dispatch" => {
                let text = if after_verb.is_empty() {
                    String::new()
                } else {
                    after_verb.to_string()
                };
                return ParsedCmd::Dispatch { text };
            }
            "status" => {
                return ParsedCmd::Verb {
                    name: "status",
                    worker: None,
                    rest: String::new(),
                };
            }
            "new" => {
                // First token is project id (required).
                let mut bits = after_verb.splitn(2, char::is_whitespace);
                let project = bits.next().unwrap_or("").trim().to_string();
                return ParsedCmd::Verb {
                    name: "new",
                    worker: if project.is_empty() { None } else { Some(project) },
                    rest: String::new(),
                };
            }
            name if cmd.uses_worker => {
                return parse_worker_verb(name, after_verb, default_worker, known_projects);
            }
            name => {
                return ParsedCmd::Verb {
                    name,
                    worker: None,
                    rest: after_verb.to_string(),
                };
            }
        }
    }

    // Free text: natural cue or explicit project route → dispatch; else prompt selected.
    if looks_like_natural_dispatch(trimmed) {
        return ParsedCmd::Dispatch {
            text: trimmed.to_string(),
        };
    }
    if default_worker.is_some() {
        ParsedCmd::Prompt {
            worker: default_worker.map(|s| s.to_string()),
            message: trimmed.to_string(),
        }
    } else {
        ParsedCmd::Dispatch {
            text: trimmed.to_string(),
        }
    }
}

fn parse_worker_verb(
    name: &'static str,
    after_verb: &str,
    default_worker: Option<&str>,
    known_projects: &[String],
) -> ParsedCmd {
    if after_verb.is_empty() {
        return ParsedCmd::Verb {
            name,
            worker: default_worker.map(|s| s.to_string()),
            rest: String::new(),
        };
    }

    let mut bits = after_verb.splitn(2, char::is_whitespace);
    let first = bits.next().unwrap_or("").trim();
    let rest_after_first = bits.next().unwrap_or("").trim();

    if looks_like_worker_id(first)
        || known_projects
            .iter()
            .any(|p| p.eq_ignore_ascii_case(first))
    {
        // Explicit worker / bare project id as target.
        ParsedCmd::Verb {
            name,
            worker: Some(first.to_string()),
            rest: rest_after_first.to_string(),
        }
    } else {
        // First token is part of the message — inject selected worker.
        ParsedCmd::Verb {
            name,
            worker: default_worker.map(|s| s.to_string()),
            rest: after_verb.to_string(),
        }
    }
}

/// Insert completion text for a slash menu pick: `/name `.
pub fn completion_insert(cmd: &DispatchCmd) -> String {
    format!("/{} ", cmd.name)
}

pub fn format_help(default_worker: Option<&str>) -> String {
    let worker = default_worker.unwrap_or("<worker>");
    let mut lines = vec![
        "dispatch commands (TUI input — worker optional when an agent is selected):".to_string(),
        "  free text            → prompt selected agent (auto steer/followUp by phase)".to_string(),
        "  … on <project>       → natural dispatch to least-busy agent".to_string(),
        "  /dispatch <text>     → always natural dispatch".to_string(),
        "  /steer msg           → steer selected (or /steer proj#0 msg)".to_string(),
        String::new(),
    ];
    for c in COMMANDS {
        if c.name == "help" {
            continue;
        }
        let usage = if c.uses_worker {
            c.usage.replace("[worker]", &format!("[{worker}]"))
        } else {
            c.usage.to_string()
        };
        lines.push(format!("  {usage:<40} — {desc}", desc = c.description));
    }
    lines.push(String::new());
    lines.push("Hotkeys: f follow-up · s steer · a abort · n/N activity · 1-4 panes · z dense".into());
    lines.push("CLI: pi-commander steer -a <worker> …, pi-commander fup -a <worker> …".into());
    lines.push("Full manifest: pi-commander capabilities --json".into());
    lines.join("\n")
}

pub fn format_menu_lines(input: &str, max: usize, highlight: usize) -> Vec<String> {
    let matches = filter_prefix(input);
    let mut out: Vec<String> = matches
        .iter()
        .take(max)
        .enumerate()
        .map(|(i, c)| {
            let mark = if i == highlight { "▸" } else { " " };
            format!("{mark} /{} — {}", c.name, c.description)
        })
        .collect();
    if matches.len() > max {
        out.push(format!("  … {} more — type /help", matches.len() - max));
    }
    if out.is_empty() {
        out.push("  (no matching commands — type /help)".into());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projects() -> Vec<String> {
        vec!["simple-workout".into(), "paperdoll".into()]
    }

    #[test]
    fn strips_slash_and_resolves_steer() {
        let p = parse_command("/steer fix it", Some("simple-workout#0"), &projects());
        assert_eq!(
            p,
            ParsedCmd::Verb {
                name: "steer",
                worker: Some("simple-workout#0".into()),
                rest: "fix it".into(),
            }
        );
    }

    #[test]
    fn explicit_worker_wins() {
        let p = parse_command(
            "fup simple-workout#1 then also X",
            Some("simple-workout#0"),
            &projects(),
        );
        assert_eq!(
            p,
            ParsedCmd::Verb {
                name: "fup",
                worker: Some("simple-workout#1".into()),
                rest: "then also X".into(),
            }
        );
    }

    #[test]
    fn follow_up_alias() {
        let p = parse_command("/follow-up do more", Some("p#0"), &projects());
        assert!(matches!(p, ParsedCmd::Verb { name: "fup", .. }));
    }

    #[test]
    fn natural_dispatch_cue() {
        let p = parse_command(
            "fix the bug on simple-workout",
            Some("other#0"),
            &projects(),
        );
        assert_eq!(
            p,
            ParsedCmd::Dispatch {
                text: "fix the bug on simple-workout".into()
            }
        );
    }

    #[test]
    fn dispatch_verb() {
        let p = parse_command("/d fix landing on paperdoll", None, &projects());
        assert_eq!(
            p,
            ParsedCmd::Dispatch {
                text: "fix landing on paperdoll".into()
            }
        );
    }

    #[test]
    fn free_text_prompts_selected() {
        let p = parse_command("list files", Some("simple-workout#0"), &projects());
        assert_eq!(
            p,
            ParsedCmd::Prompt {
                worker: Some("simple-workout#0".into()),
                message: "list files".into(),
            }
        );
    }

    #[test]
    fn abort_without_message_uses_selection() {
        let p = parse_command("/abort", Some("simple-workout#0"), &projects());
        assert_eq!(
            p,
            ParsedCmd::Verb {
                name: "abort",
                worker: Some("simple-workout#0".into()),
                rest: String::new(),
            }
        );
    }
}
