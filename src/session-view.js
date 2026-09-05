const owns = (value, key) => Object.prototype.hasOwnProperty.call(value ?? {}, key);

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

export function overviewRows(agents) {
  const rows = [];
  const seen = new Set();

  for (const agent of agents ?? []) {
    const hasProjection = owns(agent, "active_session") || owns(agent, "history_sessions");
    const sessions = hasProjection
      ? [agent.active_session, ...(agent.history_sessions ?? [])]
      : (agent.session_list ?? []);
    const agentId = agent.id ?? agent.name;

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
