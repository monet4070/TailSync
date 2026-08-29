pub(super) fn is_command(value: &str) -> bool {
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

pub(super) fn is_command_block(value: &str) -> bool {
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
