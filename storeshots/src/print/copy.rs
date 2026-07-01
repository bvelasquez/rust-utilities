use crate::config::StoreshotsConfig;
use anyhow::{Context, Result};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct PrintCopy {
    pub name: String,
    pub website_label: String,
    pub qr_url: String,
    pub tagline: String,
    pub card_tagline: String,
    pub pitch: String,
    pub eyebrow: String,
    pub features: Vec<String>,
    /// Short product titles for business cards (from brand key features).
    pub card_features: Vec<String>,
    pub contact_email: String,
    pub support_email: Option<String>,
    pub disclaimer: Option<String>,
}

impl PrintCopy {
    /// Short line for tight layouts (business card, QR panels).
    pub fn card_line(&self) -> &str {
        if !self.card_tagline.is_empty() {
            return &self.card_tagline;
        }
        &self.tagline
    }

    /// Headline sized for brochure panels (not the full BRAND.md tagline paragraph).
    pub fn print_headline(&self) -> String {
        shorten_line(&self.tagline, 72)
    }

    /// Two-to-three sentence pitch for copy columns.
    pub fn print_pitch(&self) -> String {
        shorten_to_sentence(&self.pitch, 200)
    }

    /// Short feature titles for bullet lists (strips markdown, uses label before colon).
    pub fn print_features(&self) -> Vec<String> {
        self.features
            .iter()
            .map(|f| format_feature_bullet(f))
            .take(5)
            .collect()
    }

    /// Top product bullets for tight layouts (business card).
    pub fn card_bullets(&self) -> Vec<String> {
        if !self.card_features.is_empty() {
            return self.card_features.clone();
        }
        self.features
            .iter()
            .map(|f| card_bullet_label(f))
            .take(3)
            .collect()
    }

    pub fn hero_headline(&self) -> Vec<String> {
        vec![
            "Production systems,".into(),
            "not demo chatbots".into(),
        ]
    }
}

pub fn resolve_print_copy(app_root: &Path, cfg: &StoreshotsConfig) -> Result<PrintCopy> {
    let brand_path = cfg.brand_path(app_root);
    let brand_text = std::fs::read_to_string(&brand_path)
        .with_context(|| format!("read brand file {}", brand_path.display()))?;
    let parsed = parse_brand_md(&brand_text);

    let overrides = &cfg.print.copy;
    let name = cfg.app.name.clone();
    let website = overrides
        .website
        .clone()
        .or(parsed.website.clone())
        .unwrap_or_else(|| "https://example.com".into());
    let qr_url = overrides
        .qr_url
        .clone()
        .unwrap_or_else(|| website.clone());
    let website_label = site_label(&website);
    let tagline = overrides
        .headline
        .clone()
        .or(parsed.short_tagline.clone())
        .or(parsed.one_line.clone())
        .or(parsed.tagline.clone())
        .unwrap_or_else(|| name.clone());
    let pitch = overrides
        .pitch
        .clone()
        .or(parsed.pitch.clone())
        .unwrap_or_else(|| tagline.clone());
    let card_tagline = overrides
        .card_tagline
        .clone()
        .or(parsed.one_line.clone())
        .map(|s| shorten_line(&s, 72))
        .unwrap_or_else(|| shorten_line(&tagline, 72));
    let eyebrow = overrides
        .eyebrow
        .clone()
        .or(parsed.category.clone())
        .unwrap_or_else(|| "Marketing".into());
    let contact_email = overrides
        .contact_email
        .clone()
        .or(parsed.sales_email.clone())
        .or(parsed.support_email.clone())
        .unwrap_or_else(|| "hello@example.com".into());

    let mut features = if !overrides.bullets.is_empty() {
        overrides.bullets.clone()
    } else if !parsed.features.is_empty() {
        parsed.features.clone()
    } else {
        vec![tagline.clone()]
    };
    features.truncate(8);
    let card_features = card_features_from_brand(&parsed.features, &brand_text);

    Ok(PrintCopy {
        name,
        website_label,
        qr_url,
        tagline,
        card_tagline,
        pitch,
        eyebrow,
        features,
        card_features,
        contact_email,
        support_email: parsed.support_email,
        disclaimer: parsed.disclaimer,
    })
}

#[derive(Default)]
struct ParsedBrand {
    website: Option<String>,
    tagline: Option<String>,
    short_tagline: Option<String>,
    one_line: Option<String>,
    pitch: Option<String>,
    category: Option<String>,
    features: Vec<String>,
    sales_email: Option<String>,
    support_email: Option<String>,
    disclaimer: Option<String>,
}

fn parse_brand_md(text: &str) -> ParsedBrand {
    let mut out = ParsedBrand::default();
    let lines: Vec<&str> = text.lines().collect();

    out.website = table_value(text, "Website").or(find_url(text));
    out.category = table_value(text, "Category");
    out.support_email = table_value(text, "Support Email").or(find_email(text, "support"));
    out.sales_email = table_value(text, "Sales Email").or(find_email(text, "sales"));

    if let Some(section) = section_body(&lines, "one-line description") {
        out.one_line = Some(first_meaningful_line(&section));
    }
    if let Some(section) = section_body(&lines, "taglines") {
        let items = bullets(&section);
        out.tagline = items.first().cloned();
        out.short_tagline = items.into_iter().min_by_key(|s| s.len());
    }
    if let Some(section) = section_body(&lines, "elevator pitch") {
        out.pitch = Some(paragraph(&section));
    }
    if let Some(section) = section_body(&lines, "key features") {
        out.features = bullets(&section);
    }
    if let Some(section) = section_body(&lines, "required disclaimers") {
        let d = paragraph(&section);
        if !d.is_empty() && !d.to_lowercase().contains("none explicitly") {
            out.disclaimer = Some(d);
        }
    }

    out
}

