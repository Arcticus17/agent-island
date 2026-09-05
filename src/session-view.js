const owns = (value, key) => Object.prototype.hasOwnProperty.call(value ?? {}, key);

export function agentKey(agent, index = 0) {
  const id = String(agent?.id ?? "").trim();
  return id || `legacy:${index}:${agent?.name ?? "agent"}`;
}

export function sessionKey(agent, session, agentIndex = 0) {
  return `${agentKey(agent, agentIndex)}::${session?.id || "no-active"}`;
}

export function restoreAgentIndex(agents, key, fallbackIndex = 0) {
  const index = (agents ?? []).findIndex((agent, agentIndex) => agentKey(agent, agentIndex) === key);
  if (index >= 0) return index;
  if (!agents?.length) return -1;
  return Math.min(Math.max(fallbackIndex, 0), agents.length - 1);
}

export function islandSession(agent) {
  return agent?.active_session ?? null;
}

export function statusFor(agent, session) {
  return session?.display_status
    ?? session?.log_status
    ?? agent?.display_status
    ?? agent?.status
    ?? "idle";
}

function notificationKind(previousStatus, nextStatus) {
  if (nextStatus === "error") return "error";
  if (nextStatus === "waiting") return "waiting";
  const activeBefore = ["working", "high_load", "waiting", "idle", "running"]
    .includes(previousStatus);
  if (nextStatus === "done" && activeBefore) return "done";
  return null;
}

export function snapshotTransitionDecision(agent, previousStatus, nextStatus) {
  if (agent?.freshness?.stale === true) {
    return {
      recordStatus: false,
      notificationKind: null,
      statusFlash: false,
      errorFlash: false,
      autoJump: false,
    };
  }

  const changed = previousStatus !== nextStatus;
  const kind = previousStatus === undefined || !changed
    ? null
    : notificationKind(previousStatus, nextStatus);
  return {
    recordStatus: true,
    notificationKind: kind,
    statusFlash: changed,
    errorFlash: nextStatus === "error" && previousStatus !== "error",
    autoJump: kind === "error" || kind === "waiting",
  };
}

export function overviewRows(agents) {
  const rows = [];
  const seen = new Set();

  for (const [agentIndex, agent] of (agents ?? []).entries()) {
    const hasProjection = owns(agent, "active_session") || owns(agent, "history_sessions");
    const sessions = hasProjection
      ? [agent.active_session, ...(agent.history_sessions ?? [])]
      : (agent.session_list ?? []);
    const agentId = agentKey(agent, agentIndex);

    for (const session of sessions) {
      if (!session?.id) continue;
      const id = `${agentId}::${session.id}`;
      if (seen.has(id)) continue;
      seen.add(id);

      rows.push({
        agent,
        session,
        id,
        name: session.name || agent.name,
        status: statusFor(agent, session),
        output: session.recent_output?.length
          ? session.recent_output.join("\n")
          : "暂无输出",
        cwd: session.cwd ?? null,
        file: session.current_file ?? null,
      });
    }
  }

  return rows;
}
