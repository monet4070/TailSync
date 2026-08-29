pub const CLASSIFIER_VERSION: i64 = 4;
pub const MAX_SAMPLE_BYTES: usize = 16 * 1024;

pub const CATEGORIES: [&str; 8] = [
    "text",
    "website",
    "code",
    "command",
    "structured_data",
    "path",
    "image",
    "file",
];

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
    CATEGORIES.contains(&category)
}
