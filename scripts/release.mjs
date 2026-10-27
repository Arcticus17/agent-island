#!/usr/bin/env node
// 用法：
//   node scripts/release.mjs minor --dry-run   # 只看会改什么
//   node scripts/release.mjs patch             # 真改（四个文件一起改）
//   node scripts/release.mjs set 2.0.0
//
// 只改版本号，不提交、不打 tag——那两步由人确认：
//   git commit -am "chore: release X.Y.Z"
//   git tag vX.Y.Z && git push origin vX.Y.Z

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

import { applyPlan, nextVersion, planVersionBump, verifyPlan, verifyVersion } from "./release-lib.mjs";

const root = dirname(dirname(fileURLToPath(import.meta.url)));
const args = process.argv.slice(2);
const dryRun = args.includes("--dry-run");
const [kind, explicit] = args.filter((arg) => !arg.startsWith("--"));

if (!kind) {
  console.error("用法：release.mjs <patch|minor|major|set x.y.z> [--dry-run]");
  process.exit(1);
}

try {
  const current = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;
  const next = nextVersion(current, kind, explicit);
  const plan = planVersionBump(root, next);

  console.log(`版本：${current} -> ${next}${dryRun ? "（预览，不写盘）" : ""}`);
  for (const entry of plan) console.log(`  ${entry.changed ? "改动" : "不变"}  ${entry.relative}`);

  if (dryRun) {
    const problems = verifyPlan(plan, next);
    console.log(`预览校验：${problems.length === 0 ? "四个文件都会变成 " + next : JSON.stringify(problems)}`);
    process.exit(problems.length === 0 ? 0 : 2);
  }

  const written = applyPlan(plan);
  const problems = verifyVersion(root, next);
  if (problems.length > 0) {
    console.error(`版本写入后校验失败：${problems.join(", ")}`);
    process.exit(2);
  }

  // Cargo.lock 与 Cargo.toml 的一致性由 cargo 自己检查；失败只警告，不阻断。
  const cargo = spawnSync("cargo", ["metadata", "--no-deps", "--format-version", "1"], {
    cwd: join(root, "src-tauri"),
    stdio: "ignore",
  });
  if (cargo.status !== 0) console.warn("警告：cargo metadata 失败，请手动确认 src-tauri/Cargo.lock");

  console.log(`已写入：${written.join(", ") || "（没有变化）"}`);
  console.log(`下一步：git commit -am "chore: release ${next}" && git tag v${next} && git push origin v${next}`);
} catch (error) {
  console.error(`失败：${error.message}`);
  process.exit(1);
}
