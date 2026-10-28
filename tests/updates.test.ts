import { afterEach, describe, expect, it, vi } from "vitest";

import {
  compareVersions,
  fetchLatestVersion,
  packageForAgent,
  parseVersion,
  updateCommand,
} from "../src/agent/updates";

describe("agent update helpers", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("maps agents to their npm packages", () => {
    expect(packageForAgent("Claude Code")).toEqual({ package: "@anthropic-ai/claude-code", cli: "claude" });
    expect(packageForAgent("Codex CLI")?.package).toBe("@openai/codex");
    expect(packageForAgent("OpenCode")?.cli).toBe("opencode");
    expect(packageForAgent("Hermes")).toBeNull();
    expect(packageForAgent(null)).toBeNull();
  });

  it("parses versions out of CLI output", () => {
    expect(parseVersion("codex-cli 0.163.0")).toBe("0.163.0");
    expect(parseVersion("v2.1.306 (Claude Code)")).toBe("2.1.306");
    expect(parseVersion("1.0.0-beta.3")).toBe("1.0.0-beta.3");
    expect(parseVersion("no version here")).toBeNull();
    expect(parseVersion(null)).toBeNull();
  });

  it("compares versions numerically", () => {
    expect(compareVersions("0.164.0", "0.163.0")).toBeGreaterThan(0);
    expect(compareVersions("0.163.0", "0.163.0")).toBe(0);
    expect(compareVersions("0.9.0", "0.10.0")).toBeLessThan(0);
    expect(compareVersions("2.0.0-beta.1", "1.9.9")).toBeGreaterThan(0);
  });

  it("reads the latest version from the npm registry", async () => {
    const fetchMock = vi.fn(async (_input: RequestInfo | URL) => new Response(JSON.stringify({ version: "0.170.0" }), { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    await expect(fetchLatestVersion("@openai/codex-update-test")).resolves.toBe("0.170.0");
    expect(String(fetchMock.mock.calls[0]?.[0])).toContain("registry.npmjs.org");
    vi.unstubAllGlobals();
  });

  it("surfaces registry failures", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("nope", { status: 503 })));
    await expect(fetchLatestVersion("@openai/codex-error-test")).rejects.toThrow("registry 503");
    vi.unstubAllGlobals();
  });

  it("builds the install command with native optional deps and visible scripts", () => {
    expect(updateCommand("@openai/codex")).toBe(
      "npm install -g @openai/codex@latest --include=optional --foreground-scripts",
    );
    expect(updateCommand("@anthropic-ai/claude-code", "2.1.312")).toBe(
      "npm install -g @anthropic-ai/claude-code@2.1.312 --include=optional --foreground-scripts",
    );
    // A malformed rollback version must never become part of a shell command.
    expect(updateCommand("@anthropic-ai/claude-code", "2.1.312 & calc.exe")).toContain("@latest");
  });
});
