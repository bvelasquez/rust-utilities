use mail_parser::MessageParser;

#[derive(Debug, Clone)]
pub struct ParsedMail {
    pub message_id: Option<String>,
    pub from_address: String,
    pub from_name: Option<String>,
    pub to_addresses: Vec<String>,
    pub subject: String,
    pub date: Option<chrono::DateTime<chrono::Utc>>,
    pub body_text: String,
    pub body_preview: String,
    pub list_unsubscribe: Option<String>,
    pub precedence: Option<String>,
    pub auto_submitted: Option<String>,
    pub raw_headers_json: String,
}

pub fn parse_raw(raw: &[u8], preview_chars: usize) -> ParsedMail {
    let Some(msg) = MessageParser::default().parse(raw) else {
        return empty_parsed(preview_chars);
    };

    let from_address = msg
        .from()
        .and_then(|a| a.first())
        .and_then(|a| a.address())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown@unknown".into());

    let from_name = msg
        .from()
        .and_then(|a| a.first())
        .and_then(|a| a.name())
        .map(|s| s.to_string());

    let to_addresses: Vec<String> = msg
        .to()
        .and_then(|a| a.first())
        .and_then(|a| a.address())
        .map(|s| vec![s.to_string()])
        .unwrap_or_default();

    let subject = msg.subject().unwrap_or("(no subject)").to_string();

    let date = msg.date().map(|d| {
        chrono::DateTime::from_timestamp(d.to_timestamp(), 0)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(chrono::Utc::now)
    });

    let body_text = msg
        .body_text(0)
        .map(|s| s.to_string())
        .or_else(|| msg.body_html(0).map(|s| strip_html_tags(&s)))
        .unwrap_or_default();

    let body_preview: String = body_text.chars().take(preview_chars).collect();

    let list_unsubscribe = header_raw(&msg, "List-Unsubscribe");
    let precedence = header_raw(&msg, "Precedence");
    let auto_submitted = header_raw(&msg, "Auto-Submitted");

    let message_id = msg.message_id().map(|s| s.to_string());

    let raw_headers_json =
        serde_json::to_string(&collect_headers(&msg)).unwrap_or_else(|_| "{}".into());

    ParsedMail {
        message_id,
        from_address,
        from_name,
        to_addresses,
        subject,
        date,
        body_text,
        body_preview,
        list_unsubscribe,
        precedence,
        auto_submitted,
        raw_headers_json,
    }
}

fn empty_parsed(preview_chars: usize) -> ParsedMail {
    ParsedMail {
        message_id: None,
        from_address: "unknown@unknown".into(),
        from_name: None,
        to_addresses: vec![],
        subject: "(unparseable)".into(),
        date: None,
        body_text: String::new(),
        body_preview: String::new().chars().take(preview_chars).collect(),
        list_unsubscribe: None,
        precedence: None,
        auto_submitted: None,
        raw_headers_json: "{}".into(),
    }
}

fn header_raw(msg: &mail_parser::Message, name: &str) -> Option<String> {
    msg.header_raw(name).map(|s| s.to_string())
}

fn collect_headers(msg: &mail_parser::Message) -> Vec<(String, String)> {
    msg.headers_raw()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

fn strip_html_tags(html: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fixture() {
        let raw = include_bytes!("../../tests/parser_fixtures/simple.eml");
        let parsed = parse_raw(raw, 200);
        assert!(parsed.from_address.contains("example.com"));
        assert_eq!(parsed.subject, "Test Subject");
    }
}
