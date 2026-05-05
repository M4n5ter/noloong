use crate::process::shell_executable_name;
use std::path::Path;

const GIT_GLOBAL_OPTIONS_WITH_VALUE: &[&str] = &[
    "-C",
    "-c",
    "--config-env",
    "--exec-path",
    "--git-dir",
    "--namespace",
    "--super-prefix",
    "--work-tree",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CommandSafety {
    Safe,
    Dangerous,
    Unknown,
}

pub(crate) fn classify_host_command(command: &str, shell: Option<&str>) -> CommandSafety {
    if shell.is_some_and(is_unsupported_shell) {
        return CommandSafety::Unknown;
    }
    classify_script(command)
}

fn classify_script(script: &str) -> CommandSafety {
    let Some(commands) = parse_plain_commands(script) else {
        return CommandSafety::Unknown;
    };
    if commands.is_empty() {
        return CommandSafety::Unknown;
    }
    let mut saw_unknown = false;
    for command in commands {
        match classify_words(&command) {
            CommandSafety::Safe => {}
            CommandSafety::Dangerous => return CommandSafety::Dangerous,
            CommandSafety::Unknown => saw_unknown = true,
        }
    }
    if saw_unknown {
        CommandSafety::Unknown
    } else {
        CommandSafety::Safe
    }
}

fn classify_words(words: &[String]) -> CommandSafety {
    if words.is_empty() || has_env_assignment_prefix(words) {
        return CommandSafety::Unknown;
    }
    if is_shell_wrapper(words) {
        return classify_script(&words[2]);
    }
    if is_dangerous_words(words) {
        return CommandSafety::Dangerous;
    }
    if is_safe_words(words) {
        CommandSafety::Safe
    } else {
        CommandSafety::Unknown
    }
}

fn is_unsupported_shell(shell: &str) -> bool {
    let shell_name = shell_executable_name(shell);
    shell_name == "cmd"
        || shell_name == "cmd.exe"
        || shell_name == "powershell"
        || shell_name == "powershell.exe"
        || shell_name == "pwsh"
        || shell_name == "pwsh.exe"
}

fn is_shell_wrapper(words: &[String]) -> bool {
    words.len() == 3
        && matches!(
            executable_name(&words[0]),
            "sh" | "bash" | "zsh" | "dash" | "ash"
        )
        && matches!(words[1].as_str(), "-c" | "-lc")
}

fn is_dangerous_words(words: &[String]) -> bool {
    let Some(command) = words.first().map(|word| executable_name(word)) else {
        return false;
    };
    match command {
        "rm" => matches!(words.get(1).map(String::as_str), Some("-f" | "-rf" | "-fr")),
        "sudo" => is_dangerous_words(&words[1..]),
        _ => false,
    }
}

fn is_safe_words(words: &[String]) -> bool {
    let Some(command) = words.first().map(|word| executable_name(word)) else {
        return false;
    };
    match command {
        "cat" | "pwd" | "ls" | "grep" | "head" | "tail" | "wc" => true,
        "rg" => is_safe_ripgrep(words),
        "sed" => is_safe_sed(words),
        "git" => is_safe_git(words),
        _ => false,
    }
}

fn is_safe_ripgrep(words: &[String]) -> bool {
    !words.iter().skip(1).any(|arg| {
        matches!(
            arg.as_str(),
            "--pre" | "--hostname-bin" | "--search-zip" | "-z"
        ) || arg.starts_with("--pre=")
            || arg.starts_with("--hostname-bin=")
    })
}

fn is_safe_sed(words: &[String]) -> bool {
    matches!(
        words,
        [cmd, flag, range] | [cmd, flag, range, _]
            if executable_name(cmd) == "sed" && flag == "-n" && is_valid_sed_print_range(range)
    )
}

fn is_valid_sed_print_range(value: &str) -> bool {
    let Some(value) = value.strip_suffix('p') else {
        return false;
    };
    if value.is_empty() {
        return false;
    }
    value
        .split(',')
        .all(|part| !part.is_empty() && part.chars().all(|ch| ch.is_ascii_digit()))
}

fn is_safe_git(words: &[String]) -> bool {
    if words
        .iter()
        .skip(1)
        .any(|arg| git_global_option_requires_prompt(arg))
    {
        return false;
    }
    let Some((subcommand_index, subcommand)) =
        find_git_subcommand(words, &["status", "log", "diff", "show", "branch"])
    else {
        return false;
    };
    let args = &words[subcommand_index + 1..];
    match subcommand {
        "status" | "log" | "diff" | "show" => git_args_are_read_only(args),
        "branch" => git_args_are_read_only(args) && git_branch_args_are_read_only(args),
        _ => false,
    }
}

fn git_args_are_read_only(args: &[String]) -> bool {
    !args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--output" | "--ext-diff" | "--textconv" | "--exec" | "--paginate"
        ) || arg.starts_with("--output=")
            || arg.starts_with("--exec=")
    })
}

fn git_branch_args_are_read_only(args: &[String]) -> bool {
    if args.is_empty() {
        return true;
    }
    let mut saw_read_only_flag = false;
    for arg in args {
        match arg.as_str() {
            "--list" | "-l" | "--show-current" | "-a" | "--all" | "-r" | "--remotes" | "-v"
            | "-vv" | "--verbose" => saw_read_only_flag = true,
            value if value.starts_with("--format=") => saw_read_only_flag = true,
            _ => return false,
        }
    }
    saw_read_only_flag
}