fn table_value(text: &str, key: &str) -> Option<String> {
    let key_lower = key.to_lowercase();
    for line in text.lines() {
        if line.contains('|') && line.to_lowercase().contains(&key_lower) {
            let cells: Vec<_> = line.split('|').map(str::trim).collect();
            if cells.len() >= 3 {
                let val = cells[2].trim();
                if !val.is_empty() && val != "Value" && !val.starts_with(':') {
                    return Some(clean_md(val));
                }
            }
        }
    }
    None
}

fn section_body(lines: &[&str], heading_needle: &str) -> Option<String> {
    let needle = heading_needle.to_lowercase();
    let start = lines.iter().position(|l| {
        l.starts_with('#') && l.to_lowercase().contains(&needle)
    })?;
    let mut body = Vec::new();
    for line in lines.iter().skip(start + 1) {
        if line.starts_with('#') {
            break;
        }
        body.push(*line);
    }
    Some(body.join("\n"))
}

fn first_meaningful_line(section: &str) -> String {
    section
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with('*') && !l.starts_with('-'))
        .map(clean_md)
        .unwrap_or_default()
}

fn bullets(section: &str) -> Vec<String> {
    section
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            if t.starts_with('*') || t.starts_with('-') {
                Some(clean_md(t.trim_start_matches('*').trim_start_matches('-')))
            } else {
                None
            }
        })
        .collect()
}

fn paragraph(section: &str) -> String {
    section
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('*') && !l.starts_with('-'))
        .map(clean_md)
        .collect::<Vec<_>>()
        .join(" ")
}

fn clean_md(s: &str) -> String {
    let mut out = s.trim().trim_start_matches('*').trim().trim_matches('`').to_string();
    out = out.replace("**", "");
    out
}

fn format_feature_bullet(s: &str) -> String {
    let s = clean_md(s);
    if let Some((title, _)) = s.split_once(':') {
        let title = title.trim();
        if !title.is_empty() {
            return shorten_line(title, 52);
        }
    }
    shorten_line(&s, 72)
}

/// One-line product highlight for business cards (clause before comma / "with").
fn card_bullet_label(s: &str) -> String {
    let s = format_feature_bullet(s);
    let clause = s
        .split(" with ")
        .next()
        .and_then(|p| p.split(',').next())
        .unwrap_or(&s)
        .trim();
    shorten_line(clause, 52)
}

fn card_features_from_brand(features: &[String], brand_text: &str) -> Vec<String> {
    if features.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<String> = features
        .iter()
        .take(2)
        .map(|f| card_feature_title(f))
        .collect();
    let brand_l = brand_text.to_lowercase();
    let third = if brand_l.contains("app store") || brand_l.contains("ios and android") {
        "Deploy & Release Management".into()
    } else if features.len() > 2 {
        card_feature_title(&features[2])
    } else {
        String::new()
    };
    if !third.is_empty() {
        out.push(third);
    }
    out
}

fn card_feature_title(s: &str) -> String {
    let s = clean_md(s);
    if let Some((title, desc)) = s.split_once(':') {
        let title = clean_md(title);
        let desc_l = desc.to_lowercase();
        if title.to_lowercase().contains("full-stack")
            && (desc_l.contains("react native") || desc_l.contains("react,"))
        {
            return format!("{title} (React, React Native)");
        }
        return title;
    }
    shorten_line(&s, 56)
}

fn find_url(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|t| t.starts_with("http://") || t.starts_with("https://"))
        .map(|s| s.trim_matches(|c: char| c == '<' || c == '>' || c == ')').to_string())
}

fn find_email(text: &str, prefix: &str) -> Option<String> {
    text.lines()
        .find_map(|l| {
            let lower = l.to_lowercase();
            if lower.contains(prefix) {
                l.split_whitespace()
                    .find(|t| t.contains('@'))
                    .map(|e| e.trim_matches(|c: char| c == '<' || c == '>' || c == ')').to_string())
            } else {
                None
            }
        })
}

fn shorten_line(s: &str, max_chars: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max_chars).collect();
    if let Some(sp) = out.rfind(' ') {
        out.truncate(sp);
    }
    out.trim_end_matches([',', '.', ';']).to_string()
}

fn shorten_to_sentence(s: &str, max_chars: usize) -> String {
    let s = s.trim();
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max_chars).collect();
    if let Some(pos) = truncated.rfind(". ") {
        return truncated[..pos + 1].trim().to_string();
    }
    let mut out = shorten_line(s, max_chars);
    if !out.ends_with('.') {
        out.push('.');
    }
    out
}

pub fn site_label(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_site_label() {
        assert_eq!(site_label("https://soki-creative.com/"), "soki-creative.com");
    }
}
