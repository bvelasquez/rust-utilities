use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::channel;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Utc};
use exif::{In, Reader, Tag};
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use regex::Regex;
use serde::{Deserialize, Serialize};

const BUILTIN_TOKENS: &[&str] = &["ext", "filename", "yyyy", "mm", "dd"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DateSource {
    Exif,
    Created,
    Modified,
    Now,
}

#[derive(Debug, Clone)]
pub struct CompiledRules {
    pub path: PathBuf,
    pub sources: Vec<CompiledSource>,
}

#[derive(Debug, Clone)]
pub struct CompiledSource {
    pub path: PathBuf,
    pub dest_root: PathBuf,
    pub settle_secs: u64,
    pub rules: Vec<CompiledRule>,
    pub fallback: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CompiledRule {
    pub name: String,
    pub extensions: Vec<String>,
    pub filename_regex: Option<Regex>,
    pub path_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MovePlan {
    pub version: u32,
    pub generated_at: String,
    pub rules_file: String,
    pub moves: Vec<PlannedMove>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedMove {
    pub source: PathBuf,
    pub dest: PathBuf,
    pub dest_root: PathBuf,
    pub rule: String,
    pub date_source: DateSource,
    /// Content hash when the catalog was consulted. Older plan files omit this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyResult {
    pub source: String,
    pub dest: String,
    pub rule: String,
    pub date_source: DateSource,
    pub moved: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyReport {
    pub moved: usize,
    pub skipped: usize,
    pub log_path: Option<String>,
    pub results: Vec<ApplyResult>,
}

#[derive(Debug, Deserialize)]
struct RawRules {
    sources: Vec<RawSource>,
}

#[derive(Debug, Deserialize)]
struct RawSource {
    path: String,
    dest_root: String,
    #[serde(default)]
    settle_secs: Option<u64>,
    #[serde(default)]
    rules: Vec<RawRule>,
    #[serde(default)]
    fallback: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawRule {
    name: String,
    #[serde(default)]
    extensions: Vec<String>,
    #[serde(default)]
    filename_regex: Option<String>,
    path: String,
}

pub fn default_rules_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("disk-sweep")
        .join("rules.yaml")
}

pub fn load_rules(path: &Path) -> Result<CompiledRules> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read rules {}", path.display()))?;
    let raw: RawRules = serde_yaml::from_str(&text)
        .with_context(|| format!("invalid rules YAML {}", path.display()))?;
    if raw.sources.is_empty() {
        bail!("rules file has no sources");
    }
    let mut sources = Vec::new();
    for source in raw.sources {
        let mut rules = Vec::new();
        for rule in source.rules {
            rules.push(compile_rule(rule)?);
        }
        if let Some(fallback) = &source.fallback {
            validate_template(fallback, &[])?;
        }
        sources.push(CompiledSource {
            path: absolute_path(&crate::targets::expand_path(Path::new(&source.path))),
            dest_root: absolute_path(&crate::targets::expand_path(Path::new(&source.dest_root))),
            settle_secs: source.settle_secs.unwrap_or(2),
            rules,
            fallback: source.fallback,
        });
    }
    Ok(CompiledRules {
        path: path.to_path_buf(),
        sources,
    })
}

fn compile_rule(rule: RawRule) -> Result<CompiledRule> {
    if rule.extensions.is_empty() && rule.filename_regex.is_none() {
        bail!("rule '{}' needs extensions or filename_regex", rule.name);
    }
    let filename_regex = match rule.filename_regex {
        Some(pattern) => Some(
            Regex::new(&pattern)
                .with_context(|| format!("invalid filename_regex in rule '{}'", rule.name))?,
        ),
        None => None,
    };
    let captures = filename_regex
        .as_ref()
        .map(|re| re.capture_names().flatten().map(str::to_string).collect::<Vec<_>>())
        .unwrap_or_default();
    for name in &captures {
        if BUILTIN_TOKENS.contains(&name.as_str()) {
            bail!(
                "rule '{}' capture '{name}' collides with a built-in token",
                rule.name
            );
        }
    }
    validate_template(&rule.path, &captures)
        .with_context(|| format!("rule '{}' template", rule.name))?;
    Ok(CompiledRule {
        name: rule.name,
        extensions: rule
            .extensions
            .into_iter()
            .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
            .filter(|ext| !ext.is_empty())
            .collect(),
        filename_regex,
        path_template: rule.path,
    })
}

fn validate_template(template: &str, captures: &[String]) -> Result<()> {
    if template.is_empty() {
        bail!("path template is empty");
    }
    for component in Path::new(template).components() {
        if matches!(component, Component::ParentDir | Component::RootDir | Component::Prefix(_)) {
            bail!("path template cannot contain '..' or an absolute path");
        }
    }
    if template.split(['/', '\\']).any(|part| part == "..") {
        bail!("path template cannot contain '..'");
    }
    for token in tokens(template) {
        if BUILTIN_TOKENS.contains(&token.as_str()) || captures.iter().any(|c| c == &token) {
            continue;
        }
        bail!("unknown path token '{{{token}}}'");
    }
    Ok(())
}

fn tokens(template: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(end) = template[i + 1..].find('}') {
                let name = &template[i + 1..i + 1 + end];
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                    out.push(name.to_string());
                }
                i += end + 2;
                continue;
            }
        }
        i += 1;
    }
    out
}

pub fn plan_sources(rules: &CompiledRules) -> Result<MovePlan> {
    plan_sources_inner(rules, None)
}

/// Plan moves after indexing each destination. Identical content already stored goes to `duplicates/`.
pub fn plan_with_catalog(rules: &CompiledRules, catalog: &crate::catalog::Catalog) -> Result<MovePlan> {
    catalog.ensure_backfill(rules)?;
    plan_sources_inner(rules, Some(catalog))
}

fn plan_sources_inner(rules: &CompiledRules, catalog: Option<&crate::catalog::Catalog>) -> Result<MovePlan> {
    let mut moves = Vec::new();
    let mut reserved = HashSet::new();
    let mut seen_hashes = HashSet::new();
    for source in &rules.sources {
        if !source.path.is_dir() {
            bail!("source folder does not exist: {}", source.path.display());
        }
        let mut names: Vec<_> = fs::read_dir(&source.path)
            .with_context(|| format!("read {}", source.path.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        names.sort();
        for path in names {
            if let Some(planned) = plan_file(source, &path, &mut reserved, &mut seen_hashes, catalog)? {
                moves.push(planned);
            }
        }
    }
    Ok(MovePlan {
        version: 1,
        generated_at: Utc::now().to_rfc3339(),
        rules_file: rules.path.display().to_string(),
        moves,
    })
}

fn plan_file(
    source: &CompiledSource,
    path: &Path,
    reserved: &mut HashSet<PathBuf>,
    seen_hashes: &mut HashSet<String>,
    catalog: Option<&crate::catalog::Catalog>,
) -> Result<Option<PlannedMove>> {
    if !eligible(path) {
        return Ok(None);
    }
    if !settled(path, source.settle_secs) {
        return Ok(None);
    }
    let Some((rule_name, template, captures)) = match_template(source, path) else {
        return Ok(None);
    };
    let (date, date_source) = resolve_file_date(path, Local::now());
    let sha256 = match catalog {
        Some(_) => Some(crate::catalog::sha256_file(path)?),
        None => None,
    };
    let duplicate = match (catalog, sha256.as_deref()) {
        (Some(catalog), Some(sha)) => {
            catalog.kept_copy(sha)?.is_some() || !seen_hashes.insert(sha.to_string())
        }
        _ => false,
    };
    let (rule_name, dest) = if duplicate {
        let name = path
            .file_name()
            .context("source has no file name")?
            .to_os_string();
        let dest = unique_dest(&source.dest_root.join("duplicates").join(name), reserved)?;
        ("duplicate".to_string(), dest)
    } else {
        let relative = render_template(&template, path, date, &captures)?;
        let dest = unique_dest(&join_under(&source.dest_root, &relative)?, reserved)?;
        (rule_name, dest)
    };
    reserved.insert(dest.clone());
    Ok(Some(PlannedMove {
        source: path.to_path_buf(),
        dest,
        dest_root: source.dest_root.clone(),
        rule: rule_name,
        date_source,
        sha256,
    }))
}

fn eligible(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') {
        return false;
    }
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.file_type().is_symlink() => false,
        Ok(meta) if meta.is_file() => true,
        _ => false,
    }
}

fn settled(path: &Path, secs: u64) -> bool {
    if secs == 0 {
        return true;
    }
    let Ok(meta) = fs::metadata(path) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    meta.modified()
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age >= Duration::from_secs(secs))
}