fn git_global_option_requires_prompt(arg: &str) -> bool {
    git_global_option_has_separate_value(arg) || git_global_option_has_inline_value(arg)
}

fn find_git_subcommand<'a>(words: &'a [String], subcommands: &[&str]) -> Option<(usize, &'a str)> {
    if words.first().map(|word| executable_name(word)) != Some("git") {
        return None;
    }
    let mut skip_next = false;
    for (index, arg) in words.iter().enumerate().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if git_global_option_has_inline_value(arg) {
            continue;
        }
        if git_global_option_has_separate_value(arg) {
            skip_next = true;
            continue;
        }
        if arg == "--" || arg.starts_with('-') {
            continue;
        }
        if subcommands.contains(&arg.as_str()) {
            return Some((index, arg));
        }
        return None;
    }
    None
}

fn git_global_option_has_separate_value(arg: &str) -> bool {
    GIT_GLOBAL_OPTIONS_WITH_VALUE.contains(&arg)
}

fn git_global_option_has_inline_value(arg: &str) -> bool {
    GIT_GLOBAL_OPTIONS_WITH_VALUE
        .iter()
        .any(|option| git_global_option_matches_inline_value(arg, option))
}

fn git_global_option_matches_inline_value(arg: &str, option: &str) -> bool {
    let Some(rest) = arg.strip_prefix(option) else {
        return false;
    };
    if option.starts_with("--") {
        rest.starts_with('=')
    } else {
        !rest.is_empty()
    }
}

fn parse_plain_commands(script: &str) -> Option<Vec<Vec<String>>> {
    let mut commands = Vec::new();
    let mut current = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut chars = script.chars().peekable();
    while let Some(ch) = chars.next() {
        if matches!(ch, '$' | '`' | '<' | '>' | '\n' | '\r') {
            return None;
        }
        match quote {
            Some(quote_ch) if ch == quote_ch => {
                quote = None;
            }
            Some(_) => {
                if matches!(ch, '*' | '?' | '[' | ']') {
                    return None;
                }
                word.push(ch);
            }
            None => match ch {
                '\'' | '"' => quote = Some(ch),
                '\\' => {
                    let escaped = chars.next()?;
                    if matches!(escaped, '$' | '`' | '<' | '>' | '\n' | '\r') {
                        return None;
                    }
                    word.push(escaped);
                }
                ' ' | '\t' => finish_word(&mut current, &mut word),
                ';' => finish_command(&mut commands, &mut current, &mut word)?,
                '|' => {
                    let _ = chars.next_if_eq(&'|');
                    finish_command(&mut commands, &mut current, &mut word)?;
                }
                '&' => {
                    if chars.next_if_eq(&'&').is_some() {
                        finish_command(&mut commands, &mut current, &mut word)?;
                    } else {
                        return None;
                    }
                }
                '*' | '?' | '[' | ']' => return None,
                _ => word.push(ch),
            },
        }
    }
    if quote.is_some() {
        return None;
    }
    finish_word(&mut current, &mut word);
    if !current.is_empty() {
        commands.push(current);
    }
    Some(commands)
}

fn finish_word(current: &mut Vec<String>, word: &mut String) {
    if !word.is_empty() {
        current.push(std::mem::take(word));
    }
}

fn finish_command(
    commands: &mut Vec<Vec<String>>,
    current: &mut Vec<String>,
    word: &mut String,
) -> Option<()> {
    finish_word(current, word);
    if current.is_empty() {
        return None;
    }
    commands.push(std::mem::take(current));
    Some(())
}

fn has_env_assignment_prefix(words: &[String]) -> bool {
    words
        .first()
        .is_some_and(|word| word.contains('=') && !word.starts_with('-'))
}

fn executable_name(raw: &str) -> &str {
    Path::new(raw)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_safe_read_only_commands() {
        for command in [
            "pwd",
            "ls -la",
            "rg foo src",
            "grep -R foo src",
            "head -n 20 Cargo.toml",
            "tail -n 20 Cargo.toml",
            "wc -l Cargo.toml",
            "sed -n '1,10p' Cargo.toml",
            "git status --short",
            "git log --oneline",
            "git diff",
            "git show HEAD:Cargo.toml",
            "git branch --show-current",
            "rg foo src | head -n 20",
            "pwd || ls",
        ] {
            assert_eq!(
                classify_host_command(command, Some("sh")),
                CommandSafety::Safe
            );
        }
    }

    #[test]
    fn classifies_shell_wrappers() {
        assert_eq!(
            classify_host_command("bash -lc 'pwd && rg foo src'", Some("sh")),
            CommandSafety::Safe
        );
        assert_eq!(
            classify_host_command("rm -rf target", Some("bash")),
            CommandSafety::Dangerous
        );
    }

    #[test]
    fn rejects_unsupported_syntax_as_unknown() {
        for command in [
            "FOO=bar rg foo",
            "rg foo > out.txt",
            "python - <<'PY'",
            "echo $(pwd)",
            "cat *.rs",
            "git -C /tmp status",
            "git --git-dir=.git status",
        ] {
            assert_eq!(
                classify_host_command(command, Some("sh")),
                CommandSafety::Unknown
            );
        }
    }

    #[test]
    fn classifies_dangerous_commands() {
        for command in ["rm -f file", "rm -rf target", "sudo rm -rf /tmp/x"] {
            assert_eq!(
                classify_host_command(command, Some("sh")),
                CommandSafety::Dangerous
            );
        }
    }
}
