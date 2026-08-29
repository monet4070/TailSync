pub(super) fn code_score(value: &str) -> u8 {
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