fn match_template(source: &CompiledSource, path: &Path) -> Option<(String, String, Vec<(String, String)>)> {
    let name = path.file_name()?.to_str()?.to_string();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    for rule in &source.rules {
        if !rule.extensions.is_empty() && !rule.extensions.iter().any(|e| e == &ext) {
            continue;
        }
        let mut captures = Vec::new();
        if let Some(re) = &rule.filename_regex {
            let Some(found) = re.captures(&name) else {
                continue;
            };
            for group in re.capture_names().flatten() {
                let value = found.name(group).map(|m| m.as_str().to_string()).unwrap_or_default();
                captures.push((group.to_string(), value));
            }
        }
        return Some((rule.name.clone(), rule.path_template.clone(), captures));
    }
    source
        .fallback
        .clone()
        .map(|template| ("fallback".into(), template, Vec::new()))
}

pub fn resolve_file_date(path: &Path, now: DateTime<Local>) -> (NaiveDate, DateSource) {
    if let Some(date) = exif_date(path) {
        return (date, DateSource::Exif);
    }
    if let Ok(meta) = fs::metadata(path) {
        if let Some(date) = system_date(meta.created().ok()) {
            return (date, DateSource::Created);
        }
        if let Some(date) = system_date(meta.modified().ok()) {
            return (date, DateSource::Modified);
        }
    }
    (now.date_naive(), DateSource::Now)
}

