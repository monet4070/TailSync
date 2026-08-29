pub(super) fn is_path(value: &str) -> bool {
    if value.len() > 2048
        || value.chars().any(|c| c.is_control())
        || [" && ", " || ", " | ", " > ", " < "]
            .iter()
            .any(|operator| value.contains(operator))
    {
        return false;
    }
    let unquoted = strip_matching_quotes(value);
    if unquoted.contains(" --") {
        return false;
    }
    let bytes = unquoted.as_bytes();
    if bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'\\' | b'/')
    {
        return true;
    }
    if is_unc_path(unquoted)
        || unquoted.starts_with("~/")
        || unquoted.starts_with("~\\")
        || unquoted.starts_with("./")
        || unquoted.starts_with(".\\")
        || unquoted.starts_with("../")
        || unquoted.starts_with("..\\")
        || (unquoted.starts_with("file:///") && unquoted.len() > "file:///".len())
    {
        return true;
    }
    if !unquoted.starts_with('/') || unquoted.starts_with("//") {
        return false;
    }
    const UNIX_ROOTS: [&str; 12] = [
        "/Applications/",
        "/Library/",
        "/System/",
        "/Users/",
        "/Volumes/",
        "/bin/",
        "/etc/",
        "/home/",
        "/opt/",
        "/tmp/",
        "/usr/",
        "/var/",
    ];
    UNIX_ROOTS.iter().any(|root| unquoted.starts_with(root))
}

fn is_unc_path(value: &str) -> bool {
    let Some(rest) = value.strip_prefix("\\\\") else {
        return false;
    };
    let mut parts = rest.split(['\\', '/']).filter(|part| !part.is_empty());
    parts.next().is_some() && parts.next().is_some()
}

fn strip_matching_quotes(value: &str) -> &str {
    if value.len() < 2 {
        return value;
    }
    let bytes = value.as_bytes();
    if matches!(
        (bytes[0], bytes[bytes.len() - 1]),
        (b'"', b'"') | (b'\'', b'\'')
    ) {
        &value[1..value.len() - 1]
    } else {
        value
    }
}
