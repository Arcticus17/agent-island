// @ts-expect-error The project intentionally omits @types/node; Vitest runs this file in Node.
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
// @ts-expect-error The project intentionally omits @types/node; Vitest runs this file in Node.
import { tmpdir } from "node:os";
// @ts-expect-error The project intentionally omits @types/node; Vitest runs this file in Node.
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { applyPlan, nextVersion, planVersionBump, verifyVersion } from "../scripts/release-lib.mjs";

const roots: string[] = [];

const PACKAGE = `{
  "name": "dynamic-island",
  "private": true,
  "license": "Apache-2.0",
  "version": "1.9.1",
  "type": "module"
}
`;

const CARGO = `[package]
name = "dynamic-island"
version = "1.9.1"
edition = "2021"
`;

const TAURI = `{
  "$schema": "../node_modules/@tauri-apps/cli/config.schema.json",
  "productName": "Agent Island",
  "version": "1.9.1",
  "identifier": "com.agent-island.app"
}
`;

const LOCK = `version = 3

[[package]]
name = "anyhow"
version = "1.0.99"

[[package]]
name = "dynamic-island"
version = "1.9.1"
dependencies = [
 "anyhow",
]

[[package]]
name = "serde"
version = "1.0.219"
`;

function makeRoot(): string {
  const root = mkdtempSync(join(tmpdir(), "agent-island-release-"));
  roots.push(root);
  mkdirSync(join(root, "src-tauri"), { recursive: true });
  writeFileSync(join(root, "package.json"), PACKAGE);
  writeFileSync(join(root, "src-tauri/Cargo.toml"), CARGO);
  writeFileSync(join(root, "src-tauri/tauri.conf.json"), TAURI);
  writeFileSync(join(root, "src-tauri/Cargo.lock"), LOCK);
  return root;
}

afterEach(() => {
  for (const root of roots.splice(0)) rmSync(root, { recursive: true, force: true });
});

describe("release version helper", () => {
  it("increments patch, minor and major", () => {
    expect(nextVersion("1.9.1", "patch")).toBe("1.9.2");
    expect(nextVersion("1.9.1", "minor")).toBe("1.10.0");
    expect(nextVersion("1.9.1", "major")).toBe("2.0.0");
    expect(nextVersion("1.9.1", "set", "2.3.4")).toBe("2.3.4");
  });

  it("rejects malformed versions and unknown kinds", () => {
    expect(() => nextVersion("1.9", "patch")).toThrow(/x\.y\.z/);
    expect(() => nextVersion("v1.9.1", "patch")).toThrow(/x\.y\.z/);
    expect(() => nextVersion("1.9.1", "set")).toThrow(/set 需要/);
    expect(() => nextVersion("1.9.1", "banana")).toThrow(/未知的递增类型/);
  });

  it("bumps all four manifests and leaves unrelated versions alone", () => {
    const root = makeRoot();
    const plan = planVersionBump(root, "1.9.2");
    expect(plan.every((entry) => entry.changed)).toBe(true);

    applyPlan(plan);
    expect(JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version).toBe("1.9.2");
    expect(JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8")).version).toBe("1.9.2");
    expect(readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8")).toContain('version = "1.9.2"');
    // Cargo.lock：只改 dynamic-island 那一段，serde 的版本必须保持原样
    const lock = readFileSync(join(root, "src-tauri/Cargo.lock"), "utf8");
    expect(lock).toContain('name = "dynamic-island"\nversion = "1.9.2"');
    expect(lock).toContain('name = "serde"\nversion = "1.0.219"');
    expect(lock).toContain('name = "anyhow"\nversion = "1.0.99"');
    expect(verifyVersion(root, "1.9.2")).toEqual([]);
  });

  it("fails loudly when a manifest is missing the version field", () => {
    const root = makeRoot();
    writeFileSync(join(root, "src-tauri/Cargo.toml"), "[package]\nname = \"dynamic-island\"\n");
    expect(() => planVersionBump(root, "1.9.2")).toThrow(/Cargo.toml 里找不到 version/);
  });

  it("reports which manifests disagree with the target version", () => {
    const root = makeRoot();
    expect(verifyVersion(root, "2.0.0")).toEqual([
      "package.json=1.9.1",
      "tauri.conf.json=1.9.1",
      "Cargo.toml=1.9.1",
    ]);
  });
});