/// Date priority used by the organizer. Exposed so tests can pin each step.
pub fn select_date(
    exif: Option<NaiveDate>,
    created: Option<NaiveDate>,
    modified: Option<NaiveDate>,
    now: NaiveDate,
) -> (NaiveDate, DateSource) {
    if let Some(date) = exif {
        (date, DateSource::Exif)
    } else if let Some(date) = created {
        (date, DateSource::Created)
    } else if let Some(date) = modified {
        (date, DateSource::Modified)
    } else {
        (now, DateSource::Now)
    }
}

fn exif_date(path: &Path) -> Option<NaiveDate> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if !matches!(ext.as_str(), "jpg" | "jpeg" | "tif" | "tiff") {
        return None;
    }
    let file = File::open(path).ok()?;
    let mut reader = BufReader::new(file);
    let exif = Reader::new().read_from_container(&mut reader).ok()?;
    for tag in [Tag::DateTimeOriginal, Tag::DateTimeDigitized, Tag::DateTime] {
        let Some(field) = exif.get_field(tag, In::PRIMARY) else {
            continue;
        };
        let text = field.display_value().with_unit(&exif).to_string();
        if let Some(date) = parse_exif_datetime(&text) {
            return Some(date);
        }
    }
    None
}

fn absolute_path(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

fn parse_exif_datetime(text: &str) -> Option<NaiveDate> {
    let text = text.trim().trim_matches('"');
    let date = text.get(..10)?;
    NaiveDate::parse_from_str(date, "%Y:%m:%d")
        .ok()
        .or_else(|| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
}

fn system_date(time: Option<SystemTime>) -> Option<NaiveDate> {
    let time = time?;
    let secs = time.duration_since(UNIX_EPOCH).ok()?.as_secs() as i64;
    Local.timestamp_opt(secs, 0).single().map(|t| t.date_naive())
}

fn render_template(
    template: &str,
    path: &Path,
    date: NaiveDate,
    captures: &[(String, String)],
) -> Result<String> {
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .context("file name is not valid UTF-8")?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("unknown")
        .to_ascii_lowercase();
    let mut rendered = template.to_string();
    for (name, value) in captures {
        rendered = rendered.replace(&format!("{{{name}}}"), &sanitize_component(value)?);
    }
    rendered = rendered.replace("{ext}", &sanitize_component(&ext)?);
    rendered = rendered.replace("{filename}", &sanitize_component(filename)?);
    rendered = rendered.replace("{yyyy}", &format!("{:04}", date.year()));
    rendered = rendered.replace("{mm}", &format!("{:02}", date.month()));
    rendered = rendered.replace("{dd}", &format!("{:02}", date.day()));
    if rendered.contains('{') {
        bail!("unresolved path token in '{template}'");
    }
    Ok(rendered)
}

fn sanitize_component(value: &str) -> Result<String> {
    if value.is_empty() || value == "." || value == ".." || value.contains(['/', '\\']) {
        bail!("unsafe path component '{value}'");
    }
    Ok(value.to_string())
}

fn join_under(root: &Path, relative: &str) -> Result<PathBuf> {
    let mut out = root.to_path_buf();
    for component in Path::new(relative).components() {
        match component {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => bail!("rendered path escapes {}", root.display()),
        }
    }
    Ok(out)
}

fn unique_dest(path: &Path, reserved: &HashSet<PathBuf>) -> Result<PathBuf> {
    if !path.exists() && !reserved.contains(path) {
        return Ok(path.to_path_buf());
    }
    let parent = path.parent().context("destination has no parent")?;
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file");
    let ext = path.extension().and_then(|e| e.to_str());
    for n in 2..10_000 {
        let name = match ext {
            Some(ext) => format!("{stem} ({n}).{ext}"),
            None => format!("{stem} ({n})"),
        };
        let candidate = parent.join(name);
        if !candidate.exists() && !reserved.contains(&candidate) {
            return Ok(candidate);
        }
    }
    bail!("no free name for {}", path.display())
}

pub fn write_plan(path: &Path, plan: &MovePlan) -> Result<()> {
    let yaml = serde_yaml::to_string(plan)?;
    if path.extension().and_then(|e| e.to_str()) == Some("md") {
        let mut md = String::from("# disk-sweep organize plan\n\n");
        md.push_str("| rule | date source | source | dest |\n");
        md.push_str("| --- | --- | --- | --- |\n");
        for item in &plan.moves {
            md.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                item.rule,
                date_source_label(item.date_source),
                item.source.display(),
                item.dest.display()
            ));
        }
        md.push_str("\n```yaml\n");
        md.push_str(&yaml);
        if !yaml.ends_with('\n') {
            md.push('\n');
        }
        md.push_str("```\n");
        fs::write(path, md)?;
    } else {
        fs::write(path, yaml)?;
    }
    Ok(())
}

