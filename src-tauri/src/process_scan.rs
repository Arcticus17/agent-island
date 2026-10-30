//! Process matching, runtime facts, and status projection onto snapshots.

use crate::*;

pub(crate) fn keyword_for(name: &str) -> Option<String> {
    agent_defs()
        .iter()
        .find(|d| d.name == name)
        .map(|d| d.keyword.clone())
}

pub(crate) fn matching_processes<'a>(sys: &'a System, keyword: &str) -> Vec<&'a sysinfo::Process> {    sys.processes()
        .iter()
        .filter(|(_pid, process)| {
            let name = process.name().to_string_lossy().to_lowercase();
            let args = process
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().to_lowercase())
                .collect::<Vec<_>>();
            matches_agent_process(&name, &args, keyword)
        })
        .map(|(_pid, process)| process)
        .collect()
}

pub(crate) fn matches_agent_process(name: &str, args: &[String], keyword: &str) -> bool {
    let keyword = keyword.trim().to_ascii_lowercase();
    if keyword.is_empty() {
        return false;
    }
    // A Codex desktop app-server owns many tasks, not a single CLI session.
    // Never infer CLI ownership from arbitrary prompt text or a parent shell.
    if keyword == "codex" {
        if args.iter().skip(1).any(|arg| arg == "app-server") {
            return false;
        }
        if name.eq_ignore_ascii_case("codex.exe") || name.eq_ignore_ascii_case("codex") {
            return true;
        }
        if !matches!(name.to_ascii_lowercase().as_str(), "node" | "node.exe") {
            return false;
        }
        return args.get(1).is_some_and(|entrypoint| {
            let normalized = entrypoint.replace('\\', "/");
            normalized.ends_with("/@openai/codex/bin/codex.js")
        });
    }

    // Match the executable itself, never a word that merely appears in a
    // prompt, a grep pattern, or another process's command line. This was the
    // source of phantom "opencode"/"hermes" agents when a shell happened to
    // contain those words in its arguments.
    if executable_name_matches(name, &keyword) {
        return true;
    }

    if matches!(name.to_ascii_lowercase().as_str(), "node" | "node.exe") {
        return node_entrypoint_matches(args, &keyword);
    }

    // npm creates .cmd/.ps1 shims; a shim is launched through cmd.exe and the
    // real program name is the token after /C. Only inspect that token.
    if matches!(name.to_ascii_lowercase().as_str(), "cmd" | "cmd.exe") {
        let command = args.get(1).map(String::as_str).unwrap_or("");
        if command.eq_ignore_ascii_case("/c") {
            return args
                .get(2)
                .map(|arg| executable_name_matches(arg, &keyword))
                .unwrap_or(false);
        }
    }

    false
}

fn executable_name_matches(name: &str, keyword: &str) -> bool {
    let candidate = name
        .trim()
        .trim_matches('"')
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    let candidate = candidate
        .strip_suffix(".exe")
        .or_else(|| candidate.strip_suffix(".cmd"))
        .or_else(|| candidate.strip_suffix(".bat"))
        .or_else(|| candidate.strip_suffix(".ps1"))
        .unwrap_or(&candidate);
    candidate == keyword
}

fn node_entrypoint_matches(args: &[String], keyword: &str) -> bool {
    let Some(entrypoint) = args.get(1) else {
        return false;
    };
    let normalized = entrypoint
        .to_ascii_lowercase()
        .replace('\\', "/")
        .trim_matches('"')
        .to_string();
    let file = normalized.rsplit('/').next().unwrap_or("");
    let file_stem = file
        .strip_suffix(".js")
        .or_else(|| file.strip_suffix(".mjs"))
        .or_else(|| file.strip_suffix(".cjs"))
        .unwrap_or(file);
    // Only the script path is inspected. Prompt text and later arguments are
    // never evidence that this process is the agent.
    file_stem == keyword
        || normalized.contains(&format!("/{keyword}/"))
        || normalized.contains(&format!("/{keyword}-"))
        || normalized.contains(&format!("/@{keyword}/"))
}

#[cfg(test)]
mod process_classifier_tests {
    use super::{environment_agent_candidates_from, environment_program_paths_from, known_program_paths, known_program_paths_from, matches_agent_process, program_available};

    fn matches(name: &str, args: &[&str], keyword: &str) -> bool {
        matches_agent_process(
            name,
            &args.iter().map(|arg| arg.to_string()).collect::<Vec<_>>(),
            keyword,
        )
    }

    #[test]
    fn excludes_desktop_server_and_code_mode_helpers_from_cli() {
        assert!(!matches(
            "codex.exe",
            &["codex.exe", "app-server", "--listen", "stdio://"],
            "codex"
        ));
        assert!(!matches(
            "codex-code-mode-host.exe",
            &["codex-code-mode-host.exe"],
            "codex"
        ));
    }

    #[test]
    fn ignores_shell_prompts_and_unrelated_node_arguments() {
        assert!(!matches(
            "powershell.exe",
            &["powershell.exe", "-Command", "echo codex"],
            "codex"
        ));
        assert!(!matches(
            "node.exe",
            &["node.exe", "server.js", "codex"],
            "codex"
        ));
    }

    #[test]
    fn accepts_native_cli_and_official_node_entrypoint() {
        assert!(matches(
            "codex.exe",
            &["codex.exe", "resume", "session-id"],
            "codex"
        ));
        assert!(matches(
            "node.exe",
            &[
                "node.exe",
                r"C:\Users\中文 用户\node_modules\@openai\codex\bin\codex.js"
            ],
            "codex"
        ));
        assert!(!matches(
            "node.exe",
            &[
                "node.exe",
                "/node_modules/@openai/codex/bin/codex.js",
                "app-server"
            ],
            "codex"
        ));
    }

    #[test]
    fn preserves_custom_keyword_matching_by_executable_or_entrypoint() {
        assert!(matches(
            "custom-agent.exe",
            &["custom-agent.exe", "--serve"],
            "custom-agent"
        ));
        assert!(matches(
            "node.exe",
            &["node.exe", r"C:\tools\custom-agent\bin\index.js"],
            "custom-agent"
        ));
        // The old implementation matched any argument containing the keyword;
        // that turns prompts and shell commands into phantom agents.
        assert!(!matches(
            "powershell.exe",
            &["powershell.exe", "-Command", "custom-agent-probe"],
            "custom-agent"
        ));
        assert!(!matches(
            "node.exe",
            &["node.exe", "server.js", "custom-agent"],
            "custom-agent"
        ));
    }

