// Integration test for the generated Claude hook bridge.
//
// We cannot run Claude Code in CI, but we can prove the wire format the bridge
// prints for PreToolUse and PermissionRequest. That format is version-sensitive
// (PermissionRequest uses decision.behavior, not permissionDecision), so a
// regression here would silently break approvals.
import { test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "node:http";
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawn } from "node:child_process";

const script = join(process.cwd(), "src-tauri", "src", "hook_bridge.mjs");

function runBridge(event, decisions) {
  return new Promise((resolve, reject) => {
    const home = mkdtempSync(join(tmpdir(), "agent-island-hook-test-"));
    mkdirSync(join(home, ".agent-island"), { recursive: true });
    let polls = 0;
    const server = createServer((request, response) => {
      const chunks = [];
      request.on("data", (chunk) => chunks.push(chunk));
      request.on("end", () => {
        const body = JSON.parse(Buffer.concat(chunks).toString("utf8") || "{}");
        const send = (payload) => {
          response.writeHead(200, { "content-type": "application/json", connection: "close" });
          response.end(JSON.stringify(payload));
        };
        if (request.url.startsWith("/decision")) {
          const id = new URL(request.url, "http://127.0.0.1").searchParams.get("id");
          const queue = decisions[id] ?? [];
          const next = queue[Math.min(polls, queue.length - 1)];
          polls += 1;
          send(next ?? { decision: null });
          return;
        }
        const id = body.name === "PreToolUse" ? "pre-1" : "perm-1";
        send({ decision: "pending", id });
      });
    });

    server.listen(0, "127.0.0.1", () => {
      const port = server.address().port;
      writeFileSync(
        join(home, ".agent-island", "hook-config.json"),
        JSON.stringify({ port, token: "test-token", enabled: true }),
        "utf8",
      );
      const child = spawn(process.execPath, [script], {
        env: { ...process.env, USERPROFILE: home, HOME: home },
        stdio: ["pipe", "pipe", "pipe"],
      });
      let stdout = "";
      let stderr = "";
      child.stdout.on("data", (chunk) => (stdout += chunk));
      child.stderr.on("data", (chunk) => (stderr += chunk));
      child.on("error", (error) => {
        server.close();
        rmSync(home, { recursive: true, force: true });
        reject(error);
      });
      child.on("exit", (code) => {
        server.close();
        rmSync(home, { recursive: true, force: true });
        if (code !== 0) {
          reject(new Error(`bridge exited ${code}: ${stderr}`));
          return;
        }
        try {
          resolve(JSON.parse(stdout));
        } catch (error) {
          reject(new Error(`bridge output was not JSON: ${stdout} / ${stderr}`));
        }
      });
      child.stdin.end(JSON.stringify(event));
    });
  });
}

test("PreToolUse bridge prints permissionDecision and the human reason", async () => {
  const output = await runBridge(
    {
      hook_event_name: "PreToolUse",
      session_id: "s-1",
      cwd: "D:\\work",
      tool_name: "Bash",
      tool_input: { command: "npm test" },
    },
    { "pre-1": [{ decision: null }, { decision: "allow", reason: "looks safe" }] },
  );
  assert.equal(output.hookSpecificOutput.hookEventName, "PreToolUse");
  assert.equal(output.hookSpecificOutput.permissionDecision, "allow");
  assert.equal(output.hookSpecificOutput.permissionDecisionReason, "looks safe");
});

test("PermissionRequest bridge prints decision.behavior and a deny message", async () => {
  const output = await runBridge(
    {
      hook_event_name: "PermissionRequest",
      session_id: "s-1",
      cwd: "D:\\work",
      tool_name: "Edit",
      tool_input: { file_path: "src/a.ts", old_string: "a", new_string: "b" },
    },
    { "perm-1": [{ decision: null }, { decision: "deny", reason: "no production" }] },
  );
  assert.equal(output.hookSpecificOutput.hookEventName, "PermissionRequest");
  assert.equal(output.hookSpecificOutput.decision.behavior, "deny");
  assert.equal(output.hookSpecificOutput.decision.message, "no production");
});

test("PermissionRequest bridge prints an allow decision", async () => {
  const output = await runBridge(
    {
      hook_event_name: "PermissionRequest",
      session_id: "s-1",
      cwd: "D:\\work",
      tool_name: "Write",
      tool_input: { file_path: "src/a.ts", content: "a" },
    },
    { "perm-1": [{ decision: "allow" }] },
  );
  assert.equal(output.hookSpecificOutput.decision.behavior, "allow");
});
