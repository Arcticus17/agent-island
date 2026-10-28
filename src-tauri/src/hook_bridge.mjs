#!/usr/bin/env node
// Agent Island hook bridge (auto-generated, do not edit by hand)
//
// Claude Code runs this script for PreToolUse / PermissionRequest / Stop /
// Notification. It forwards the event to the local island, waits for a human
// decision when needed, and prints the hook-specific JSON that Claude Code
// understands.
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { join } from "node:path";
import { request } from "node:http";

let cfg = null;
try {
  cfg = JSON.parse(readFileSync(join(homedir(), ".agent-island", "hook-config.json"), "utf8"));
} catch {
  process.exit(0);
}
if (!cfg || !cfg.port || !cfg.token) {
  process.exit(0);
}

const headers = { "content-type": "application/json", "x-agent-island-token": cfg.token };
const DECISION_TIMEOUT_MS = 120_000;

// Give stdout/undici a tick to flush before exiting; an immediate exit after a
// fast allow decision can abort a closing socket (libuv assertion on Windows).
function exitSoon() {
  setTimeout(() => process.exit(0), 25);
}

function capValue(value, limit = 8_000) {
  if (typeof value === "string") {
    return value.length > limit ? value.slice(0, limit) : value;
  }
  if (Array.isArray(value)) {
    return value.slice(0, 20).map((entry) => capValue(entry, limit));
  }
  if (value && typeof value === "object") {
    const copy = {};
    for (const [key, entry] of Object.entries(value)) {
      copy[key] = capValue(entry, limit);
    }
    return copy;
  }
  return value;
}

function toolInput(event) {
  const input = event.tool_input;
  if (!input || typeof input !== "object") return {};
  return capValue(input);
}

function httpJson(path, payload) {
  return new Promise((resolve, reject) => {
    const body = payload ? JSON.stringify(payload) : "";
    const req = request(
      {
        host: "127.0.0.1",
        port: cfg.port,
        path,
        method: payload ? "POST" : "GET",
        agent: false,
        headers: {
          ...headers,
          connection: "close",
          ...(payload ? { "content-length": Buffer.byteLength(body) } : {}),
        },
      },
      (response) => {
        let raw = "";
        response.setEncoding("utf8");
        response.on("data", (chunk) => (raw += chunk));
        response.on("end", () => {
          try {
            resolve(JSON.parse(raw || "{}"));
          } catch (error) {
            reject(error);
          }
        });
      },
    );
    req.setTimeout(5_000, () => req.destroy(new Error("island_timeout")));
    req.on("error", reject);
    req.end(body);
  });
}

async function postEvent(payload) {
  return await httpJson("/event", payload);
}

async function waitForDecision(id) {
  const deadline = Date.now() + DECISION_TIMEOUT_MS;
  while (Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 300));
    try {
      const body = await httpJson(`/decision?id=${encodeURIComponent(id)}`, null);
      if (body.decision) {
        return { decision: body.decision, reason: body.reason || "" };
      }
    } catch {
      // The island may be restarting; keep polling until the deadline.
    }
  }
  return { decision: null, reason: "" };
}

async function resolveDecision(body) {
  if (body.decision === "allow" || body.decision === "deny" || body.decision === "ask") {
    return { decision: body.decision, reason: body.reason || "" };
  }
  if (body.id) {
    return await waitForDecision(body.id);
  }
  return { decision: null, reason: "" };
}

function preToolOutput(decision, reason) {
  process.stdout.write(JSON.stringify({
    hookSpecificOutput: {
      hookEventName: "PreToolUse",
      permissionDecision: decision,
      permissionDecisionReason: reason,
    },
  }));
}

function permissionRequestOutput(decision, reason) {
  if (decision === "allow") {
    process.stdout.write(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: "PermissionRequest",
        decision: { behavior: "allow" },
      },
    }));
    return;
  }
  if (decision === "deny") {
    process.stdout.write(JSON.stringify({
      hookSpecificOutput: {
        hookEventName: "PermissionRequest",
        decision: {
          behavior: "deny",
          message: reason || "Agent Island 拒绝了这次操作",
          decisionClassification: "user_reject",
        },
      },
    }));
  }
  // No decision: print nothing, so Claude Code shows its native prompt.
}

let input = "";
process.stdin.on("data", (chunk) => (input += chunk));
process.stdin.on("end", async () => {
  try {
    const event = JSON.parse(input || "{}");
    const name = event.hook_event_name || "";

    if (name === "PreToolUse" || name === "PermissionRequest") {
      const tool = event.tool_name || "";
      const payload = {
        name,
        tool,
        command: (event.tool_input && event.tool_input.command) || "",
        tool_input: toolInput(event),
        session: event.session_id || "",
        cwd: event.cwd || "",
        tool_use_id: event.tool_use_id || "",
        permission_mode: event.permission_mode || "",
      };
      let result = { decision: null, reason: "" };
      try {
        result = await resolveDecision(await postEvent(payload));
      } catch {
        // Island offline: fall back to the native prompt.
      }
      if (name === "PreToolUse") {
        const decision = result.decision === "allow" || result.decision === "deny"
          ? result.decision
          : "ask";
        preToolOutput(
          decision,
          result.reason || (decision === "allow"
            ? "Agent Island 允许"
            : decision === "deny"
              ? "Agent Island 拒绝"
              : "Agent Island 未响应，回退原生确认"),
        );
      } else {
        permissionRequestOutput(result.decision, result.reason);
      }
      exitSoon();
    }

    if (name === "Stop") {
      try {
        await postEvent({
          name: "Stop",
          session: event.session_id || "",
          cwd: event.cwd || "",
        });
      } catch {}
      exitSoon();
    }

    if (name === "Notification") {
      try {
        await postEvent({
          name: "Notification",
          message: event.message || "",
          session: event.session_id || "",
          cwd: event.cwd || "",
        });
      } catch {}
      exitSoon();
    }

    exitSoon();
  } catch {
    exitSoon();
  }
});
