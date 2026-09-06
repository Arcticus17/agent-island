import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

import {
  agentKey,
  agentIndexesFor,
  freshErrorIndex,
  hookNotificationTarget,
  islandSession,
  isSnapshotFresh,
  markRetainedAgentsStale,
  notificationGroupKey,
  overviewRows,
  restoreAgentIndex,
  sessionKey,
  snapshotTransitionDecision,
  statusFor,
} from "../src/session-view.js";

test("hook notifications resolve the stable Claude agent and its matching active session", () => {
  const agents = [
    {
      id: "codex",
      name: "Claude Code",
      active_session: { id: "turn-7" },
      freshness: { observed_at_ms: 1_000, stale: false },
    },
    {
      id: "claude",
      name: "Renamed assistant",
      active_session: { id: "turn-7" },
      freshness: { observed_at_ms: 1_000, stale: false },
    },
  ];

  const target = hookNotificationTarget(agents, "turn-7", 1_001);

  assert.deepEqual(target, {
    agent: agents[1],
    agentIndex: 1,
    session: agents[1].active_session,
  });
  assert.equal(
    notificationGroupKey(target.agent, target.session, target.agentIndex),
    "claude::turn-7",
  );
  assert.equal(hookNotificationTarget(agents, "missing", 1_001), null);
});

test("hook notifications without a session require one fresh Claude active session", () => {
  const freshClaude = {
    id: "claude",
    active_session: { id: "current" },
    freshness: { observed_at_ms: 1_000, stale: false },
  };

  assert.deepEqual(
    hookNotificationTarget([freshClaude], "", 1_001),
    { agent: freshClaude, agentIndex: 0, session: freshClaude.active_session },
  );
  assert.equal(
    hookNotificationTarget([
      freshClaude,
      {
        id: "claude",
        active_session: { id: "other" },
        freshness: { observed_at_ms: 1_000, stale: false },
      },
    ], "", 1_001),
    null,
  );
  assert.equal(
    hookNotificationTarget([{
      ...freshClaude,
      freshness: { observed_at_ms: 1_000, stale: true },
    }], "", 1_001),
    null,
  );
  assert.equal(
    hookNotificationTarget([freshClaude], "", 7_001),
    null,
  );
});

test("present malformed freshness is ineligible while no freshness preserves legacy compatibility", () => {
  assert.equal(isSnapshotFresh({}, 10_000), true);
  assert.equal(isSnapshotFresh({ freshness: { stale: false } }, 10_000), false);
  assert.equal(
    isSnapshotFresh({ freshness: { observed_at_ms: "not-a-timestamp", stale: false } }, 10_000),
    false,
  );
});

test("snapshot transport age is independent from an unchanged event timestamp", () => {
  const quietButPolled = {
    snapshot_received_at_ms: 10_000,
    freshness: { observed_at_ms: 1_000, stale: false },
  };

  assert.equal(isSnapshotFresh(quietButPolled, 10_001), true);
});

test("event timestamps beyond the clock-skew allowance are never fresh", () => {
  assert.equal(
    isSnapshotFresh({ freshness: { observed_at_ms: 15_001, stale: false } }, 10_000),
    false,
  );
});

test("a current successful poll can route a hook for a quiet active session", () => {
  const quietClaude = {
    id: "claude",
    active_session: { id: "current" },
    snapshot_received_at_ms: 8_000,
    freshness: { observed_at_ms: 1_000, stale: false },
  };

  assert.deepEqual(
    hookNotificationTarget([quietClaude], "current", 8_001),
    { agent: quietClaude, agentIndex: 0, session: quietClaude.active_session },
  );
});

test("hook event production route uses the resolved active-session target", async () => {
  const source = await readFile(new URL("../src/main.js", import.meta.url), "utf8");

  assert.match(source, /hookNotificationTarget,/);
  assert.match(source, /const target = hookNotificationTarget\(agents, ev\.session, Date\.now\(\)\);/);
  assert.match(source, /pushNotify\(target\.agent, "waiting", target\.session, target\.agentIndex\);/);
  assert.match(source, /snapshot_received_at_ms:\s*receivedAt/);
});

test("error focus ignores stale errors", () => {
  const agents = [
    { status: "error", freshness: { observed_at_ms: 10, stale: true } },
    { status: "working", freshness: { observed_at_ms: 20, stale: false } },
  ];

  assert.equal(freshErrorIndex(agents), -1);
});

test("error focus selects a fresh error", () => {
  const agents = [
    { status: "error", freshness: { observed_at_ms: 10, stale: true } },
    { status: "error", freshness: { observed_at_ms: 20, stale: false } },
  ];

  assert.equal(freshErrorIndex(agents, 21), 1);
});

