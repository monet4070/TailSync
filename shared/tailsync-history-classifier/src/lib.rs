mod code;
mod command;
mod model;
mod paths;
mod structured;
mod website;

pub use model::{
    is_known_category, Classification, CATEGORIES, CLASSIFIER_VERSION, MAX_SAMPLE_BYTES,
};

pub fn classify_text(text: &str) -> Classification {
    let (sample, truncated) = sample_prefix(text);
    let sample = sample.trim();
    if sample.is_empty() {
        return Classification::new("text", 99);
    }
    if !truncated {
        if let Some(confidence) = website::website_confidence(sample) {
            return if confidence >= 95 {
                Classification::new("website", confidence)
            } else {
                Classification::ambiguous("website", confidence)
            };
        }
        if structured::is_structured_json(sample) {
            return Classification::new("structured_data", 99);
        }
        if command::is_command(sample) {
            return Classification::ambiguous("command", 92);
        }
        if command::is_command_block(sample) {
            return Classification::new("command", 96);
        }
        if paths::is_path(sample) {
            return Classification::new("path", 96);
        }
    }

    let code_score = code::code_score(sample);
    if code_score >= 8 {
        Classification::new("code", 96)
    } else if !truncated && code_score >= 6 {
        Classification::ambiguous("code", 90)
    } else {
        Classification::new("text", 75)
    }
}

fn sample_prefix(text: &str) -> (&str, bool) {
    if text.len() <= MAX_SAMPLE_BYTES {
        return (text, false);
    }
    let mut end = MAX_SAMPLE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

#[cfg(test)]
mod tests;
