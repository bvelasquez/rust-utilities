use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PrintFormatSpec {
    pub id: &'static str,
    pub title: &'static str,
    pub size: &'static str,
    pub output: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variant: Option<&'static str>,
}

pub const PRINT_FORMATS: &[PrintFormatSpec] = &[
    PrintFormatSpec {
        id: "trifold",
        title: "Tri-fold brochure",
        size: "11×8.5 in landscape (US letter folded)",
        output: "outside + inside PNGs, combined PDF",
        variant: None,
    },
    PrintFormatSpec {
        id: "single-landscape",
        title: "Single-page flyer (landscape)",
        size: "11×8.5 in landscape",
        output: "1 PNG + PDF",
        variant: None,
    },
    PrintFormatSpec {
        id: "single-portrait",
        title: "Single-page flyer (portrait)",
        size: "8.5×11 in portrait",
        output: "1 PNG + PDF",
        variant: None,
    },
    PrintFormatSpec {
        id: "business-card",
        title: "Business card",
        size: "3.625×2.125 in (US standard + bleed)",
        output: "front + back PNGs, combined PDF",
        variant: Some("front | back | both (default: both)"),
    },
];

pub fn format_id_slice() -> &'static [&'static str] {
    &["trifold", "single-landscape", "single-portrait", "business-card"]
}

pub fn unknown_format_message(got: &str) -> String {
    let list = PRINT_FORMATS
        .iter()
        .map(|f| format!("  {} — {}", f.id, f.title))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "unknown print format: {got}\n\nSupported formats (see `storeshots print formats`):\n{list}"
    )
}