pub fn read_plan(path: &Path) -> Result<MovePlan> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("cannot read plan {}", path.display()))?;
    let yaml = if path.extension().and_then(|e| e.to_str()) == Some("md") {
        extract_yaml_fence(&text)?
    } else {
        text
    };
    serde_yaml::from_str(&yaml).context("plan file is not a disk-sweep move list")
}

fn extract_yaml_fence(text: &str) -> Result<String> {
    let start = text
        .find("```yaml\n")
        .context("markdown plan is missing a yaml block")?;
    let body = &text[start + "```yaml\n".len()..];
    let end = body.find("\n```").context("markdown plan yaml block is not closed")?;
    Ok(body[..end].to_string())
}

pub fn date_source_label(source: DateSource) -> &'static str {
    match source {
        DateSource::Exif => "exif",
        DateSource::Created => "created",
        DateSource::Modified => "modified",
        DateSource::Now => "now",
    }
}

pub fn apply_plan(plan: &MovePlan, log_dir: Option<&Path>) -> Result<ApplyReport> {
    apply_plan_inner(plan, log_dir, true)
}

/// Move every file the rules currently match. A failure on one file does not stop the rest.
pub fn organize_pending(rules: &CompiledRules) -> Result<ApplyReport> {
    let catalog = crate::catalog::Catalog::open_default()?;
    organize_with_catalog(rules, &catalog)
}

/// Same as [`organize_pending`], using the given catalog instead of the default database.
pub fn organize_with_catalog(
    rules: &CompiledRules,
    catalog: &crate::catalog::Catalog,
) -> Result<ApplyReport> {
    let plan = plan_with_catalog(rules, catalog)?;
    let report = apply_plan_inner(&plan, None, false)?;
    record_applied(catalog, &plan, &report)?;
    Ok(report)
}

/// Apply a saved plan and record each filed copy in the catalog.
pub fn apply_recorded(plan: &MovePlan, catalog: &crate::catalog::Catalog) -> Result<ApplyReport> {
    let report = apply_plan(plan, None)?;
    record_applied(catalog, plan, &report)?;
    Ok(report)
}

fn record_applied(
    catalog: &crate::catalog::Catalog,
    plan: &MovePlan,
    report: &ApplyReport,
) -> Result<()> {
    for (item, result) in plan.moves.iter().zip(&report.results) {
        if !result.moved || item.rule == "duplicate" {
            continue;
        }
        let sha = match &item.sha256 {
            Some(sha) => sha.clone(),
            None => crate::catalog::sha256_file(&item.dest)?,
        };
        let size = fs::metadata(&item.dest).map(|meta| meta.len()).unwrap_or(0);
        catalog.remember(&sha, size, &item.dest)?;
    }
    Ok(())
}

fn apply_plan_inner(plan: &MovePlan, log_dir: Option<&Path>, stop_on_error: bool) -> Result<ApplyReport> {
    let mut results = Vec::new();
    let mut moved = 0usize;
    let mut stop = false;
    for item in &plan.moves {
        if stop {
            results.push(ApplyResult {
                source: item.source.display().to_string(),
                dest: item.dest.display().to_string(),
                rule: item.rule.clone(),
                date_source: item.date_source,
                moved: false,
                error: Some("not applied because an earlier check failed".into()),
            });
            continue;
        }
        match apply_one(item) {
            Ok(()) => {
                moved += 1;
                results.push(ApplyResult {
                    source: item.source.display().to_string(),
                    dest: item.dest.display().to_string(),
                    rule: item.rule.clone(),
                    date_source: item.date_source,
                    moved: true,
                    error: None,
                });
            }
            Err(err) => {
                if stop_on_error {
                    stop = true;
                }
                results.push(ApplyResult {
                    source: item.source.display().to_string(),
                    dest: item.dest.display().to_string(),
                    rule: item.rule.clone(),
                    date_source: item.date_source,
                    moved: false,
                    error: Some(err.to_string()),
                });
            }
        }
    }

    let log_path = append_move_log(log_dir, &results)?;
    Ok(ApplyReport {
        moved,
        skipped: results.len() - moved,
        log_path,
        results,
    })
}

