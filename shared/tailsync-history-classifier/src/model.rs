pub const CLASSIFIER_VERSION: i64 = 5;
pub const MAX_SAMPLE_BYTES: usize = 16 * 1024;

pub const CATEGORIES: [&str; 6] = ["text", "website", "code", "path", "image", "file"];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Classification {
    pub category: &'static str,
    pub confidence: u8,
    pub secondary_category: Option<&'static str>,
}

impl Classification {
    pub(super) const fn new(category: &'static str, confidence: u8) -> Self {
        Self {
            category,
            confidence,
            secondary_category: None,
        }
    }

    pub(super) const fn ambiguous(category: &'static str, confidence: u8) -> Self {
        Self {
            category,
            confidence,
            secondary_category: Some("text"),
        }
    }

    pub fn categories(self) -> Vec<&'static str> {
        let mut categories = vec![self.category];
        if let Some(secondary) = self.secondary_category {
            if secondary != self.category {
                categories.push(secondary);
            }
        }
        categories
    }
}

pub fn is_known_category(category: &str) -> bool {
    CATEGORIES.contains(&canonical_category(category))
}

/// Accept legacy names from saved records and older clients as the same code group.
pub fn canonical_category(category: &str) -> &str {
    match category {
        "command" | "structured_data" => "code",
        _ => category,
    }
}
