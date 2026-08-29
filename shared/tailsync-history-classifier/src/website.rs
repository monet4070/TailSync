pub(super) fn website_confidence(value: &str) -> Option<u8> {
    if value.len() > 2048
        || value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '\\')
    {
        return None;
    }
    let (rest, prefixed_confidence) = if value
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
    {
        (&value[8..], Some(99))
    } else if value
        .get(..7)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
    {
        (&value[7..], Some(99))
    } else if let Some(rest) = value.strip_prefix("//") {
        (rest, Some(98))
    } else {
        (value, None)
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    if authority.is_empty() {
        return None;
    }
    if prefixed_confidence.is_none() && authority.contains('@') {
        return None;
    }
    let host_port = if prefixed_confidence.is_some() {
        authority.rsplit('@').next().unwrap_or_default()
    } else {
        authority
    };
    if host_port.starts_with('[') {
        let end = host_port.find(']')?;
        let port_suffix = &host_port[end + 1..];
        if end <= 1
            || host_port[1..end].parse::<std::net::Ipv6Addr>().is_err()
            || !valid_port_suffix(port_suffix)
            || (prefixed_confidence.is_none() && port_suffix.is_empty())
        {
            return None;
        }
        return Some(prefixed_confidence.unwrap_or(93));
    }

    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) if !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => {
            (host, Some(port))
        }
        Some(_) if host_port.matches(':').count() == 1 => return None,
        _ => (host_port, None),
    };
    if port.is_some_and(|value| value.parse::<u16>().is_err()) {
        return None;
    }
    if host.eq_ignore_ascii_case("localhost") {
        return prefixed_confidence.or_else(|| port.map(|_| 93));
    }
    if host.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return host
            .parse::<std::net::Ipv4Addr>()
            .ok()
            .map(|_| prefixed_confidence.unwrap_or(93));
    }
    if !is_valid_domain_host(host) {
        return None;
    }
    if let Some(confidence) = prefixed_confidence {
        return Some(confidence);
    }
    if has_www_prefix(host) {
        return Some(97);
    }
    if port.is_some() {
        return Some(94);
    }
    if has_common_domain_suffix(host) {
        return Some(if rest.len() > authority.len() { 96 } else { 93 });
    }
    None
}

fn is_valid_domain_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('.')
        && !host.ends_with('.')
        && host.contains('.')
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

fn has_www_prefix(host: &str) -> bool {
    let Some(remainder) = host.get(4..) else {
        return false;
    };
    if !host[..4].eq_ignore_ascii_case("www.") {
        return false;
    }
    let Some((domain, suffix)) = remainder.rsplit_once('.') else {
        return false;
    };
    !domain.is_empty()
        && (2..=24).contains(&suffix.len())
        && suffix.chars().all(|c| c.is_ascii_alphabetic())
}

fn has_common_domain_suffix(host: &str) -> bool {
    const COMMON_TLDS: &[&str] = &[
        "ai", "au", "biz", "br", "ca", "cn", "co", "com", "de", "edu", "fr", "gov", "hk", "in",
        "info", "io", "jp", "kr", "me", "mx", "net", "nl", "online", "org", "ru", "sg", "site",
        "top", "tv", "tw", "uk", "us", "xyz",
    ];
    let suffix = host.rsplit('.').next().unwrap_or_default();
    COMMON_TLDS
        .iter()
        .any(|candidate| suffix.eq_ignore_ascii_case(candidate))
}

fn valid_port_suffix(suffix: &str) -> bool {
    if suffix.is_empty() {
        return true;
    }
    let Some(port) = suffix.strip_prefix(':') else {
        return false;
    };
    !port.is_empty() && port.parse::<u16>().is_ok()
}
