import type { AgentView, AgentViewSnapshot } from "../bridge/types";

/** Choose a default view, without reinterpreting logs or changing backend status. */
export function preferredAgent(snapshot: AgentViewSnapshot, currentId: string | null): AgentView | null {
  const fresh = snapshot.agents.filter((agent) => !agent.freshness.stale);
  const working = fresh.filter((agent) =>
    agent.state.process === "running" && agent.display_status === "working",
  );
  if (working.length) {
    return working.find((agent) => agent.id === currentId) ?? working[0];
  }

  const completed = fresh.filter((agent) =>
    (agent.state.turn === "succeeded" || agent.state.turn === "failed") &&
    agent.state.result_at_ms !== null &&
    Number.isSafeInteger(agent.state.result_at_ms) &&
    agent.state.result_at_ms >= 0 &&
    agent.state.result_at_ms <= snapshot.generated_at_ms + 5_000,
  );
  if (completed.length) {
    const latest = Math.max(...completed.map((agent) => agent.state.result_at_ms!));
    const tied = completed.filter((agent) => agent.state.result_at_ms === latest);
    return tied.find((agent) => agent.id === currentId) ?? tied[0];
  }
  return fresh.find((agent) => agent.id === currentId) ?? fresh[0]
    ?? snapshot.agents.find((agent) => agent.id === currentId) ?? snapshot.agents[0] ?? null;
}