fn apply_one(item: &PlannedMove) -> Result<()> {
    let meta = fs::symlink_metadata(&item.source)
        .with_context(|| format!("source missing: {}", item.source.display()))?;
    if meta.file_type().is_symlink() || !meta.is_file() {
        bail!("source is not a regular file: {}", item.source.display());
    }
    let dest_root = item
        .dest_root
        .canonicalize()
        .or_else(|_| {
            fs::create_dir_all(&item.dest_root)?;
            item.dest_root.canonicalize()
        })
        .with_context(|| format!("dest root {}", item.dest_root.display()))?;
    let dest_parent = item
        .dest
        .parent()
        .context("destination has no parent")?
        .to_path_buf();
    if !dest_is_inside(&dest_root, &item.dest) {
        bail!("destination escapes {}", dest_root.display());
    }
    if item.dest.exists() {
        bail!("destination already exists: {}", item.dest.display());
    }
    if let Some(parent) = item.dest.parent() {
        fs::create_dir_all(parent)?;
    }
    let parent_canon = dest_parent
        .canonicalize()
        .context("destination parent")?;
    if !parent_canon.starts_with(&dest_root) {
        bail!("destination escapes {}", dest_root.display());
    }
    move_file(&item.source, &item.dest)
}

fn dest_is_inside(root: &Path, dest: &Path) -> bool {
    let root = normalize_lex(root);
    let dest = normalize_lex(dest);
    dest.starts_with(&root)
}