    #[test]
    fn never_turns_prompt_text_into_open_code_or_hermes_processes() {
        assert!(!matches(
            "powershell.exe",
            &["powershell.exe", "-Command", "Get-Process opencode"],
            "opencode"
        ));
        assert!(!matches(
            "node.exe",
            &["node.exe", "server.js", "--prompt", "talk to hermes"],
            "hermes"
        ));
        assert!(matches(
            "opencode.exe",
            &["opencode.exe", "--port", "4096"],
            "opencode"
        ));
        assert!(matches(
            "node.exe",
            &["node.exe", r"D:\tools\opencode\bin\opencode.js"],
            "opencode"
        ));
    }

    #[test]
    fn accepts_cmd_shims_only_when_the_command_token_is_the_agent() {
        assert!(matches(
            "cmd.exe",
            &["cmd.exe", "/C", r"D:\node_global\claude.cmd", "--version"],
            "claude"
        ));
        assert!(!matches(
            "cmd.exe",
            &["cmd.exe", "/C", "echo", "claude"],
            "claude"
        ));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn detects_installed_cli_and_rejects_uninstalled_leftovers() {
        // 系统自带的 cmd 一定能解析到；随机名字不应被当成已安装。
        assert!(program_available("cmd"));
        assert!(!program_available("missing-agent-87a310"));
        assert!(known_program_paths("claude").iter().any(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().starts_with("claude"))
                .unwrap_or(false)
        }));
    }

    #[test]
    fn finds_the_npm_prefix_from_the_user_npmrc() {
        let root = std::env::temp_dir().join(format!(
            "agent-island-npmrc-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join(".npmrc"), "prefix=D:\\node_global\n").unwrap();
        let paths = known_program_paths_from("claude", Some(root.clone()), |_| None);
        assert!(
            paths.iter().any(|path| {
                path.to_string_lossy()
                    .eq_ignore_ascii_case("D:\\node_global\\claude.cmd")
            }),
            "custom npm prefix should be a candidate: {paths:?}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn finds_the_pi_managed_install_directory() {
        // Pi 的托管安装不在 PATH 的常规位置：~/.pi/agent/bin（pi.cmd / pi / pi.ps1）。
        let home = std::path::Path::new("C:\\Users\\me");
        let expected = home.join(".pi/agent/bin").join("pi.cmd");
        let paths = known_program_paths_from("pi", Some(home.to_path_buf()), |_| None);
        assert!(
            paths.iter().any(|path| path == &expected),
            "managed Pi install should be a candidate: {paths:?}"
        );
    }

    #[test]
    fn includes_scoop_and_volta_prefixes_from_the_environment() {
        let paths = known_program_paths_from("codex", None, |name| match name {
            "SCOOP" => Some(std::ffi::OsString::from("C:\\scoop")),
            "VOLTA_HOME" => Some(std::ffi::OsString::from("C:\\volta")),
            _ => None,
        });
        assert!(paths.iter().any(|path| {
            path.to_string_lossy()
                .eq_ignore_ascii_case("C:\\scoop\\shims\\codex.cmd")
        }));
        assert!(paths.iter().any(|path| {
            path.to_string_lossy()
                .eq_ignore_ascii_case("C:\\volta\\bin\\codex.cmd")
        }));
    }

    #[test]
    fn finds_agents_from_environment_variables() {
        let root = std::env::temp_dir().join(format!(
            "agent-island-env-agent-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("opencode").join("bin")).unwrap();
        let direct = root.join("claude.exe");
        std::fs::write(&direct, b"stub").unwrap();
        let nested = root.join("opencode").join("bin").join("opencode.cmd");
        std::fs::write(&nested, b"stub").unwrap();

        let found = environment_program_paths_from(
            "claude",
            vec![(
                std::ffi::OsString::from("CLAUDE_CODE_PATH"),
                std::ffi::OsString::from(direct.to_string_lossy().as_ref()),
            )],
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "CLAUDE_CODE_PATH");
        assert_eq!(found[0].1, direct);

        let found = environment_program_paths_from(
            "opencode",
            vec![(
                std::ffi::OsString::from("OPENCODE_HOME"),
                std::ffi::OsString::from(root.join("opencode").to_string_lossy().as_ref()),
            )],
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "OPENCODE_HOME");
        assert_eq!(found[0].1, nested);

        // A prompt-like value is not a path and must never become an agent.
        let found = environment_program_paths_from(
            "hermes",
            vec![(
                std::ffi::OsString::from("PROMPT"),
                std::ffi::OsString::from("please use hermes"),
            )],
        );
        assert!(found.is_empty());

        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn finds_unknown_agents_referenced_by_environment_directories() {
        let root = std::env::temp_dir().join(format!(
            "agent-island-env-candidate-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("my-agent").join("bin")).unwrap();
        let executable = root.join("my-agent").join("bin").join("my-agent.exe");
        std::fs::write(&executable, b"stub").unwrap();
        // Non-executable files must never be offered just because of the name.
        std::fs::write(root.join("my-agent").join("bin").join("agent-notes.json"), b"{}").unwrap();

        let found = environment_agent_candidates_from(vec![(
            std::ffi::OsString::from("MY_AGENT_HOME"),
            std::ffi::OsString::from(root.join("my-agent").to_string_lossy().as_ref()),
        )]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, "MY_AGENT_HOME");
        assert_eq!(found[0].1, executable);

        // Generic runtimes are not offered just because a variable points at them.
        std::fs::create_dir_all(root.join("java")).unwrap();
        std::fs::write(root.join("java").join("java.exe"), b"stub").unwrap();
        let found = environment_agent_candidates_from(vec![(
            std::ffi::OsString::from("JAVA_HOME"),
            std::ffi::OsString::from(root.join("java").to_string_lossy().as_ref()),
        )]);
        assert!(found.is_empty());

        // Temporary directories contain agent-island-*.err files; they are not agents.
        std::fs::write(root.join("agent-island-1.err"), b"stub").unwrap();
        let found = environment_agent_candidates_from(vec![(
            std::ffi::OsString::from("TEMP"),
            std::ffi::OsString::from(root.to_string_lossy().as_ref()),
        )]);
        assert!(found.is_empty());

        std::fs::remove_dir_all(root).ok();
    }
}

/// CLI 是否仍装在系统上：先查 PATH（Windows 用 `where.exe`），再兜底常见安装目录。
///
/// 只认可执行文件——卸载后残留的日志/会话记录**不算**存在，所以"下载过又卸载"
/// 的 Agent 不会再出现在岛上。
#[derive(Debug, Clone)]
pub(crate) struct ProgramResolution {
    pub path: PathBuf,
    /// path | env:NAME
    pub source: String,
}

static PROGRAM_CACHE: OnceLock<Mutex<HashMap<String, (Instant, Option<ProgramResolution>)>>> =
    OnceLock::new();

pub(crate) fn resolve_agent_program(program: &str) -> Option<PathBuf> {
    resolve_agent_program_source(program).map(|resolution| resolution.path)
}

pub(crate) fn resolve_agent_program_source(program: &str) -> Option<ProgramResolution> {
    let program = program.trim();
    if program.is_empty() {
        return None;
    }
    let cache = PROGRAM_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = program.to_lowercase();
    if let Ok(guard) = cache.lock() {
        if let Some((checked_at, value)) = guard.get(&key) {
            if checked_at.elapsed().as_secs() < 60 {
                return value.clone();
            }
        }
    }
    let value = resolve_program_path(program);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, (Instant::now(), value.clone()));
    }
    value
}

pub(crate) fn program_available(program: &str) -> bool {
    resolve_agent_program(program).is_some()
}

/// User-triggered "re-detect" must not wait for the 60s cache.
pub(crate) fn clear_program_cache() {
    if let Some(cache) = PROGRAM_CACHE.get() {
        if let Ok(mut guard) = cache.lock() {
            guard.clear();
        }
    }
}

fn resolve_program_path(program: &str) -> Option<ProgramResolution> {
    // Explicit paths are returned as-is, matching restart_program_available.
    if program.contains(['\\', '/']) || std::path::Path::new(program).is_absolute() {
        let path = PathBuf::from(program);
        return path.is_file().then_some(ProgramResolution {
            path,
            source: "path".to_string(),
        });
    }
    // Environment variables are an explicit user signal: check them before
    // generic install dirs so a custom location wins over an older PATH copy.
    if let Some((name, path)) = environment_program_paths(program).into_iter().next() {
        return Some(ProgramResolution {
            path,
            source: format!("env:{name}"),
        });
    }
    if let Some(path) = known_program_paths(program)
        .into_iter()
        .find(|candidate| candidate.is_file())
    {
        return Some(ProgramResolution {
            path,
            source: "path".to_string(),
        });
    }
    where_program_path(program).map(|path| ProgramResolution {
        path,
        source: "path".to_string(),
    })
}

fn program_extensions() -> &'static [&'static str] {
    if cfg!(target_os = "windows") {
        &[".exe", ".cmd", ".bat", ".ps1", ""]
    } else {
        &[""]
    }
}

/// Discover agent executables referenced by environment variables.
///
/// Examples:
/// - `CLAUDE_CODE_PATH=C:\tools\claude.exe` (direct file)
/// - `CLAUDE_CONFIG_DIR=C:\Users\me\.claude` containing `local\claude.exe`
/// - `OPENCODE_HOME=C:\tools\opencode` containing `bin\opencode.cmd`
///
/// Only existing files/directories are considered; prompt text, tokens and
/// other arbitrary values cannot become an agent.
fn environment_program_paths(program: &str) -> Vec<(String, PathBuf)> {
    environment_program_paths_from(program, std::env::vars_os())
}

/// Environment variables can also point at an agent we have never heard of
/// (for example `MY_AGENT_HOME=C:\tools\my-agent`). This scans only entries
/// with agent-like executable names and leaves them as **candidates** for the
/// "+" panel; it never adds or displays them automatically.
fn environment_agent_candidates() -> Vec<(String, PathBuf)> {
    environment_agent_candidates_from(std::env::vars_os())
}

fn environment_agent_candidates_from<I>(vars: I) -> Vec<(String, PathBuf)>
where
    I: IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
{
    const HINTS: &[&str] = &[
        "agent",
        "claude",
        "codex",
        "opencode",
        "hermes",
        "gemini",
        "aider",
        "cursor",
        "copilot",
        "qwen",
        "crush",
        "goose",
        "amp",
        "droid",
        "kilo",
        "auggie",
        "plandex",
        "openhands",
        "cline",
        "warp",
    ];
    fn has_hint(text: &str, hints: &[&str]) -> bool {
        hints.iter().any(|hint| text.contains(hint))
    }
    fn is_agent_executable(path: &std::path::Path, hints: &[&str]) -> bool {
        let extension = path
            .extension()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        // A directory scan is full of .json/.err/.dll files; never offer those
        // as agents just because their name contains "agent" or "claude".
        if !matches!(extension.as_str(), "exe" | "cmd" | "bat" | "ps1" | "com") {
            return false;
        }
        let stem = path
            .file_stem()
            .map(|value| value.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if stem.is_empty() || stem.len() > 64 {
            return false;
        }
        stem.split(['-', '_', '.', ' '])
            .any(|token| hints.contains(&token))
            || hints.iter().any(|hint| stem == *hint)
    }

    let mut found = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (name, value) in vars {
        let name = name.to_string_lossy().into_owned();
        let value_text = value.to_string_lossy().trim().trim_matches('"').to_string();
        if value_text.is_empty() || value_text.len() > 4096 {
            continue;
        }
        // Generic variables such as JAVA_HOME/TEMP/HOME point at directories
        // full of unrelated files. Only names (or values) mentioning an agent
        // are eligible; the executable filter above still applies.
        let name_lower = name.to_ascii_lowercase();
        let value_lower = value_text.to_ascii_lowercase();
        if !has_hint(&name_lower, HINTS) && !has_hint(&value_lower, HINTS) {
            continue;
        }
        let candidate = PathBuf::from(&value_text);
        if candidate.is_file() {
            if is_agent_executable(&candidate, HINTS)
                && seen.insert(candidate.to_string_lossy().to_lowercase())
            {
                found.push((name, candidate));
                if found.len() >= 20 {
                    break;
                }
            }
            continue;
        }
        if !candidate.is_dir() {
            continue;
        }
        for subdir in ["", "bin", "local", "node_modules/.bin"] {
            let directory = if subdir.is_empty() {
                candidate.clone()
            } else {
                candidate.join(subdir)
            };
            let Ok(entries) = std::fs::read_dir(&directory) else {
                continue;
            };
            for entry in entries.flatten().take(200) {
                let path = entry.path();
                if !path.is_file() || !is_agent_executable(&path, HINTS) {
                    continue;
                }
                if seen.insert(path.to_string_lossy().to_lowercase()) {
                    found.push((name.clone(), path));
                    if found.len() >= 20 {
                        return found;
                    }
                }
            }
        }
    }
    found
}

fn environment_program_paths_from<I>(program: &str, vars: I) -> Vec<(String, PathBuf)>
where
    I: IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
{
    let program_lower = program.trim().to_ascii_lowercase();
    if program_lower.is_empty() {
        return Vec::new();
    }
    let mut found = Vec::new();
    for (name, value) in vars {
        let name_text = name.to_string_lossy().into_owned();
        if name_text.eq_ignore_ascii_case("PATH") {
            continue;
        }
        let value_text = value.to_string_lossy().trim().trim_matches('"').to_string();
        if value_text.is_empty() || value_text.len() > 4096 {
            continue;
        }
        let candidate = PathBuf::from(&value_text);
        if candidate.is_file() {
            if candidate
                .file_stem()
                .map(|stem| stem.to_string_lossy().eq_ignore_ascii_case(&program_lower))
                .unwrap_or(false)
            {
                found.push((name_text, candidate));
            }
            continue;
        }
        if !candidate.is_dir() {
            continue;
        }
        // Directory-valued vars are scanned only when the variable name or
        // path mentions the agent, keeping the check cheap and deterministic.
        if !name_text.to_ascii_lowercase().contains(&program_lower)
            && !value_text.to_ascii_lowercase().contains(&program_lower)
        {
            continue;
        }
        for subdir in ["", "bin", "local", "node_modules/.bin"] {
            let directory = if subdir.is_empty() {
                candidate.clone()
            } else {
                candidate.join(subdir)
            };
            for extension in program_extensions() {
                let path = directory.join(format!("{program_lower}{extension}"));
                if path.is_file() {
                    found.push((name_text.clone(), path));
                }
            }
        }
    }
    found
}

#[cfg(target_os = "windows")]
fn where_program_path(program: &str) -> Option<PathBuf> {
    let output = crate::commands::quiet_command("where.exe")
        .arg(program)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .find(|path| path.is_file())
}

#[cfg(not(target_os = "windows"))]
fn where_program_path(_program: &str) -> Option<PathBuf> {
    None
}

/// `where.exe` 只看 PATH；这里补上各包管理器/安装器的常见落点。
///
/// 关键点：npm 的全局 prefix 可以由 `~/.npmrc` 改成任意目录（本机就是
/// `D:\node_global`）。只查 `%APPDATA%\npm` 会在自定义 prefix 的机器上
/// 把已安装的 Claude/Codex 误判为"不存在"，所以这里显式读取 `.npmrc`
/// 的 `prefix=`，并覆盖 Scoop/Chocolatey/Volta/Bun/pnpm 等常见位置。
fn known_program_paths(program: &str) -> Vec<PathBuf> {
    known_program_paths_from(program, home_dir(), |name| std::env::var_os(name))
}

fn known_program_paths_from<F>(
    program: &str,
    home: Option<PathBuf>,
    env: F,
) -> Vec<PathBuf>
where
    F: Fn(&str) -> Option<std::ffi::OsString>,
{
    let program = program.trim();
    if program.is_empty() {
        return Vec::new();
    }
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Some(home) = home.as_deref() {
        for relative in [
            ".local/bin",
            ".cargo/bin",
            ".bun/bin",
            ".opencode/bin",
            ".claude/local",
            ".pi/agent/bin",
            ".npm-global/bin",
            ".hermes/bin",
            ".volta/bin",
            ".local/share/pnpm",
            "scoop/shims",
        ] {
            bases.push(home.join(relative));
        }
        if let Some(prefix) = npmrc_prefix(home) {
            bases.push(prefix);
        }
    }
    for (variable, subdir) in [
        ("NPM_CONFIG_PREFIX", ""),
        ("npm_config_prefix", ""),
        ("PNPM_HOME", ""),
        ("SCOOP", "shims"),
        ("SCOOP_GLOBAL", "shims"),
        ("ChocolateyInstall", "bin"),
        ("VOLTA_HOME", "bin"),
        ("BUN_INSTALL", "bin"),
    ] {
        if let Some(value) = env(variable) {
            if !value.is_empty() {
                let base = PathBuf::from(value);
                bases.push(if subdir.is_empty() {
                    base
                } else {
                    base.join(subdir)
                });
            }
        }
    }
    for variable in ["APPDATA", "LOCALAPPDATA"] {
        if let Some(base) = env(variable) {
            if !base.is_empty() {
                bases.push(PathBuf::from(base).join("npm"));
            }
        }
    }
    for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(base) = env(variable) {
            if !base.is_empty() {
                bases.push(PathBuf::from(base).join("nodejs"));
            }
        }
    }
    if let Some(paths) = env("PATH") {
        bases.extend(std::env::split_paths(&paths));
    }

    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();
    // On Windows prefer a real launcher over npm's extensionless POSIX shim;
    // CreateProcess cannot run the extensionless script.
    let extensions: &[&str] = if cfg!(target_os = "windows") {
        &[".exe", ".cmd", ".bat", ".ps1", ""]
    } else {
        &[""]
    };
    for base in bases {
        for directory in [base.clone(), base.join("node_modules").join(".bin")] {
            for extension in extensions {
                let candidate = directory.join(format!("{program}{extension}"));
                let key = candidate.to_string_lossy().to_lowercase();
                if seen.insert(key) {
                    candidates.push(candidate);
                }
            }
        }
    }
    candidates
}

/// 读取 npm 用户配置里的 `prefix=`（本机是 `D:\node_global`）。
fn npmrc_prefix(home: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(home.join(".npmrc")).ok()?;
    for line in text.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix("prefix=") else {
            continue;
        };
        let value = value.trim().trim_matches('"').trim_matches('\'');
        if value.is_empty() {
            return None;
        }
        let path = PathBuf::from(value);
        return Some(if path.is_absolute() { path } else { home.join(path) });
    }
    None
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AgentDetection {
    pub id: String,
    pub name: String,
    pub keyword: String,
    pub installed: bool,
    pub running: bool,
    pub process_count: usize,
    /// 实际解析到的可执行文件；不存在时为 null。
    pub path: Option<String>,
    /// process | path | env:NAME | missing
    pub source: String,
}

/// Diagnostics: explain exactly why each configured agent is shown or hidden.
#[tauri::command]
pub(crate) fn detect_agents(state: tauri::State<AppState>) -> Vec<AgentDetection> {
    let mut sys = state.sys.lock_recover();
    refresh_agent_processes(&mut sys);
    agent_defs()
        .into_iter()
        .map(|def| {
            let processes = matching_processes(&sys, &def.keyword);
            let resolution = resolve_agent_program_source(def.program());
            let path = resolution.as_ref().map(|value| value.path.clone());
            let running = !processes.is_empty();
            let installed = path.is_some() || running;
            let source = if running {
                "process".to_string()
            } else {
                resolution
                    .map(|value| value.source)
                    .unwrap_or_else(|| "missing".to_string())
            };
            AgentDetection {
                id: if def.log_kind.is_empty() {
                    def.keyword.clone()
                } else {
                    def.log_kind.clone()
                },
                name: def.name.clone(),
                keyword: def.keyword.clone(),
                installed,
                running,
                process_count: processes.len(),
                path: path.map(|value| value.to_string_lossy().into_owned()),
                source,
            }
        })
        .collect()
}

/// Same as `detect_agents`, but bypasses the 60s program cache.
#[tauri::command]
pub(crate) fn refresh_agent_detection(state: tauri::State<AppState>) -> Vec<AgentDetection> {
    clear_program_cache();
    detect_agents(state)
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AgentCandidate {
    pub name: String,
    pub program: String,
    pub description: String,
    pub installed: bool,
    pub configured: bool,
    pub path: Option<String>,
    pub keyword: String,
}

/// "检测本机" candidates: configured agents plus curated CLI agents whose
/// executable actually resolves. Nothing is auto-added; the user chooses.
#[tauri::command]
pub(crate) fn detect_agent_candidates() -> Vec<AgentCandidate> {
    let defs = crate::scan_agent_defs();
    let mut candidates = Vec::new();
    for def in &defs {
        let path = resolve_agent_program(def.program());
        let detected = def.is_detected();
        candidates.push(AgentCandidate {
            name: def.name.clone(),
            program: def.program().to_string(),
            description: if detected {
                "自动发现（本机已安装）".to_string()
            } else {
                "已在岛内配置".to_string()
            },
            installed: path.is_some(),
            configured: !detected,
            path: path.map(|value| value.to_string_lossy().into_owned()),
            keyword: def.keyword.clone(),
        });
    }
    for entry in crate::agent_catalog::CATALOG {
        let already = defs.iter().any(|def| {
            def.name.eq_ignore_ascii_case(entry.name)
                || def.program().eq_ignore_ascii_case(entry.program)
                || def.keyword.eq_ignore_ascii_case(entry.program)
        });
        if already {
            continue;
        }
        let Some(path) = resolve_agent_program(entry.program) else {
            continue;
        };
        candidates.push(AgentCandidate {
            name: entry.name.to_string(),
            program: entry.program.to_string(),
            description: entry.description.to_string(),
            installed: true,
            configured: false,
            path: Some(path.to_string_lossy().into_owned()),
            keyword: entry.program.to_string(),
        });
    }
    for (env_name, path) in environment_agent_candidates() {
        let keyword = path
            .file_stem()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default();
        if keyword.is_empty() {
            continue;
        }
        let already = candidates.iter().any(|candidate| {
            candidate.keyword.eq_ignore_ascii_case(&keyword)
                || candidate
                    .path
                    .as_deref()
                    .is_some_and(|value| Path::new(value) == path)
        });
        if already {
            continue;
        }
        candidates.push(AgentCandidate {
            name: keyword.clone(),
            program: path.to_string_lossy().into_owned(),
            description: format!("环境变量 {env_name}"),
            installed: true,
            configured: false,
            path: Some(path.to_string_lossy().into_owned()),
            keyword,
        });
    }
    candidates
}

#[tauri::command]
pub(crate) fn add_agent_definition(name: String, program: String) -> Result<(), String> {
    let name = name.trim();
    let program = program.trim();
    if name.is_empty() {
        return Err("agent_name_empty".to_string());
    }
    if program.is_empty() {
        return Err("program_empty".to_string());
    }
    if name.chars().any(char::is_control) || program.chars().any(char::is_control) {
        return Err("invalid_agent_definition".to_string());
    }
    if program.contains(['&', '|', '<', '>', '^', '%', '!', '\r', '\n', '"']) {
        return Err("program_contains_unsafe_characters".to_string());
    }
    let resolved = resolve_agent_program(program).ok_or_else(|| "program_not_found".to_string())?;
    let program_text = resolved.to_string_lossy().into_owned();
    let keyword = resolved
        .file_stem()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "program_not_found".to_string())?;

    let mut defs = load_agent_defs();
    if defs.iter().any(|def| {
        def.name.eq_ignore_ascii_case(name)
            || def.keyword.eq_ignore_ascii_case(&keyword)
            || def.program().eq_ignore_ascii_case(&program_text)
    }) {
        return Err("agent_already_exists".to_string());
    }
    defs.push(AgentDef {
        name: name.to_string(),
        keyword,
        log_kind: String::new(),
        program: program_text,
        resume_args: Vec::new(),
        send_args: Vec::new(),
        source: "custom".into(),
    });
    save_agent_defs(&defs)?;
    reload_agent_defs();
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProcessObservation {
    pub(crate) pid: u32,
    pub(crate) project_path: Option<String>,
    pub(crate) started_at_ms: u64,
}

pub(crate) fn process_fact_from_observations(
    agent_id: &str,
    name: &str,
    observations: &[ProcessObservation],
) -> interface::snapshot::ProcessFact {
    let project_path = observations
        .first()
        .and_then(|observation| observation.project_path.clone())
        .filter(|first| {
            let normalized = normalize_process_path(first);
            observations.iter().all(|observation| {
                observation
                    .project_path
                    .as_deref()
                    .map(normalize_process_path)
                    .as_deref()
                    == Some(normalized.as_str())
            })
        });

    interface::snapshot::ProcessFact {
        name: name.into(),
        identity: domain::ProcessIdentity {
            agent_id: agent_id.into(),
            project_path,
            process_ids: observations
                .iter()
                .map(|observation| observation.pid)
                .collect(),
            started_at_ms: observations
                .iter()
                .map(|observation| observation.started_at_ms)
                .max()
                .unwrap_or(0),
        },
        process_state: if observations.is_empty() {
            domain::ProcessState::Stopped
        } else {
            domain::ProcessState::Running
        },
        activity: interface::snapshot::ProcessActivity::Unknown,
    }
}

pub(crate) fn normalize_process_path(path: &str) -> String {
    path.trim_end_matches(['/', '\\'])
        .replace('/', "\\")
        .to_lowercase()
}

/// Process-fact placeholder for the compatibility `AgentInfo` fields.
///
/// The island's authoritative status is derived from structured events in
/// `interface::snapshot`; `apply_snapshot_projection` writes that back onto
/// `AgentInfo`. Text heuristics used to feed these fields and were removed so a
/// message body can never decide a status again.
pub(crate) fn legacy_process_status(
    total_cpu: f32,
    total_mem_mb: f32,
    busy: bool,
) -> (&'static str, domain::DisplayStatus) {
    if busy {
        ("working", domain::DisplayStatus::Working)
    } else if total_cpu > 80.0 || total_mem_mb > 1024.0 {
        ("high_load", domain::DisplayStatus::Working)
    } else {
        ("idle", domain::DisplayStatus::Idle)
    }
}

pub(crate) fn scan_agents(
    sys: &mut System,
    session: &mut SessionState,
    pty_roots: &std::collections::HashSet<u32>,
    launch_facts: &std::collections::HashMap<String, u64>,
    bindings: &std::collections::HashMap<String, crate::bindings::BoundSession>,
    hooked_sessions: &std::collections::HashMap<String, crate::hooked::HookedSession>,
    managed_roots: &std::collections::HashSet<u32>,
) -> (
    Vec<AgentInfo>,
    interface::snapshot::AgentViewSnapshot,
    interface::diagnostics::DiagnosticSnapshot,
    bool,
) {
    let _scan_started = Instant::now();
    refresh_agent_processes(sys);
    // pid → 父 pid：判断 Agent 是不是岛内终端（PTY）的后代
    let parents = launcher::parent_map(
        sys.processes()
            .iter()
            .map(|(pid, process)| (pid.as_u32(), process.parent().map(|parent| parent.as_u32()))),
    );
    let _after_processes = Instant::now();
    // 团队任务派生的 Claude 进程不是"用户自己的 Agent"，不能出现在主岛快照里。
    let excluded: std::collections::HashSet<u32> = if managed_roots.is_empty() {
        std::collections::HashSet::new()
    } else {
        sys.processes()
            .keys()
            .map(|pid| pid.as_u32())
            .filter(|pid| {
                managed_roots.contains(pid)
                    || launcher::descends_from(*pid, &parents, managed_roots)
            })
            .collect()
    };
    let now = Instant::now();
    let delta = now.duration_since(session.last_poll).as_secs();
    session.last_poll = now;
    let mut agents: Vec<AgentInfo> = Vec::new();
    let mut process_facts = Vec::new();
    let mut session_candidates = Vec::new();
    let mut diagnostic_records = Vec::new();
    let mut acquisition_issues = Vec::new();
    let mut stats_changed = false;

    session.index_cache.begin_scan();
    for def in crate::scan_agent_defs() {
        let agent_name = &def.name;
        let agent_id = if def.log_kind.is_empty() {
            def.keyword.as_str()
        } else {
            def.log_kind.as_str()
        };
        let processes: Vec<&sysinfo::Process> = matching_processes(sys, &def.keyword)
            .into_iter()
            .filter(|process| !excluded.contains(&process.pid().as_u32()))
            .collect();
        if processes.is_empty() && !program_available(def.program()) {
            // 系统上已经找不到这个 CLI（可能是装了又卸载）：不再出现在岛上，
            // 也不再扫它残留的会话日志——残留日志不能证明 Agent 还在。
            session.activity.remove(agent_name);
            session.runtime_start.remove(agent_name);
            continue;
        }
        let observations: Vec<ProcessObservation> = processes
            .iter()
            .map(|process| ProcessObservation {
                pid: process.pid().as_u32(),
                project_path: process
                    .cwd()
                    .map(|path| path.to_string_lossy().into_owned()),
                started_at_ms: process.start_time().saturating_mul(1_000),
            })
            .collect();
        let mut process_fact = process_fact_from_observations(agent_id, agent_name, &observations);
        let process_cwd = process_fact.identity.project_path.clone();
        let mut session_scan = build_session_scan_for_process_cached(
            agent_name,
            Some(&process_fact.identity),
            &mut session.index_cache,
        );
        // 岛内启动过这个 Agent：只保留启动之后有动静的会话，避免挑到上一次的旧会话
        if let Some(&launched_at) = launch_facts.get(agent_id) {
            let kept: Vec<interface::snapshot::SessionCandidate> =
                launcher::keep_after_launch(&session_scan.candidates, launched_at, |candidate| {
                    candidate.identity.last_event_at_ms
                })
                .into_iter()
                .cloned()
                .collect();
            session_scan.candidates = kept;
        }
        if session_scan.acquisition == AcquisitionCompleteness::Incomplete {
            acquisition_issues.push(
                session_scan
                    .diagnostics
                    .first()
                    .map(|record| record.view.clone())
                    .unwrap_or_else(|| {
                        interface::diagnostics::DiagnosticView::new(
                            agent_id,
                            domain::DataIssue::AcquisitionIncomplete,
                            domain::Freshness {
                                observed_at_ms: epoch_millis(),
                                stale: true,
                            },
                            None,
                            0,
                            0,
                        )
                    }),
            );
        }
        let session_list = std::mem::take(&mut session_scan.legacy_sessions);
        session_candidates.append(&mut session_scan.candidates);
        diagnostic_records.append(&mut session_scan.diagnostics);
        if processes.is_empty() {
            process_facts.push(process_fact);
            session.activity.remove(agent_name);
            session.runtime_start.remove(agent_name);
            let first = session_list.first();
            agents.push(AgentInfo {
                id: agent_id.to_string(),
                name: agent_name.clone(),
                status: "stopped".to_string(),
                display_status: domain::DisplayStatus::Stopped,
                pid: None,
                cpu: None,
                memory: None,
                uptime: None,
                cwd: first.and_then(|s| s.cwd.clone()),
                sessions: 0,
                last_active_secs: None,
                log_path: first.and_then(|s| s.log_path.clone()),
                recent_output: first.map(|s| s.recent_output.clone()).unwrap_or_default(),
                current_file: first.and_then(|s| s.current_file.clone()),
                log_status: first.and_then(|s| s.log_status.clone()),
                alert: first.and_then(|s| s.alert.clone()),
                can_restart: session.commands.contains_key(agent_name),
                stats: session.stats.get(agent_name).cloned(),
                session_count: session_list.len(),
                session_list,
                active_session: None,
                history_sessions: None,
                freshness: domain::Freshness {
                    observed_at_ms: epoch_millis(),
                    stale: false,
                },
                usage: usage_for(agent_name),
            });
            continue;
        }

        let main = processes
            .iter()
            .max_by_key(|process| {
                let exe_matches = process
                    .exe()
                    .and_then(|path| path.file_name())
                    .map(|file| file.to_string_lossy().to_lowercase().contains(&def.keyword))
                    .unwrap_or(false);
                (exe_matches, process.cpu_usage() as u32)
            })
            .copied();

        let total_cpu: f32 = processes.iter().map(|p| p.cpu_usage()).sum();
        let total_mem_mb: f32 = processes
            .iter()
            .map(|p| p.memory() as f32 / (1024.0 * 1024.0))
            .sum();
        let uptime = processes.iter().map(|p| p.run_time()).max().unwrap_or(0);
        let log = session_list.first().map(|s| LogSnapshot {
            path: s.log_path.clone().unwrap_or_default(),
            recent: s.recent_output.clone(),
            file: s.current_file.clone(),
            cwd: s.cwd.clone(),
            log_status: s.log_status.clone(),
            alert: s.alert.clone(),
        });
        let cwd = process_cwd;

        let entry = session
            .activity
            .entry(agent_name.clone())
            .or_insert_with(|| ActivityState {
                last_cpu: total_cpu,
                last_active: now,
            });
        let cpu_delta = (total_cpu - entry.last_cpu).abs();
        let busy = total_cpu > 2.0 || cpu_delta > 1.0;
        if busy {
            entry.last_active = now;
        }
        entry.last_cpu = total_cpu;
        process_fact.activity = if busy {
            interface::snapshot::ProcessActivity::Busy {
                observed_at_ms: epoch_millis(),
            }
        } else {
            interface::snapshot::ProcessActivity::Idle {
                observed_at_ms: epoch_millis(),
            }
        };
        let idle_secs = now.duration_since(entry.last_active).as_secs();
        let log_status = log.as_ref().and_then(|l| l.log_status.clone());
        let alert = log.as_ref().and_then(|l| l.alert.clone());
        // Compatibility placeholder only. Structured events are the single source
        // of the authoritative status and `apply_snapshot_projection` overwrites
        // both fields as soon as the agent has a snapshot view. Message text never
        // decides a status here.
        let (status, placeholder_display_status) =
            legacy_process_status(total_cpu, total_mem_mb, busy);

        if let Some(main_proc) = main {
            let cmd_vec: Vec<String> = main_proc
                .cmd()
                .iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect();
            if !cmd_vec.is_empty() {
                session.commands.insert(
                    agent_name.clone(),
                    AgentCommand {
                        cwd: cwd.clone(),
                        cmd: cmd_vec,
                    },
                );
            }
        }

        let stats = session.stats.entry(agent_name.clone()).or_default();
        let today = today_key();
        let day_stats = session
            .daily
            .entry(today)
            .or_default()
            .entry(agent_name.clone())
            .or_default();
        if session.runtime_start.contains_key(agent_name) {
            stats.total_seconds += delta;
            day_stats.total_seconds += delta;
        } else {
            session.runtime_start.insert(agent_name.clone(), now);
        }
        let stats_snapshot = stats.clone();

        agents.push(AgentInfo {
            id: agent_id.to_string(),
            name: agent_name.clone(),
            status: status.to_string(),
            display_status: placeholder_display_status,
            pid: main.map(|p| p.pid().as_u32()),
            cpu: Some(total_cpu),
            memory: Some(total_mem_mb),
            uptime: Some(uptime),
            cwd,
            sessions: processes.len(),
            last_active_secs: Some(if busy { 0 } else { idle_secs }),
            log_path: log.as_ref().map(|l| l.path.clone()),
            recent_output: log.as_ref().map(|l| l.recent.clone()).unwrap_or_default(),
            current_file: log.as_ref().and_then(|l| l.file.clone()),
            log_status,
            alert,
            can_restart: session.commands.contains_key(agent_name),
            stats: Some(stats_snapshot),
            session_count: session_list.len(),
            session_list,
            active_session: None,
            history_sessions: None,
            freshness: domain::Freshness {
                observed_at_ms: epoch_millis(),
                stale: false,
            },
            usage: usage_for(agent_name),
        });
        process_facts.push(process_fact);
    }
    let generated_at_ms = epoch_millis();
    session.index_cache.finish_scan();
    if std::env::var_os("AGENT_ISLAND_SCAN_DEBUG").is_some() {
        eprintln!(
            "[scan] processes={}ms agents={}ms",
            _after_processes.duration_since(_scan_started).as_millis(),
            _after_processes.elapsed().as_millis()
        );
    }
    let mut snapshot = interface::snapshot::build_snapshot_with_acquisition(
        generated_at_ms,
        &process_facts,
        &session_candidates,
        &acquisition_issues,
    );
    attach_runtime_metadata_to_snapshot(&mut snapshot, &agents);
    // 权威绑定：Agent 自报（hooks）> 岛内 PTY 后代（launched）> 手动绑定 > 日志推断
    for agent in &mut snapshot.agents {
        let pids: Vec<u32> = process_facts
            .iter()
            .find(|fact| fact.identity.agent_id == agent.id)
            .map(|fact| fact.identity.process_ids.clone())
            .unwrap_or_default();
        let launched = matches!(
            launcher::binding_for_pids(&pids, &parents, pty_roots),
            launcher::SessionBinding::Launched
        );
        // Agent 自己报过身份：直接把活动会话固定到它说的那个 session
        let mut hooked = false;
        if let Some(report) = hooked_sessions.get(&agent.id) {
            let matches_candidate = session_candidates.iter().any(|candidate| {
                candidate.identity.agent_id == agent.id
                    && candidate.identity.session_id == report.session_id
            });
            if matches_candidate {
                if let Some(candidate) = session_candidates.iter().find(|candidate| {
                    candidate.identity.agent_id == agent.id
                        && candidate.identity.session_id == report.session_id
                }) {
                    agent.active_session = Some(candidate.view.clone());
                }
                hooked = true;
            } else if agent
                .active_session
                .as_ref()
                .is_some_and(|session| session.id == report.session_id)
            {
                hooked = true;
            }
        }
        // 用户手动绑定的会话（岛外启动也能确认身份）
        let mut manual = false;
        if let Some(bound) = bindings.get(&agent.id) {
            if let Some(candidate) = session_candidates.iter().find(|candidate| {
                candidate.identity.agent_id == agent.id
                    && candidate.identity.session_id == bound.session_id
            }) {
                agent.active_session = Some(candidate.view.clone());
                manual = true;
            }
        }
        agent.binding = launcher::choose_binding(launched, hooked, manual);
    }
    for agent in &snapshot.agents {
        if let Some(view) = &agent.diagnostic {
            diagnostic_records.push(interface::diagnostics::DiagnosticRecord {
                view: view.clone(),
                session_id: None,
                project_path: process_facts
                    .iter()
                    .find(|fact| fact.identity.agent_id == agent.id)
                    .and_then(|fact| fact.identity.project_path.clone()),
                message_length: 0,
            });
        }
    }
    let diagnostics = interface::diagnostics::DiagnosticSnapshot {
        generated_at_ms,
        records: diagnostic_records,
    };
    apply_snapshot_projection(&mut agents, &snapshot);
    stats_changed |= update_terminal_statistics_from_snapshot(
        &mut session.stats,
        &mut session.daily,
        &today_key(),
        &snapshot,
    );
    for agent in &mut agents {
        agent.stats = session.stats.get(&agent.name).cloned();
    }
    (agents, snapshot, diagnostics, stats_changed)
}

pub(crate) fn attach_runtime_metadata_to_snapshot(
    snapshot: &mut interface::snapshot::AgentViewSnapshot,
    agents: &[AgentInfo],
) {
    for view in &mut snapshot.agents {
        let agent = agents
            .iter()
            .find(|agent| agent.id == view.id)
            .or_else(|| agents.iter().find(|agent| agent.name == view.name));
        view.can_restart = agent.is_some_and(|agent| agent.can_restart);
        view.usage = agent.and_then(|agent| agent.usage.clone());
    }
}

pub(crate) fn refresh_agent_processes(sys: &mut System) {
    sys.refresh_processes_specifics(ProcessesToUpdate::All, true, agent_process_refresh_kind());
}

pub(crate) fn agent_process_refresh_kind() -> ProcessRefreshKind {
    ProcessRefreshKind::nothing()
        .with_cmd(UpdateKind::Always)
        .with_cwd(UpdateKind::Always)
        .with_exe(UpdateKind::Always)
        .with_cpu()
        .with_memory()
}

pub(crate) fn legacy_status(display_status: domain::DisplayStatus) -> &'static str {
    match display_status {
        domain::DisplayStatus::Stopped => "stopped",
        domain::DisplayStatus::Idle => "idle",
        domain::DisplayStatus::Working => "working",
        domain::DisplayStatus::Done => "done",
        domain::DisplayStatus::Error => "error",
        domain::DisplayStatus::Waiting => "waiting",
    }
}

pub(crate) fn legacy_session(session: &interface::snapshot::SessionView) -> AgentSession {
    AgentSession {
        id: session.id.clone(),
        name: session.name.clone(),
        cwd: session.cwd.clone(),
        log_path: session.log_path.clone(),
        recent_output: session.recent_output.clone(),
        current_file: session.current_file.clone(),
        log_status: session.log_status.clone(),
        alert: session.alert.clone(),
    }
}

pub(crate) fn apply_snapshot_projection(
    agents: &mut [AgentInfo],
    snapshot: &interface::snapshot::AgentViewSnapshot,
) {
    for agent in agents {
        let Some(view) = snapshot.agents.iter().find(|view| view.id == agent.id) else {
            continue;
        };
        agent.id = view.id.clone();
        agent.status = legacy_status(view.display_status).into();
        agent.display_status = view.display_status;
        agent.active_session = view.active_session.clone();
        agent.history_sessions = Some(view.history_sessions.clone());
        agent.freshness = view.freshness;
        agent.session_list = view
            .active_session
            .as_ref()
            .map(legacy_session)
            .into_iter()
            .collect();
    }
}

pub(crate) fn update_terminal_statistics_from_snapshot(
    stats: &mut HashMap<String, AgentStats>,
    daily: &mut HashMap<String, HashMap<String, AgentStats>>,
    day: &str,
    snapshot: &interface::snapshot::AgentViewSnapshot,
) -> bool {
    let mut changed = false;
    for agent in &snapshot.agents {
        if agent.freshness.stale || agent.state.process != domain::ProcessState::Running {
            continue;
        }
        let (Some(session), Some(turn), Some(result_at_ms)) = (
            agent.active_session.as_ref(),
            agent.active_turn.as_ref(),
            agent.state.result_at_ms,
        ) else {
            continue;
        };
        if turn.agent_id != agent.id || turn.session_id != session.id {
            continue;
        }
        let status = match agent.state.turn {
            domain::TurnState::Succeeded => "done",
            domain::TurnState::Failed => "error",
            _ => continue,
        };
        let transition_key = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{result_at_ms}",
            agent.id, turn.session_id, turn.turn_id, status,
        );
        let transition_hash = terminal_key_hash(&transition_key);
        let agent_stats = stats.entry(agent.name.clone()).or_default();
        if agent_stats
            .recent_terminal_hashes
            .iter()
            .any(|hash| hash == &transition_hash)
        {
            continue;
        }
        let day_stats = daily
            .entry(day.into())
            .or_default()
            .entry(agent.name.clone())
            .or_default();
        match status {
            "done" => {
                agent_stats.done_count += 1;
                day_stats.done_count += 1;
            }
            "error" => {
                agent_stats.error_count += 1;
                day_stats.error_count += 1;
            }
            _ => unreachable!(),
        }
        agent_stats.last_status = Some(status.into());
        remember_terminal_hash(agent_stats, transition_hash);
        changed = true;
    }
    changed
}





