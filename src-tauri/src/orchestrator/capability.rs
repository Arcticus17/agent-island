//! 团队任务的能力与凭据预检。
//!
//! 产品原则是"不给假答案"：没装、没登录、后端不可达的 Agent 不能出现在
//! "可派发"列表里装作能用。这里只做本地、无网络、可缓存的检查；
//! 真正的执行失败仍由 runner 报回具体原因。

use crate::*;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentCapability {
    pub id: String,
    pub name: String,
    pub program: String,
    pub installed: bool,
    pub can_plan: bool,
    pub can_work: bool,
    pub available: bool,
    /// 面向用户的说明：为什么可用/不可用。
    pub detail: String,
}

/// Claude 的 npm 安装目录里真正的 exe 优先于 `.cmd` shim。
/// shim 经过 cmd.exe 时，任务 prompt 里的 `%` 等字符可能被二次展开。
pub(crate) fn claude_program() -> Option<PathBuf> {
    let resolved = resolve_agent_program("claude")?;
    let is_exe = resolved
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("exe"));
    if is_exe {
        return Some(resolved);
    }
    let parent = resolved.parent()?;
    let candidates = [
        parent
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code")
            .join("bin")
            .join("claude.exe"),
        parent.join("node_modules").join(".bin").join("claude.exe"),
    ];
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .or(Some(resolved))
}

/// Codex CLI 的 npm shim 不是真正的 exe；优先找 @openai/codex 自带的
/// 原生可执行文件，避免经过 cmd.exe 导致长 prompt 和引号被二次展开。
pub(crate) fn codex_program() -> Option<PathBuf> {
    let resolved = resolve_agent_program("codex")?;
    if resolved
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("exe"))
    {
        return Some(resolved);
    }
    let parent = resolved.parent()?;
    let candidates = [
        parent
            .join("node_modules")
            .join("@openai")
            .join("codex")
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64")
            .join("vendor")
            .join("x86_64-pc-windows-msvc")
            .join("bin")
            .join("codex.exe"),
        parent
            .join("node_modules")
            .join("@openai")
            .join("codex-win32-x64")
            .join("vendor")
            .join("x86_64-pc-windows-msvc")
            .join("bin")
            .join("codex.exe"),
        parent
            .join("node_modules")
            .join("@openai")
            .join("codex")
            .join("vendor")
            .join("x86_64-pc-windows-msvc")
            .join("bin")
            .join("codex.exe"),
    ];
    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .or(Some(resolved))
}

fn codex_has_auth() -> bool {
    home_dir()
        .map(|home| home.join(".codex").join("auth.json").is_file())
        .unwrap_or(false)
}

fn settings_env_files() -> Vec<PathBuf> {
    let Some(home) = home_dir() else {
        return Vec::new();
    };
    // settings.json 是 Claude Code 的标准文件；setting.json 是本机兼容端点
    // 配置常用的旧文件名，两个都读，标准文件后读、优先覆盖。
    vec![
        home.join(".claude").join("setting.json"),
        home.join(".claude").join("settings.json"),
    ]
}

/// 只返回需要注入/覆盖的环境变量，不把整个进程环境复制一遍。
pub(crate) fn claude_settings_env() -> HashMap<String, String> {
    let mut env = HashMap::new();
    for path in settings_env_files() {
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(entries) = value.get("env").and_then(Value::as_object) else {
            continue;
        };
        for (key, value) in entries {
            if let Some(text) = value.as_str() {
                env.insert(key.clone(), text.to_string());
            }
        }
    }
    env
}

fn has_credentials(settings_env: &HashMap<String, String>) -> bool {
    [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
    ]
    .iter()
    .any(|key| {
        std::env::var(key)
            .ok()
            .filter(|value| !value.trim().is_empty())
            .is_some()
            || settings_env
                .get(*key)
                .is_some_and(|value| !value.trim().is_empty())
    })
}