fn normalize_lex(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn move_file(src: &Path, dest: &Path) -> Result<()> {
    match fs::rename(src, dest) {
        Ok(()) => Ok(()),
        Err(err) if err.raw_os_error() == Some(18) => {
            fs::copy(src, dest)?;
            fs::remove_file(src)?;
            Ok(())
        }
        Err(err) => Err(err).context("move file"),
    }
}

fn append_move_log(log_dir: Option<&Path>, results: &[ApplyResult]) -> Result<Option<String>> {
    if results.is_empty() {
        return Ok(None);
    }
    let dir = match log_dir {
        Some(dir) => dir.to_path_buf(),
        None => dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("disk-sweep"),
    };
    fs::create_dir_all(&dir)?;
    let day = Utc::now().format("%Y%m%d");
    let path = dir.join(format!("organize-{day}.csv"));
    let fresh = !path.exists();
    let mut file = fs::OpenOptions::new().create(true).append(true).open(&path)?;
    if fresh {
        writeln!(file, "time,source,dest,rule,date_source,moved,error")?;
    }
    let stamp = Utc::now().to_rfc3339();
    for result in results {
        writeln!(
            file,
            "{stamp},{},{},{},{},{},{}",
            result.source,
            result.dest,
            result.rule,
            date_source_label(result.date_source),
            result.moved,
            result.error.as_deref().unwrap_or("")
        )?;
    }
    Ok(Some(path.display().to_string()))
}

pub fn watch_sources(rules: &CompiledRules, json_lines: bool, cancel: &AtomicBool) -> Result<()> {
    let (tx, rx) = channel();
    let mut watcher: RecommendedWatcher = notify::recommended_watcher(move |result| {
        let _ = tx.send(result);
    })?;
    for source in &rules.sources {
        fs::create_dir_all(&source.path)
            .with_context(|| format!("create source {}", source.path.display()))?;
        watcher
            .watch(&source.path, RecursiveMode::NonRecursive)
            .with_context(|| format!("watch {}", source.path.display()))?;
        if !json_lines {
            println!("auto: {}", source.path.display());
        }
    }
    report_auto(&organize_pending(rules)?, json_lines);
    let mut last_sweep = std::time::Instant::now();
    while !cancel.load(Ordering::Relaxed) {
        match rx.recv_timeout(Duration::from_millis(250)) {
            Ok(Ok(event)) => {
                if !matches!(
                    event.kind,
                    EventKind::Create(_) | EventKind::Modify(_) | EventKind::Access(_)
                ) {
                    continue;
                }
                for path in event.paths {
                    handle_drop(rules, &path, json_lines);
                }
            }
            Ok(Err(err)) => {
                if json_lines {
                    eprintln!("{err}");
                } else {
                    println!("watch error: {err}");
                }
            }
            Err(_) => {
                if last_sweep.elapsed() >= Duration::from_secs(1) {
                    report_auto(&organize_pending(rules)?, json_lines);
                    last_sweep = std::time::Instant::now();
                }
            }
        }
    }
    Ok(())
}

fn report_auto(report: &ApplyReport, json_lines: bool) {
    if report.results.is_empty() {
        return;
    }
    if json_lines {
        let _ = crate::output::Envelope::ok("organize", report).print_json();
        return;
    }
    for item in &report.results {
        if item.moved {
            println!("moved {} -> {}", item.source, item.dest);
        } else if let Some(err) = &item.error {
            println!("skipped {}: {err}", item.source);
        }
    }
}

fn same_dir(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

fn handle_drop(rules: &CompiledRules, path: &Path, json_lines: bool) {
    let Some(parent) = path.parent() else {
        return;
    };
    if !rules.sources.iter().any(|source| same_dir(&source.path, parent)) {
        return;
    }
    match organize_pending(rules) {
        Ok(report) => report_auto(&report, json_lines),
        Err(err) => eprintln!("organize: {err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn write_rules(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("rules.yaml");
        fs::write(&path, body).unwrap();
        path
    }

    fn jpeg_with_exif_date() -> Vec<u8> {
        let tiff = [
            0x49, 0x49, 0x2a, 0x00, 0x08, 0x00, 0x00, 0x00, // TIFF header, IFD at 8
            0x01, 0x00, // IFD0 count
            0x69, 0x87, // ExifIFD pointer
            0x04, 0x00, // LONG
            0x01, 0x00, 0x00, 0x00, // count
            0x1a, 0x00, 0x00, 0x00, // offset 26
            0x00, 0x00, 0x00, 0x00, // next IFD
            0x01, 0x00, // Exif IFD count
            0x03, 0x90, // DateTimeOriginal
            0x02, 0x00, // ASCII
            0x14, 0x00, 0x00, 0x00, // 20 bytes
            0x2c, 0x00, 0x00, 0x00, // offset 44
            0x00, 0x00, 0x00, 0x00,
        ];
        let stamp = b"2020:01:02 03:04:05\0";
        assert_eq!(tiff.len(), 44);
        assert_eq!(stamp.len(), 20);
        let mut app1 = Vec::new();
        app1.extend_from_slice(b"Exif\0\0");
        app1.extend_from_slice(&tiff);
        app1.extend_from_slice(stamp);
        let len = (app1.len() + 2) as u16;
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1];
        jpeg.extend_from_slice(&len.to_be_bytes());
        jpeg.extend_from_slice(&app1);
        jpeg.extend_from_slice(&[0xff, 0xd9]);
        jpeg
    }

    #[test]
    fn date_priority_prefers_exif_then_created_then_modified() {
        let exif = NaiveDate::from_ymd_opt(2020, 1, 2).unwrap();
        let created = NaiveDate::from_ymd_opt(2021, 3, 4).unwrap();
        let modified = NaiveDate::from_ymd_opt(2022, 5, 6).unwrap();
        let now = NaiveDate::from_ymd_opt(2024, 1, 1).unwrap();
        assert_eq!(select_date(Some(exif), Some(created), Some(modified), now).1, DateSource::Exif);
        assert_eq!(select_date(None, Some(created), Some(modified), now).1, DateSource::Created);
        assert_eq!(select_date(None, None, Some(modified), now).1, DateSource::Modified);
        assert_eq!(select_date(None, None, None, now).1, DateSource::Now);
    }

    #[test]
    fn exif_date_beats_mtime_and_regex_uses_capture() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        let dest = dir.path().join("sorted");
        fs::create_dir_all(&inbox).unwrap();
        let photo = inbox.join("shot.jpeg");
        fs::write(&photo, jpeg_with_exif_date()).unwrap();
        let status = std::process::Command::new("touch")
            .args(["-d", "1999-08-08 00:00:00", photo.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(status.success());
        fs::write(inbox.join("invoice-acme-2026.pdf"), b"pdf").unwrap();
        fs::write(inbox.join("notes.txt"), b"hi").unwrap();

        let rules_path = write_rules(
            dir.path(),
            &format!(
                "sources:\n  - path: {}\n    dest_root: {}\n    settle_secs: 0\n    rules:\n      - name: photos\n        extensions: [jpg, jpeg]\n        path: \"{{ext}}/{{yyyy}}-{{mm}}-{{dd}}/{{filename}}\"\n      - name: invoices\n        filename_regex: '(?i)^invoice-(?P<client>[a-z0-9]+)[-_].+\\.pdf$'\n        path: \"invoices/{{client}}/{{yyyy}}/{{filename}}\"\n    fallback: unsorted/{{filename}}\n",
                inbox.display(),
                dest.display()
            ),
        );
        let rules = load_rules(&rules_path).unwrap();
        let plan = plan_sources(&rules).unwrap();
        let photo_move = plan.moves.iter().find(|m| m.rule == "photos").unwrap();
        assert!(photo_move.dest.ends_with("jpeg/2020-01-02/shot.jpeg"));
        assert_eq!(photo_move.date_source, DateSource::Exif);
        let invoice = plan.moves.iter().find(|m| m.rule == "invoices").unwrap();
        assert!(invoice.dest.to_string_lossy().contains("invoices/acme/"));
        assert!(plan.moves.iter().any(|m| m.rule == "fallback"));

        let yaml = dir.path().join("plan.yaml");
        write_plan(&yaml, &plan).unwrap();
        assert!(fs::read_to_string(&yaml).unwrap().contains("shot.jpeg"));
        let reread = read_plan(&yaml).unwrap();
        assert_eq!(reread.moves.len(), plan.moves.len());

        let md = dir.path().join("plan.md");
        write_plan(&md, &plan).unwrap();
        let from_md = read_plan(&md).unwrap();
        assert_eq!(from_md.moves.len(), plan.moves.len());

        let preview = fs::read_to_string(&yaml).unwrap();
        assert!(preview.contains("source:"));
        assert!(!dest.join("jpeg/2020-01-02/shot.jpeg").exists());

        let report = apply_plan(&plan, Some(dir.path())).unwrap();
        assert_eq!(report.moved, 3);
        assert!(dest.join("jpeg/2020-01-02/shot.jpeg").exists());
        assert!(!photo.exists());
    }

    #[test]
    fn plain_file_without_exif_uses_metadata_date() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        let dest = dir.path().join("out");
        fs::create_dir_all(&inbox).unwrap();
        let file = inbox.join("scan.jpeg");
        fs::write(&file, b"not-really-jpeg").unwrap();
        let status = std::process::Command::new("touch")
            .args(["-d", "2024-05-06 15:00:00", file.to_str().unwrap()])
            .status()
            .unwrap();
        assert!(status.success());
        let (date, source) = resolve_file_date(&file, Local.with_ymd_and_hms(1990, 1, 1, 0, 0, 0).unwrap());
        assert_ne!(source, DateSource::Exif);
        assert_ne!(source, DateSource::Now);
        let rules = load_rules(&write_rules(
            dir.path(),
            &format!(
                "sources:\n  - path: {}\n    dest_root: {}\n    settle_secs: 0\n    rules:\n      - name: photos\n        extensions: [jpeg]\n        path: \"{{ext}}/{{yyyy}}-{{mm}}-{{dd}}/{{filename}}\"\n",
                inbox.display(),
                dest.display()
            ),
        ))
        .unwrap();
        let plan = plan_sources(&rules).unwrap();
        let expected = format!("jpeg/{:04}-{:02}-{:02}/scan.jpeg", date.year(), date.month(), date.day());
        assert!(plan.moves[0].dest.ends_with(&expected));
    }

    #[test]
    fn invalid_regex_and_token_collision_fail_at_load() {
        let dir = tempfile::tempdir().unwrap();
        let bad = write_rules(
            dir.path(),
            "sources:\n  - path: /tmp\n    dest_root: /tmp/out\n    rules:\n      - name: bad\n        filename_regex: '('\n        path: '{filename}'\n",
        );
        assert!(load_rules(&bad).is_err());
        let clash = write_rules(
            dir.path(),
            "sources:\n  - path: /tmp\n    dest_root: /tmp/out\n    rules:\n      - name: bad\n        filename_regex: '(?P<ext>a)'\n        path: '{ext}/{filename}'\n",
        );
        assert!(load_rules(&clash).is_err());
        let escape = write_rules(
            dir.path(),
            "sources:\n  - path: /tmp\n    dest_root: /tmp/out\n    rules:\n      - name: bad\n        extensions: [txt]\n        path: '../{filename}'\n",
        );
        assert!(load_rules(&escape).is_err());
    }

    #[test]
    fn collision_gets_numeric_suffix_and_apply_skips_existing_dest() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        let dest = dir.path().join("out");
        fs::create_dir_all(&inbox).unwrap();
        fs::create_dir_all(dest.join("txt")).unwrap();
        fs::write(inbox.join("a.txt"), b"one").unwrap();
        fs::write(inbox.join("b.txt"), b"two").unwrap();
        fs::write(dest.join("txt/a.txt"), b"already").unwrap();
        let rules = load_rules(&write_rules(
            dir.path(),
            &format!(
                "sources:\n  - path: {}\n    dest_root: {}\n    settle_secs: 0\n    rules:\n      - name: text\n        extensions: [txt]\n        path: 'txt/a.txt'\n",
                inbox.display(),
                dest.display()
            ),
        ))
        .unwrap();
        let plan = plan_sources(&rules).unwrap();
        assert_eq!(plan.moves.len(), 2);
        let names: Vec<_> = plan
            .moves
            .iter()
            .map(|m| m.dest.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(names.iter().any(|n| n.contains("(2)") || n.contains("(3)")));

        let blocked = dest.join("txt/blocked.txt");
        let mut second = plan.clone();
        second.moves[0].dest = dest.join("txt/first.txt");
        second.moves[1].dest = blocked.clone();
        fs::write(&blocked, b"occupied").unwrap();
        let report = apply_plan(&second, Some(dir.path())).unwrap();
        assert!(report.results[0].moved);
        assert!(!report.results[1].moved);
        assert!(inbox.join("b.txt").exists() || second.moves[0].source.ends_with("b.txt") && inbox.join("a.txt").exists());
    }

    #[test]
    fn pending_moves_a_file_without_a_plan_file() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        let dest = dir.path().join("out");
        fs::create_dir_all(&inbox).unwrap();
        fs::write(inbox.join("note.md"), b"hello").unwrap();
        let rules = load_rules(&write_rules(
            dir.path(),
            &format!(
                "sources:\n  - path: {}\n    dest_root: {}\n    settle_secs: 0\n    rules:\n      - name: markdown\n        extensions: [md]\n        path: 'notes/{{filename}}'\n",
                inbox.display(),
                dest.display()
            ),
        ))
        .unwrap();
        let catalog = crate::catalog::Catalog::open(&dir.path().join("catalog.sqlite")).unwrap();
        let report = organize_with_catalog(&rules, &catalog).unwrap();
        assert_eq!(report.moved, 1);
        assert!(!inbox.join("note.md").exists());
        assert!(dest.join("notes/note.md").exists());
    }

    #[test]
    fn identical_content_is_filed_once() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        let dest = dir.path().join("out");
        fs::create_dir_all(&inbox).unwrap();
        let rules = load_rules(&write_rules(
            dir.path(),
            &format!(
                "sources:\n  - path: {}\n    dest_root: {}\n    settle_secs: 0\n    rules:\n      - name: markdown\n        extensions: [md]\n        path: 'notes/{{filename}}'\n",
                inbox.display(),
                dest.display()
            ),
        ))
        .unwrap();
        let catalog = crate::catalog::Catalog::open(&dir.path().join("catalog.sqlite")).unwrap();
        fs::write(inbox.join("note.md"), b"hello").unwrap();
        fs::write(inbox.join("same.md"), b"hello").unwrap();
        fs::write(inbox.join("other.md"), b"different").unwrap();
        let first = organize_with_catalog(&rules, &catalog).unwrap();
        assert_eq!(first.moved, 3);
        assert!(dest.join("notes/note.md").is_file());
        assert!(dest.join("duplicates/same.md").is_file());
        assert!(dest.join("notes/note.md").is_file());
        assert!(dest.join("notes/other.md").is_file());

        fs::write(inbox.join("copy.md"), b"hello").unwrap();
        let second = organize_with_catalog(&rules, &catalog).unwrap();
        assert_eq!(second.moved, 1);
        assert_eq!(second.results[0].rule, "duplicate");
        assert!(dest.join("duplicates/copy.md").is_file());
        assert!(!dest.join("notes/copy.md").exists());
        assert_eq!(fs::read(dest.join("notes/note.md")).unwrap(), b"hello");
    }

    #[test]
    fn missing_catalog_file_can_be_filed_again() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        let dest = dir.path().join("out");
        fs::create_dir_all(&inbox).unwrap();
        let rules = load_rules(&write_rules(
            dir.path(),
            &format!(
                "sources:\n  - path: {}\n    dest_root: {}\n    settle_secs: 0\n    rules:\n      - name: markdown\n        extensions: [md]\n        path: 'notes/{{filename}}'\n",
                inbox.display(),
                dest.display()
            ),
        ))
        .unwrap();
        let catalog = crate::catalog::Catalog::open(&dir.path().join("catalog.sqlite")).unwrap();
        let sha = crate::catalog::sha256_bytes(b"hello");
        catalog
            .remember(&sha, 5, &dest.join("notes/gone.md"))
            .unwrap();
        fs::write(inbox.join("note.md"), b"hello").unwrap();
        let report = organize_with_catalog(&rules, &catalog).unwrap();
        assert_eq!(report.moved, 1);
        assert_eq!(report.results[0].rule, "markdown");
        assert!(dest.join("notes/note.md").is_file());
    }
}
