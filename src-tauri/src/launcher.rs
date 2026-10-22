//! 启动者绑定（Launcher binding）的判定核心。
//!
//! 岛内置终端用 `portable-pty` 启动 shell，shell 再启动 Agent。于是：
//! 从 Agent 进程沿父进程链向上走，若命中某个 PTY 根 pid，就能**确定**这个 Agent 是
//! "在我们终端里跑起来的"——不依赖日志、不依赖 Agent 配合，也不需要猜。
//!
//! 这里只做纯函数判定，进程表由调用方（process_scan）提供，便于单测。

use std::collections::{HashMap, HashSet};

/// 绑定可信度。序列化后前端直接显示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionBinding {
    /// 岛内的终端（PTY）后代进程：我们看着它出生
    Launched,
    /// Agent 通过 hooks 自报身份（S4 接入后才会产生）
    #[allow(dead_code)]
    Hooked,
    /// 只能靠日志/进程推断
    Inferred,
}

impl Default for SessionBinding {
    fn default() -> Self {
        Self::Inferred
    }
}

impl SessionBinding {
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn label(self) -> &'static str {
        match self {
            Self::Launched => "已确认",
            Self::Hooked => "已确认（hooks）",
            Self::Inferred => "推测",
        }
    }
}

/// pid → 父 pid 的快照，用来做祖先遍历。
pub(crate) fn parent_map(entries: impl IntoIterator<Item = (u32, Option<u32>)>) -> HashMap<u32, u32> {
    entries
        .into_iter()
        .filter_map(|(pid, parent)| parent.map(|parent| (pid, parent)))
        .collect()
}

/// 沿父链向上找，是否经过 `roots` 中任意一个 pid。
///
/// 防御两件事：进程表里的环（父指向自己/互相指向）和异常深的链。
pub(crate) fn descends_from(
    pid: u32,
    parents: &HashMap<u32, u32>,
    roots: &HashSet<u32>,
) -> bool {
    const MAX_DEPTH: usize = 64;
    if roots.is_empty() {
        return false;
    }
    let mut current = pid;
    let mut visited = HashSet::new();
    for _ in 0..MAX_DEPTH {
        if roots.contains(&current) {
            return true;
        }
        if !visited.insert(current) {
            // 环：已经见过这个 pid，继续走没有意义
            return false;
        }
        match parents.get(&current) {
            Some(&parent) if parent != current => current = parent,
            _ => return false,
        }
    }
    false
}

/// 给定某个 Agent 的所有 pid，返回最强的绑定。
pub(crate) fn binding_for_pids(
    pids: &[u32],
    parents: &HashMap<u32, u32>,
    pty_roots: &HashSet<u32>,
) -> SessionBinding {
    if pids.iter().any(|pid| descends_from(*pid, parents, pty_roots)) {
        SessionBinding::Launched
    } else {
        SessionBinding::Inferred
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parents(entries: &[(u32, Option<u32>)]) -> HashMap<u32, u32> {
        parent_map(entries.iter().copied())
    }

    fn roots(ids: &[u32]) -> HashSet<u32> {
        ids.iter().copied().collect()
    }

    #[test]
    fn detects_a_direct_and_a_deep_descendant() {
        // 100(shell) <- 200(claude) <- 300(node)
        let map = parents(&[(100, Some(1)), (200, Some(100)), (300, Some(200))]);
        assert!(descends_from(200, &map, &roots(&[100])));
        assert!(descends_from(300, &map, &roots(&[100])));
        assert!(!descends_from(200, &map, &roots(&[999])));
    }

    #[test]
    fn a_root_itself_counts() {
        let map = parents(&[(100, Some(1))]);
        assert!(descends_from(100, &map, &roots(&[100])));
    }

    #[test]
    fn survives_cycles_and_missing_parents() {
        // 200 <-> 300 互相指
        let map = parents(&[(200, Some(300)), (300, Some(200))]);
        assert!(!descends_from(200, &map, &roots(&[100])));
        // 父 pid 不在表里（进程已退出）
        let partial = parents(&[(400, Some(999))]);
        assert!(!descends_from(400, &partial, &roots(&[100])));
        // 自己是自己的父
        let self_parent = parents(&[(500, Some(500))]);
        assert!(!descends_from(500, &self_parent, &roots(&[100])));
    }

    #[test]
    fn picks_the_strongest_binding() {
        let map = parents(&[(200, Some(100)), (201, Some(1))]);
        let pty = roots(&[100]);
        assert_eq!(binding_for_pids(&[201, 200], &map, &pty), SessionBinding::Launched);
        assert_eq!(binding_for_pids(&[201], &map, &pty), SessionBinding::Inferred);
        assert_eq!(binding_for_pids(&[], &map, &pty), SessionBinding::Inferred);
        // 没有终端时不应误判
        assert_eq!(binding_for_pids(&[200], &map, &roots(&[])), SessionBinding::Inferred);
    }

    #[test]
    fn labels_are_human_readable() {
        assert_eq!(SessionBinding::Launched.label(), "已确认");
        assert_eq!(SessionBinding::Inferred.label(), "推测");
        assert_eq!(SessionBinding::default(), SessionBinding::Inferred);
    }
}

