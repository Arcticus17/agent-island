import test from "node:test";
import assert from "node:assert/strict";

import { islandSession, overviewRows, statusFor } from "../src/session-view.js";

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
      name: "Claude Code",
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