test("island never falls back to historical or legacy sessions", () => {
  const agent = {
    display_status: "idle",
    active_session: null,
    history_sessions: [{ id: "old" }],
    session_list: [{ id: "legacy" }],
  };

  assert.equal(islandSession(agent), null);
  assert.equal(islandSession({ session_list: [{ id: "legacy" }] }), null);
});

test("island returns the projected active session", () => {
  const active = { id: "current", display_status: "working" };

  assert.equal(islandSession({ active_session: active }), active);
});

test("overview includes active and history once per agent and session", () => {
  const active = { id: "same", name: "Active", display_status: "working" };
  const rows = overviewRows([
    {
      id: "codex",
      name: "Codex CLI",
      active_session: active,
      history_sessions: [
        { id: "same", name: "Duplicate", display_status: "done" },
        { id: "old", name: "Old", display_status: "done" },
      ],
    },
    {
      id: "claude",
      name: "Codex CLI",
      active_session: null,
      history_sessions: [{ id: "same", name: "Other agent", display_status: "idle" }],
    },
  ]);

  assert.deepEqual(
    rows.map((row) => [row.id, row.session.name]),
    [
      ["codex::same", "Active"],
      ["codex::old", "Old"],
      ["claude::same", "Other agent"],
    ],
  );
});

test("overview uses old session_list only when projection fields are absent", () => {
  const legacy = { id: "legacy", name: "Legacy", log_status: "done" };
  const rows = overviewRows([
    { id: "old-client", name: "Old Client", session_list: [legacy] },
    {
      id: "new-client",
      name: "New Client",
      active_session: null,
      history_sessions: [],
      session_list: [{ id: "stale", name: "Stale" }],
    },
  ]);

  assert.equal(rows.length, 1);
  assert.equal(rows[0].session, legacy);
  assert.equal(rows[0].id, "old-client::legacy");
});

test("status prefers the selected session display status", () => {
  const agent = { display_status: "working", status: "error" };

  assert.equal(statusFor(agent, { display_status: "done", log_status: "waiting" }), "done");
  assert.equal(statusFor(agent, { log_status: "waiting" }), "waiting");
  assert.equal(statusFor(agent, null), "working");
});

test("historical rows do not borrow current agent output or paths", () => {
  const [row] = overviewRows([
    {
      id: "codex",
      name: "Codex CLI",
      display_status: "working",
      recent_output: ["current output"],
      cwd: "D:\\current",
      current_file: "D:\\current\\main.js",
      active_session: null,
      history_sessions: [{ id: "old", name: "Old", display_status: "done" }],
    },
  ]);

  assert.equal(row.status, "done");
  assert.equal(row.output, "暂无输出");
  assert.equal(row.cwd, null);
  assert.equal(row.file, null);
});

test("overview renders the serialized history session's own record projection and metadata", () => {
  const [row] = overviewRows([{
    id: "codex",
    name: "Codex CLI",
    display_status: "working",
    active_session: null,
    history_sessions: [{
      id: "old",
      name: "Old session",
      recent_output: ["old prompt", "old result"],
      records: [
        { event_id: "one", at_ms: 10, role: "user", text: "old prompt" },
        { event_id: "two", at_ms: 20, role: "assistant", text: "old result" },
      ],
      cwd: "D:\\old",
      log_path: "D:\\logs\\old.jsonl",
      current_file: "D:\\old\\main.rs",
      log_status: "done",
      display_status: "done",
      lifecycle: "Historical",
      last_active_at_ms: 20,
    }],
  }]);

  assert.equal(row.status, "done");
  assert.equal(row.output, "old prompt\nold result");
  assert.equal(row.cwd, "D:\\old");
  assert.equal(row.file, "D:\\old\\main.rs");
  assert.deepEqual(row.session.records.map((record) => record.text), ["old prompt", "old result"]);
});

test("overview uses stopped typed history payload without falling back to legacy session_list", () => {
  const [row] = overviewRows([{
    id: "codex",
    name: "Codex CLI",
    status: "stopped",
    display_status: "stopped",
    active_session: null,
    history_sessions: [{
      id: "completed-session",
      name: "Completed session",
      records: [{ event_id: "result", at_ms: 20, role: "Assistant", text: "completed" }],
      recent_output: ["completed"],
      cwd: "D:\\history",
      log_path: "D:\\logs\\history.jsonl",
      current_file: "D:\\history\\main.rs",
      log_status: "done",
      display_status: "done",
      lifecycle: "Historical",
      last_active_at_ms: 20,
    }],
    session_list: [{ id: "legacy", name: "Wrong fallback", log_status: "idle" }],
  }]);

  assert.equal(row.id, "codex::completed-session");
  assert.equal(row.status, "done");
  assert.equal(row.output, "completed");
  assert.equal(row.cwd, "D:\\history");
  assert.equal(row.file, "D:\\history\\main.rs");
});

