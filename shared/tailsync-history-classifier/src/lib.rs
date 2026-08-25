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
    const fn new(category: &'static str, confidence: u8) -> Self {
        Self {
            category,
            confidence,
            secondary_category: None,
        }
    }

    const fn ambiguous(category: &'static str, confidence: u8) -> Self {
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

pub fn classify_text(text: &str) -> Classification {
    let (sample, truncated) = sample_prefix(text);
    let sample = sample.trim();
    if sample.is_empty() {
        return Classification::new("text", 99);
    }
    if !truncated {
        if let Some(confidence) = website_confidence(sample) {
            return if confidence >= 95 {
                Classification::new("website", confidence)
            } else {
                Classification::ambiguous("website", confidence)
            };
        }
        if is_structured_json(sample) {
            return Classification::new("structured_data", 99);
        }
        if is_command(sample) {
            return Classification::ambiguous("command", 92);
        }
        if is_command_block(sample) {
            return Classification::new("command", 96);
        }
        if is_path(sample) {
            return Classification::new("path", 96);
        }
    }

    let code_score = code_score(sample);
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

fn website_confidence(value: &str) -> Option<u8> {
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

fn is_structured_json(value: &str) -> bool {
    if !matches!(value.as_bytes().first(), Some(b'{') | Some(b'[')) {
        return false;
    }
    matches!(
        serde_json::from_str::<serde_json::Value>(value),
        Ok(serde_json::Value::Object(_) | serde_json::Value::Array(_))
    )
}

fn is_path(value: &str) -> bool {
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

fn is_command(value: &str) -> bool {
    if value.lines().count() != 1 {
        return false;
    }
    let (command, prompted) = strip_shell_prompt(value);
    let tokens = split_shell_tokens(command);
    let mut command_index = 0_usize;
    let mut first = tokens.first().copied().unwrap_or_default();

    while looks_like_environment_assignment(first) {
        command_index += 1;
        first = tokens.get(command_index).copied().unwrap_or_default();
    }
    if ["sudo", "doas", "nohup", "command"]
        .iter()
        .any(|wrapper| wrapper.eq_ignore_ascii_case(first))
    {
        command_index += 1;
        first = tokens.get(command_index).copied().unwrap_or_default();
    } else if first.eq_ignore_ascii_case("env") {
        command_index += 1;
        while tokens
            .get(command_index)
            .is_some_and(|token| looks_like_environment_assignment(token))
        {
            command_index += 1;
        }
        first = tokens.get(command_index).copied().unwrap_or_default();
    }

    first = first.trim_matches(['"', '\'']);
    if first.is_empty() {
        return false;
    }
    let normalized_first = first.to_ascii_lowercase();
    let executable_name = normalized_first
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(normalized_first.as_str());
    let token_count = tokens.len();
    let has_arguments = token_count > command_index + 1;
    let wrapped = command_index > 0;
    let has_shell_syntax = command.contains(" && ")
        || command.contains(" || ")
        || command.contains(" | ")
        || command.contains(" > ")
        || command.contains(" < ");
    let executable_path = normalized_first.starts_with("./")
        || normalized_first.starts_with(".\\")
        || executable_name.ends_with(".exe")
        || executable_name.ends_with(".cmd")
        || executable_name.ends_with(".bat")
        || executable_name.ends_with(".ps1")
        || executable_name.ends_with(".sh");

    if prompted && (token_count > 0 || has_shell_syntax) {
        return true;
    }
    if executable_path && (has_arguments || wrapped) {
        return true;
    }
    if is_powershell_cmdlet(first) {
        return token_count > 1 || prompted;
    }
    if wrapped
        && has_arguments
        && executable_name
            .chars()
            .any(|character| character.is_ascii_alphabetic())
        && executable_name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-')
        })
    {
        return true;
    }
    if !is_known_command(executable_name) {
        return false;
    }
    if normalized_first.contains(['/', '\\']) && !has_arguments && !wrapped && !prompted {
        return false;
    }

    if matches!(executable_name, "go" | "make" | "find") {
        let second = tokens.get(command_index + 1).copied().unwrap_or_default();
        return has_shell_syntax
            || second.starts_with('-')
            || matches!(
                (executable_name, second),
                (
                    "go",
                    "build"
                        | "clean"
                        | "env"
                        | "fmt"
                        | "generate"
                        | "get"
                        | "install"
                        | "mod"
                        | "run"
                        | "test"
                        | "tool"
                        | "version"
                        | "work"
                )
            );
    }
    token_count > 1 || has_shell_syntax || matches!(executable_name, "ls" | "pwd" | "clear")
}

fn is_command_block(value: &str) -> bool {
    if value.starts_with("#!") {
        return false;
    }
    let physical_lines: Vec<&str> = value
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if !(2..=64).contains(&physical_lines.len()) {
        return false;
    }

    let mut lines = Vec::new();
    let mut current = String::new();
    let mut has_continuation = false;
    for line in physical_lines {
        let continued = line.ends_with(['\\', '`']);
        let segment = if continued {
            has_continuation = true;
            line[..line.len() - 1].trim_end()
        } else {
            line
        };
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(segment);
        if !continued {
            lines.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }

    let mut command_lines = 0_usize;
    let mut shell_context_lines = 0_usize;
    for line in lines {
        if line.starts_with('#') || looks_like_environment_assignment(&line) {
            shell_context_lines += 1;
        } else if is_command(&line) {
            command_lines += 1;
        } else {
            return false;
        }
    }
    command_lines >= 2 || (command_lines == 1 && (shell_context_lines >= 1 || has_continuation))
}

fn split_shell_tokens(value: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = None;
    let mut quote = None;
    for (index, character) in value.char_indices() {
        if let Some(active_quote) = quote {
            if character == active_quote {
                quote = None;
            }
            continue;
        }
        if matches!(character, '"' | '\'') {
            start.get_or_insert(index);
            quote = Some(character);
        } else if character.is_whitespace() {
            if let Some(token_start) = start.take() {
                tokens.push(&value[token_start..index]);
            }
        } else {
            start.get_or_insert(index);
        }
    }
    if let Some(token_start) = start {
        tokens.push(&value[token_start..]);
    }
    tokens
}

fn strip_shell_prompt(value: &str) -> (&str, bool) {
    let trimmed = value.trim();
    if let Some(command) = trimmed
        .strip_prefix("$ ")
        .or_else(|| trimmed.strip_prefix("> "))
    {
        return (command.trim_start(), true);
    }
    if trimmed.starts_with("PS ") {
        if let Some((_, command)) = trimmed.split_once("> ") {
            return (command.trim_start(), true);
        }
    }
    (trimmed, false)
}

fn looks_like_environment_assignment(token: &str) -> bool {
    let Some((name, _)) = token.split_once('=') else {
        return false;
    };
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_powershell_cmdlet(token: &str) -> bool {
    const VERBS: [&str; 16] = [
        "Add", "Clear", "Copy", "Get", "Import", "Invoke", "Move", "New", "Remove", "Rename",
        "Restart", "Set", "Start", "Stop", "Test", "Update",
    ];
    token.split_once('-').is_some_and(|(verb, noun)| {
        VERBS.iter().any(|known| known.eq_ignore_ascii_case(verb))
            && !noun.is_empty()
            && noun.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    })
}

fn is_known_command(token: &str) -> bool {
    const COMMANDS: &[&str] = &[
        "apt",
        "apt-get",
        "awk",
        "bash",
        "brew",
        "bun",
        "cargo",
        "cat",
        "cd",
        "certbot",
        "chmod",
        "chown",
        "choco",
        "clear",
        "cmake",
        "cp",
        "curl",
        "deno",
        "dnf",
        "docker",
        "dotnet",
        "du",
        "echo",
        "find",
        "git",
        "go",
        "gradle",
        "grep",
        "head",
        "java",
        "javac",
        "journalctl",
        "kill",
        "killall",
        "kubectl",
        "less",
        "ln",
        "ls",
        "make",
        "mkdir",
        "mv",
        "node",
        "nginx",
        "npm",
        "npx",
        "perl",
        "php",
        "pip",
        "pip3",
        "pnpm",
        "powershell",
        "ps",
        "pwd",
        "pwsh",
        "python",
        "python3",
        "reboot",
        "rg",
        "rm",
        "rmdir",
        "rsync",
        "ruby",
        "rustc",
        "scp",
        "sed",
        "service",
        "sh",
        "ssh",
        "swift",
        "systemctl",
        "tail",
        "tar",
        "tee",
        "touch",
        "ufw",
        "uname",
        "unzip",
        "wget",
        "winget",
        "yarn",
        "xargs",
        "zsh",
    ];
    COMMANDS
        .iter()
        .any(|command| command.eq_ignore_ascii_case(token))
}

fn code_score(value: &str) -> u8 {
    let trimmed = value.trim();
    if trimmed.starts_with("```") {
        return 10;
    }
    if trimmed.starts_with("#!") && trimmed.lines().count() > 1 {
        return 9;
    }
    let upper = trimmed.to_ascii_uppercase();
    if [
        "SELECT ", "INSERT ", "UPDATE ", "DELETE ", "CREATE ", "ALTER ",
    ]
    .iter()
    .any(|prefix| upper.starts_with(prefix))
        && [" FROM ", " INTO ", " SET ", " TABLE "]
            .iter()
            .any(|token| upper.contains(token))
    {
        return 9;
    }
    if trimmed.starts_with("<!DOCTYPE")
        || (trimmed.starts_with('<')
            && trimmed.ends_with('>')
            && (trimmed.contains("</") || trimmed.contains("/>")))
    {
        return 9;
    }
    if is_strong_code_signature(trimmed) {
        return 8;
    }
    if trimmed.contains('{')
        && trimmed.contains('}')
        && trimmed.contains(':')
        && trimmed.contains(';')
    {
        return 8;
    }
    if is_known_code_call(trimmed) {
        return 8;
    }

    let lines: Vec<&str> = trimmed.lines().take(200).collect();
    let mut score = 0_u8;
    if lines.len() >= 2 {
        score += 1;
    }
    if lines.len() >= 4 {
        score += 1;
    }

    let mut statement_lines = 0_u8;
    let mut comment_lines = 0_u8;
    let mut declaration_lines = 0_u8;
    let mut assignment_lines = 0_u8;
    let mut strong_assignment_lines = 0_u8;
    let mut function_call_lines = 0_u8;
    for line in &lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.ends_with(';') {
            statement_lines = statement_lines.saturating_add(1);
        }
        if line.starts_with("//") || line.starts_with("/*") || line.starts_with('#') {
            comment_lines = comment_lines.saturating_add(1);
        }
        if starts_code_declaration(line) {
            declaration_lines = declaration_lines.saturating_add(1);
        }
        if let Some(right_hand_side) = code_assignment_value(line) {
            assignment_lines = assignment_lines.saturating_add(1);
            if has_code_value_signal(right_hand_side) {
                strong_assignment_lines = strong_assignment_lines.saturating_add(1);
            }
        }
        if looks_like_function_call(line) {
            function_call_lines = function_call_lines.saturating_add(1);
        }
    }
    if assignment_lines >= 2 && strong_assignment_lines >= 1 {
        return 8;
    }
    score += statement_lines.min(2);
    score += comment_lines.min(1);
    score += declaration_lines.saturating_mul(4).min(6);
    score += function_call_lines.saturating_mul(3).min(6);

    if trimmed.contains("=>") || trimmed.contains("::") {
        score += 2;
    }
    if trimmed.contains('{') && trimmed.contains('}') {
        score += 2;
    }
    if trimmed.contains(" = ")
        && (declaration_lines > 0
            || trimmed.contains("==")
            || trimmed.contains("=>")
            || trimmed.contains(';'))
    {
        score += 1;
    }
    if looks_like_function_call(trimmed) {
        score += 2;
    }
    score
}

fn code_assignment_value(line: &str) -> Option<&str> {
    if ["==", "!=", "<=", ">=", "=>"]
        .iter()
        .any(|operator| line.contains(operator))
    {
        return None;
    }
    let (left, right) = line.split_once('=')?;
    let left = ["const ", "let ", "var "]
        .iter()
        .find_map(|prefix| left.trim().strip_prefix(prefix))
        .unwrap_or_else(|| left.trim());
    if !is_identifier(left) {
        return None;
    }
    let right = right.trim().trim_end_matches(';').trim();
    (!right.is_empty()).then_some(right)
}

fn has_code_value_signal(value: &str) -> bool {
    value.starts_with(['"', '\'', '[', '{', '('])
        || value
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
        || matches!(value, "true" | "false" | "null" | "None" | "nil")
        || [" + ", " - ", " * ", " / ", " % "]
            .iter()
            .any(|operator| value.contains(operator))
        || value.contains(['(', '['])
}

fn is_strong_code_signature(value: &str) -> bool {
    let first = value.lines().next().unwrap_or_default().trim();
    (first.starts_with("def ") && first.contains('(') && first.ends_with(':'))
        || (first.starts_with("fn ") && first.contains('('))
        || (first.starts_with("func ") && first.contains('('))
        || (first.starts_with("function ") && first.contains('('))
        || (first.starts_with("class ") && value.contains('{'))
        || (first.starts_with("struct ") && value.contains('{'))
        || (["const ", "let ", "var "]
            .iter()
            .any(|prefix| first.starts_with(prefix))
            && first.contains(" = "))
        || looks_like_import_declaration(first)
        || first.starts_with("package main")
}

fn looks_like_import_declaration(line: &str) -> bool {
    let line = line.trim();
    if let Some(target) = line.strip_prefix("#include ") {
        let target = target.trim();
        return (target.starts_with('<') && target.ends_with('>'))
            || (target.starts_with('"') && target.ends_with('"'));
    }
    if let Some(rest) = line.strip_prefix("from ") {
        let Some((module, names)) = rest.split_once(" import ") else {
            return false;
        };
        return is_module_path(module.trim()) && is_import_list(names);
    }
    if let Some(rest) = line.strip_prefix("import ") {
        let rest = rest.trim();
        if rest.contains(['"', '\'']) {
            return true;
        }
        let rest = rest.strip_suffix(';').unwrap_or(rest).trim();
        let tokens: Vec<&str> = rest.split_whitespace().collect();
        return match tokens.as_slice() {
            [module] => is_module_path(module),
            [module, "as", alias] => is_module_path(module) && is_identifier(alias),
            _ => false,
        };
    }
    if let Some(rest) = line.strip_prefix("use ") {
        let Some(rest) = rest.strip_suffix(';') else {
            return false;
        };
        let rest = rest.trim();
        return !rest.is_empty()
            && !rest.contains(char::is_whitespace)
            && (is_module_path(rest) || rest.contains(['{', '}']));
    }
    false
}

fn is_module_path(value: &str) -> bool {
    !value.is_empty()
        && value.chars().any(|c| c.is_ascii_alphanumeric() || c == '_')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '/'))
}

fn is_import_list(value: &str) -> bool {
    let value = value.trim().trim_matches(['(', ')']);
    !value.is_empty()
        && value.split(',').all(|item| {
            let tokens: Vec<&str> = item.split_whitespace().collect();
            match tokens.as_slice() {
                ["*"] => true,
                [name] => is_identifier(name),
                [name, "as", alias] => is_identifier(name) && is_identifier(alias),
                _ => false,
            }
        })
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn is_known_code_call(value: &str) -> bool {
    const PREFIXES: [&str; 8] = [
        "console.log(",
        "fmt.Print",
        "print(",
        "printf(",
        "println!(",
        "System.out.print",
        "NSLog(",
        "assert_eq!(",
    ];
    PREFIXES.iter().any(|prefix| value.starts_with(prefix)) && value.ends_with([')', ';'])
}

fn starts_code_declaration(line: &str) -> bool {
    if looks_like_import_declaration(line) {
        return true;
    }
    const PREFIXES: [&str; 18] = [
        "class ",
        "const ",
        "def ",
        "enum ",
        "export ",
        "fn ",
        "func ",
        "function ",
        "impl ",
        "interface ",
        "let ",
        "mod ",
        "namespace ",
        "package ",
        "pub ",
        "struct ",
        "type ",
        "var ",
    ];
    PREFIXES.iter().any(|prefix| line.starts_with(prefix))
        && line.contains(['=', '(', '{', ':', ';'])
}

fn looks_like_function_call(value: &str) -> bool {
    if value.lines().count() != 1 || !value.ends_with([')', ';']) {
        return false;
    }
    let Some(open) = value.find('(') else {
        return false;
    };
    let prefix = value[..open].trim();
    !prefix.is_empty()
        && !prefix.contains(char::is_whitespace)
        && prefix
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '!' | '$'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn category(text: &str) -> &'static str {
        classify_text(text).category
    }

    #[test]
    fn classifies_deterministic_formats() {
        assert_eq!(category("https://example.com/docs?q=rust#intro"), "website");
        assert_eq!(category("http://localhost:5173/history"), "website");
        for website in [
            "www.example.com",
            "WWW.Example.XYZ/docs?q=rust#intro",
            "example.com",
            "sub.example.org/path",
            "baidu.com",
            "12306.cn",
            "github.io/project",
            "example.co.uk/docs",
            "example.com:8443/path",
            "localhost:5173",
            "192.168.1.1:8080/settings",
            "//example.com/docs",
        ] {
            assert_eq!(category(website), "website", "{website}");
        }
        assert_eq!(classify_text("https://example.com").confidence, 99);
        assert_eq!(classify_text("www.example.com").confidence, 97);
        assert_eq!(classify_text("example.com/docs").confidence, 96);
        assert_eq!(classify_text("example.com").confidence, 93);
        assert_eq!(
            category(r#"{"name":"TailSync","enabled":true}"#),
            "structured_data"
        );
        assert_eq!(category("[1, 2, 3]"), "structured_data");
        assert_eq!(category(r#"C:\Users\tester\notes.txt"#), "path");
        assert_eq!(
            category(r#"C:\Program Files (x86)\TailSync\TailSync.exe"#),
            "path"
        );
        assert_eq!(category("/Users/tester/Documents/notes.txt"), "path");
    }

    #[test]
    fn classifies_commands_and_code() {
        assert_eq!(category("git status --short"), "command");
        assert_eq!(category("Get-Content README.md"), "command");
        assert_eq!(category("get-content README.md"), "command");
        assert_eq!(category("docker compose up -d"), "command");
        assert_eq!(category("go test ./..."), "command");
        assert_eq!(
            category(r#"C:\Tools\formatter.exe --check source.rs"#),
            "command"
        );
        assert_eq!(
            category(r#""C:\Program Files\TailSync\tailsync.exe" --version"#),
            "command"
        );
        assert_eq!(category("/usr/bin/git status --short"), "command");
        let nginx_deploy =
            "sudo tar -xzf /tmp/tailsync-site-theme-switch-20260728.tar.gz -C /var/www/tailsync\n\
sudo chown -R www-data:www-data /var/www/tailsync\n\
sudo nginx -t\n\
sudo systemctl reload nginx";
        let deployment_classification = classify_text(nginx_deploy);
        assert_eq!(deployment_classification.category, "command");
        assert_eq!(deployment_classification.confidence, 96);
        assert_eq!(deployment_classification.categories(), vec!["command"]);
        assert_eq!(category("git status --short\nnpm run build"), "command");
        assert_eq!(
            category("sudo customctl restart\nsudo customctl status"),
            "command"
        );
        assert_eq!(
            category("docker run --rm \\\n+  -v /tmp/source:/source \\\n+  alpine:latest"),
            "command"
        );
        assert_eq!(
            category("const ids = items.map((item) => item.id);"),
            "code"
        );
        assert_eq!(
            category("def greet(name):\n    return f\"Hello {name}\""),
            "code"
        );
        assert_eq!(
            category("SELECT id, name FROM users WHERE active = 1"),
            "code"
        );
        assert_eq!(category("console.log(\"ready\");"), "code");
        assert_eq!(category("const url = \"https://example.com\";"), "code");
        assert_eq!(category("#!/bin/sh\ngit status"), "code");
        assert_eq!(category("import os"), "code");
        assert_eq!(category("import numpy as np"), "code");
        assert_eq!(category("from pathlib import Path"), "code");
        assert_eq!(category("use std::collections::HashMap;"), "code");
        assert_eq!(category("#include <stdio.h>"), "code");
        assert_eq!(category("x = 1\ny = x + 2\nprint(y)"), "code");
        assert_eq!(category("prepare()\nexecute()\ncleanup()"), "code");
    }

    #[test]
    fn conservative_rules_keep_prose_as_text() {
        assert_eq!(category("请打开 https://example.com 查看文档"), "text");
        assert_eq!(
            category("Please run git status and send me the result."),
            "text"
        );
        assert_eq!(category("let me know when you are ready"), "text");
        assert_eq!(category("class action lawsuits can take years"), "text");
        assert_eq!(category("import this document"), "text");
        assert_eq!(category("use this approach"), "text");
        assert_eq!(category("from here import this document"), "text");
        assert_eq!(category("go home now"), "text");
        assert_eq!(category("make this easier"), "text");
        assert_eq!(category("Name = Alice\nCity = Beijing"), "text");
        assert_eq!(category("git"), "text");
        assert_eq!(category("true"), "text");
        assert_eq!(category("{this is ordinary text}"), "text");
        for text in [
            "user@example.com",
            "Please visit example.com today",
            "example.com second-value",
            "README.md",
            "main.rs",
            "config.toml",
            "archive.zip",
            "object.method",
            "com.example.App",
            "1.2.3",
            "v1.2.3",
            "example",
            "www.example",
            ".example.com",
            "example..com",
            "-foo.com",
            "foo-.com",
            "example.com:abc",
            "example.com:99999",
            r#"example.com\docs"#,
            "example.invalid",
        ] {
            assert_eq!(category(text), "text", "{text}");
        }
        assert_eq!(category(r#"C:\Tools\formatter.exe"#), "path");
        assert_eq!(category("/usr/bin/ls"), "path");
    }

    #[test]
    fn only_scans_the_bounded_prefix() {
        let text = format!("{}\nconst value = 1;", "a".repeat(MAX_SAMPLE_BYTES));
        assert_eq!(category(&text), "text");

        let mut split_multibyte = "a".repeat(MAX_SAMPLE_BYTES - 1);
        split_multibyte.push('\u{4f60}');
        split_multibyte.push_str("\nconst value = 1;");
        assert_eq!(category(&split_multibyte), "text");

        let long_json = format!(r#"{{"value":"{}"}}"#, "a".repeat(MAX_SAMPLE_BYTES));
        assert_eq!(category(&long_json), "text");
    }

    #[test]
    fn exposes_only_supported_categories() {
        for category in CATEGORIES {
            assert!(is_known_category(category));
        }
        assert!(!is_known_category("unknown"));
    }
}
