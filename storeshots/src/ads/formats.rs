use crate::sizes::ExportSize;

/// One exportable ad dimension with platform metadata.
#[derive(Debug, Clone, Copy)]
pub struct AdFormat {
    pub id: &'static str,
    pub title: &'static str,
    pub platform: &'static str,
    pub group: &'static str,
    pub w: u32,
    pub h: u32,
}

impl AdFormat {
    pub fn export_size(&self) -> ExportSize {
        ExportSize {
            label: self.id,
            w: self.w,
            h: self.h,
        }
    }

    pub fn aspect_ratio(&self) -> f32 {
        self.w as f32 / self.h as f32
    }

    /// Closest Gemini image aspect ratio label.
    pub fn gemini_aspect(&self) -> &'static str {
        let r = self.aspect_ratio();
        if r > 2.5 {
            "16:9"
        } else if r > 1.4 {
            "16:9"
        } else if r > 1.1 {
            "4:3"
        } else if r > 0.85 {
            "1:1"
        } else if r > 0.7 {
            "3:4"
        } else {
            "9:16"
        }
    }

    pub fn is_banner(&self) -> bool {
        self.aspect_ratio() > 2.8
    }
}

pub const AD_FORMATS: &[AdFormat] = &[
    // Google Performance Max (required marketing images)
    AdFormat {
        id: "pmax-landscape",
        title: "PMax landscape (1.91:1)",
        platform: "google-ads",
        group: "google-pmax",
        w: 1200,
        h: 628,
    },
    AdFormat {
        id: "pmax-square",
        title: "PMax square",
        platform: "google-ads",
        group: "google-pmax",
        w: 1200,
        h: 1200,
    },
    AdFormat {
        id: "pmax-portrait",
        title: "PMax portrait (4:5)",
        platform: "google-ads",
        group: "google-pmax",
        w: 960,
        h: 1200,
    },
    // Google Play store listing
    AdFormat {
        id: "play-feature-graphic",
        title: "Play feature graphic",
        platform: "google-play",
        group: "play-feature",
        w: 1024,
        h: 500,
    },
    // Google Display / IAB standard sizes
    AdFormat {
        id: "display-medium-rectangle",
        title: "Medium rectangle",
        platform: "google-display",
        group: "google-display",
        w: 300,
        h: 250,
    },
    AdFormat {
        id: "display-large-rectangle",
        title: "Large rectangle",
        platform: "google-display",
        group: "google-display",
        w: 336,
        h: 280,
    },
    AdFormat {
        id: "display-leaderboard",
        title: "Leaderboard",
        platform: "google-display",
        group: "google-display",
        w: 728,
        h: 90,
    },
    AdFormat {
        id: "display-wide-skyscraper",
        title: "Wide skyscraper",
        platform: "google-display",
        group: "google-display",
        w: 160,
        h: 600,
    },
    AdFormat {
        id: "display-half-page",
        title: "Half page",
        platform: "google-display",
        group: "google-display",
        w: 300,
        h: 600,
    },
    AdFormat {
        id: "display-billboard",
        title: "Billboard",
        platform: "google-display",
        group: "google-display",
        w: 970,
        h: 250,
    },
    AdFormat {
        id: "display-mobile-banner",
        title: "Mobile banner",
        platform: "google-display",
        group: "google-display",
        w: 320,
        h: 50,
    },
    // Meta / social
    AdFormat {
        id: "social-square",
        title: "Social square (feed)",
        platform: "meta",
        group: "social",
        w: 1080,
        h: 1080,
    },
    AdFormat {
        id: "social-story",
        title: "Social story / reel",
        platform: "meta",
        group: "social",
        w: 1080,
        h: 1920,
    },
    AdFormat {
        id: "social-landscape",
        title: "Social landscape link",
        platform: "meta",
        group: "social",
        w: 1200,
        h: 628,
    },
];

pub const AD_FORMAT_GROUPS: &[(&str, &str)] = &[
    ("google-pmax", "Google Performance Max (landscape, square, portrait)"),
    ("google-display", "Google Display / IAB banner sizes"),
    ("social", "Meta & social (square, story, landscape)"),
    ("play-feature", "Google Play feature graphic"),
    ("all", "All supported ad sizes"),
];

pub fn format_by_id(id: &str) -> Option<&'static AdFormat> {
    AD_FORMATS.iter().find(|f| f.id == id)
}

pub fn formats_for_groups(groups: &[String]) -> Vec<&'static AdFormat> {
    if groups.is_empty() || groups.iter().any(|g| g == "all") {
        return AD_FORMATS.iter().collect();
    }
    AD_FORMATS
        .iter()
        .filter(|f| groups.iter().any(|g| g == f.group))
        .collect()
}

pub fn format_id_slice() -> Vec<&'static str> {
    AD_FORMATS.iter().map(|f| f.id).collect()
}

pub fn group_id_slice() -> Vec<&'static str> {
    AD_FORMAT_GROUPS.iter().map(|(id, _)| *id).collect()
}

/// Pick layout automatically when config layout is `auto`.
pub fn auto_layout(format: &AdFormat) -> &'static str {
    effective_layout_for_format(format)
}

/// Format-driven layout — ignores creative-specific layout when it would break at this size.
pub fn effective_layout_for_format(format: &AdFormat) -> &'static str {
    if format.h <= 50 || (format.is_banner() && format.h <= 100) {
        return "text-strip";
    }
    if format.w <= 180 && format.h >= 400 {
        return "skyscraper";
    }
    if format.w <= 380 && format.h <= 320 {
        return "compact";
    }
    if format.aspect_ratio() > 1.25 {
        return "landscape-split";
    }
    "stacked"
}