test("stable session keys isolate duplicate names and session ids", () => {
  const session = { id: "current" };
  const first = { id: "claude", name: "Assistant" };
  const second = { id: "codex", name: "Assistant" };

  assert.equal(agentKey(first, 0), "claude");
  assert.equal(agentKey(second, 1), "codex");
  assert.equal(sessionKey(first, session, 0), "claude::current");
  assert.equal(sessionKey(second, session, 1), "codex::current");
});

test("legacy duplicate names remain distinct by stable position", () => {
  const first = { name: "Assistant" };
  const second = { name: "Assistant" };

  assert.equal(agentKey(first, 0), "legacy:0:Assistant");
  assert.equal(agentKey(second, 1), "legacy:1:Assistant");
});

test("poll selection follows a stable id across rename and reorder", () => {
  const before = [
    { id: "claude", name: "Assistant" },
    { id: "codex", name: "Assistant" },
  ];
  const selectedKey = agentKey(before[1], 1);
  const after = [
    { id: "codex", name: "Renamed Codex" },
    { id: "claude", name: "Assistant" },
  ];

  assert.equal(restoreAgentIndex(after, selectedKey, 1), 0);
});

test("stale snapshot cannot notify flash auto-jump or replace transition baseline", () => {
  const decision = snapshotTransitionDecision(
    { freshness: { observed_at_ms: 10, stale: true } },
    "working",
    "error",
  );

  assert.deepEqual(decision, {
    recordStatus: false,
    notificationKind: null,
    statusFlash: false,
    errorFlash: false,
    autoJump: false,
  });
});

test("fresh snapshot retains legacy transition behavior", () => {
  const decision = snapshotTransitionDecision(
    { freshness: { observed_at_ms: 20, stale: false } },
    "working",
    "waiting",
    21,
  );

  assert.deepEqual(decision, {
    recordStatus: true,
    notificationKind: "waiting",
    statusFlash: true,
    errorFlash: false,
    autoJump: true,
  });
  assert.equal(
    snapshotTransitionDecision({}, "working", "done").notificationKind,
    "done",
  );
});

test("an expired retained error cannot notify, auto-jump, focus, or replace its baseline", () => {
  const retained = {
    id: "codex",
    status: "error",
    freshness: { observed_at_ms: 1_000, stale: false },
  };

  assert.equal(freshErrorIndex([retained], 8_001), -1);
  assert.deepEqual(
    snapshotTransitionDecision(retained, "working", "error", 8_001),
    {
      recordStatus: false,
      notificationKind: null,
      statusFlash: false,
      errorFlash: false,
      autoJump: false,
    },
  );
});

test("a failed poll marks retained records stale before transition and focus paths run", () => {
  const retained = markRetainedAgentsStale([{
    id: "claude",
    status: "error",
    freshness: { observed_at_ms: 7_000, stale: false },
  }]);

  assert.equal(retained[0].freshness.stale, true);
  assert.equal(freshErrorIndex(retained, 7_001), -1);
  assert.equal(
    snapshotTransitionDecision(retained[0], "working", "error", 7_001).recordStatus,
    false,
  );
});

test("notification groups and preference indexes remain isolated across duplicate labels and rename", () => {
  const before = [
    { id: "claude", name: "Assistant", status: "working" },
    { id: "codex", name: "Assistant", status: "error" },
  ];
  const claudeGroup = notificationGroupKey(before[0], { id: "turn-a" }, 0);
  const codexGroup = notificationGroupKey(before[1], { id: "turn-a" }, 1);
  const after = [
    { id: "codex", name: "Renamed Codex", status: "error" },
    { id: "claude", name: "Assistant", status: "working" },
  ];

  assert.notEqual(claudeGroup, codexGroup);
  assert.equal(
    notificationGroupKey(after[0], { id: "turn-a" }, 0),
    codexGroup,
  );
  assert.deepEqual(
    agentIndexesFor(after, {
      focusMode: "pinned",
      pinnedKeys: ["codex"],
      orderKeys: ["claude", "codex"],
      now: 1_000,
    }),
    [0],
  );
  assert.deepEqual(
    agentIndexesFor(after, {
      orderKeys: ["claude", "codex"],
      now: 1_000,
    }),
    [1, 0],
  );
});