fn capability(
    id: &str,
    name: &str,
    program: String,
    installed: bool,
    can_plan: bool,
    can_work: bool,
    available: bool,
    detail: impl Into<String>,
) -> AgentCapability {
    AgentCapability {
        id: id.to_string(),
        name: name.to_string(),
        program,
        installed,
        can_plan,
        can_work,
        available,
        detail: detail.into(),
    }
}

/// 当前团队任务可用的 Agent。顺序固定，前端不用猜。
pub(crate) fn capabilities() -> Vec<AgentCapability> {
    let defs = agent_defs();
    let mut result = Vec::new();

    let claude_installed = claude_program().is_some();
    let env = claude_settings_env();
    let claude_ready = claude_installed && has_credentials(&env);
    result.push(capability(
        "claude",
        "Claude Code",
        claude_program()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "claude".to_string()),
        claude_installed,
        true,
        true,
        claude_ready,
        if claude_ready {
            "可用：主 Agent 规划 + 次级 Agent 执行"
        } else if claude_installed {
            "已安装，但没找到 Claude 凭据；请在 ~/.claude/settings.json 的 env 或环境变量里配置 ANTHROPIC_API_KEY"
        } else {
            "未安装：找不到 claude 可执行文件"
        },
    ));

    let codex_installed = codex_program().is_some();
    let codex_ready = codex_installed && codex_has_auth();
    result.push(capability(
        "codex",
        "Codex CLI",
        codex_program()
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|| "codex".to_string()),
        codex_installed,
        true,
        true,
        codex_ready,
        if codex_ready {
            "可用：Codex 主 Agent 规划 + 无头 worker；workspace-write 沙箱，无岛内审批；不返回 USD 成本"
        } else if codex_installed {
            "已安装，但没找到 Codex 登录凭据；请先在本机运行 codex 登录"
        } else {
            "未安装：找不到 codex 可执行文件"
        },
    ));

    for (id, name, hint) in [
        (
            "opencode",
            "OpenCode",
            "已安装，但团队任务尚未接入这个 Agent",
        ),
        ("hermes", "Hermes", "已安装，但团队任务尚未接入这个 Agent"),
    ] {
        let def = defs.iter().find(|def| {
            def.log_kind.eq_ignore_ascii_case(id) || def.keyword.eq_ignore_ascii_case(id)
        });
        let installed = def
            .and_then(|def| resolve_agent_program(def.program()))
            .is_some();
        result.push(capability(
            id,
            name,
            def.map(|def| def.program().to_string())
                .unwrap_or_else(|| id.to_string()),
            installed,
            false,
            false,
            false,
            if installed {
                hint.to_string()
            } else {
                "未安装：找不到可执行文件".to_string()
            },
        ));
    }

    result
}

/// 规划只允许真正可用的执行 Agent；主 Agent 由用户选择。
pub(crate) fn available_agent_ids() -> Vec<String> {
    capabilities()
        .into_iter()
        .filter(|capability| capability.available && capability.can_work)
        .map(|capability| capability.id)
        .collect()
}

/// 可作为主 Agent 的项：既要有规划能力，也要当前可用。
pub(crate) fn available_planner_ids() -> Vec<String> {
    capabilities()
        .into_iter()
        .filter(|capability| capability.available && capability.can_plan)
        .map(|capability| capability.id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatchable_agents_have_work_capability() {
        let caps = capabilities();
        assert!(caps.iter().any(|capability| capability.id == "claude"));
        assert!(caps
            .iter()
            .filter(|capability| capability.available)
            .all(|capability| capability.can_work));
    }

    #[test]
    fn every_capability_has_a_reason() {
        for capability in capabilities() {
            assert!(
                !capability.detail.trim().is_empty(),
                "{} 缺少说明",
                capability.id
            );
            assert!(
                !capability.can_plan || capability.can_work,
                "{} 能规划却不能干活",
                capability.id
            );
        }
    }
}
