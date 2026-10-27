// 版本号同步：一次改完四个文件，避免"漏改一处"导致产物版本不一致。
//
// 为什么需要它：版本号散落在 package.json / Cargo.toml / tauri.conf.json / Cargo.lock，
// 手改很容易漏掉 Cargo.lock，构建出来的包和 tag 就对不上。这里只做"改版本号"这一件事，
// 提交和打 tag 仍然由人确认——发版是破坏性操作，不该全自动。

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export const VERSION_FILES = [
  "package.json",
  "src-tauri/Cargo.toml",
  "src-tauri/tauri.conf.json",
  "src-tauri/Cargo.lock",
];

/** 只接受 x.y.z（拒绝 v 前缀、预发布后缀，避免和 tag 规则混淆）。 */
export function parseVersion(value) {
  const match = /^(\d+)\.(\d+)\.(\d+)$/.exec(String(value).trim());
  if (!match) throw new Error(`版本号必须是 x.y.z 形式，收到：${value}`);
  return match.slice(1).map(Number);
}

export function formatVersion([major, minor, patch]) {
  return `${major}.${minor}.${patch}`;
}

/** patch / minor / major 递增；set 需要显式给版本。 */
export function nextVersion(current, kind, explicit) {
  const [major, minor, patch] = parseVersion(current);
  switch (kind) {
    case "patch":
      return formatVersion([major, minor, patch + 1]);
    case "minor":
      return formatVersion([major, minor + 1, 0]);
    case "major":
      return formatVersion([major + 1, 0, 0]);
    case "set":
      if (!explicit) throw new Error("set 需要给出目标版本，例如：release.mjs set 2.0.0");
      return formatVersion(parseVersion(explicit));
    default:
      throw new Error(`未知的递增类型：${kind}（可用 patch / minor / major / set）`);
  }
}

function replaceFirst(source, pattern, replacement) {
  if (!pattern.test(source)) return null;
  pattern.lastIndex = 0;
  return source.replace(pattern, replacement);
}

function bumpPackageJson(text, next) {
  const updated = replaceFirst(text, /("version"\s*:\s*")[^"]+(")/, `$1${next}$2`);
  if (updated === null) throw new Error("package.json 里找不到 version 字段");
  JSON.parse(updated);
  return updated;
}

function bumpCargoToml(text, next) {
  const updated = replaceFirst(text, /(^version\s*=\s*")[^"]+(")/m, `$1${next}$2`);
  if (updated === null) throw new Error("Cargo.toml 里找不到 version 字段");
  return updated;
}

function bumpTauriConfig(text, next) {
  const updated = replaceFirst(text, /("version"\s*:\s*")[^"]+(")/, `$1${next}$2`);
  if (updated === null) throw new Error("tauri.conf.json 里找不到 version 字段");
  if (JSON.parse(updated).version !== next) throw new Error("tauri.conf.json 版本没写对");
  return updated;
}

/** Cargo.lock 里有很多 version 行，只能改 name = "dynamic-island" 那一段。 */
function bumpCargoLock(text, next, crateName) {
  const lines = text.split(/\r?\n/);
  const start = lines.findIndex((line) => line.trim() === `name = "${crateName}"`);
  if (start < 0) throw new Error(`Cargo.lock 里找不到包 ${crateName}`);
  for (let index = start + 1; index < Math.min(start + 6, lines.length); index += 1) {
    if (/^version\s*=\s*"/.test(lines[index].trim())) {
      lines[index] = lines[index].replace(/version\s*=\s*"[^"]+"/, `version = "${next}"`);
      return lines.join("\n");
    }
  }
  throw new Error(`Cargo.lock 里 ${crateName} 段没有 version 行`);
}

/**
 * 就地改四个文件。返回每个文件的新内容（不写盘），方便预览和测试。
 */
export function planVersionBump(root, next) {
  const plan = [];
  for (const relative of VERSION_FILES) {
    const path = join(root, relative);
    if (!existsSync(path)) throw new Error(`缺少文件：${relative}`);
    const before = readFileSync(path, "utf8");
    let after;
    if (relative === "package.json") after = bumpPackageJson(before, next);
    else if (relative === "src-tauri/Cargo.toml") after = bumpCargoToml(before, next);
    else if (relative === "src-tauri/tauri.conf.json") after = bumpTauriConfig(before, next);
    else after = bumpCargoLock(before, next, "dynamic-island");
    plan.push({ relative, path, before, after, changed: before !== after });
  }
  return plan;
}

export function applyPlan(plan) {
  const written = [];
  for (const entry of plan) {
    if (!entry.changed) continue;
    writeFileSync(entry.path, entry.after, "utf8");
    written.push(entry.relative);
  }
  return written;
}

/** 预览模式用：只检查"将要写入的内容"是否已经是目标版本，不碰磁盘。 */
export function verifyPlan(plan, next) {
  const problems = [];
  for (const entry of plan) {
    if (!entry.changed) continue;
    if (entry.relative === "src-tauri/Cargo.lock") {
      const match = new RegExp(`name = "dynamic-island"\\r?\\nversion = "([^"]+)"`).exec(entry.after);
      if (match?.[1] !== next) problems.push(`${entry.relative}=${match?.[1] ?? "?"}`);
      continue;
    }
    if (entry.relative === "src-tauri/Cargo.toml") {
      const match = /^version\s*=\s*"([^"]+)"/m.exec(entry.after);
      if (match?.[1] !== next) problems.push(`${entry.relative}=${match?.[1] ?? "?"}`);
      continue;
    }
    try {
      if (JSON.parse(entry.after).version !== next) problems.push(`${entry.relative} 版本不对`);
    } catch {
      problems.push(`${entry.relative} 不是合法 JSON`);
    }
  }
  return problems;
}

/** 提交前的基本校验：JSON 还能解析、Cargo.toml 版本对得上。 */
export function verifyVersion(root, next) {
  const pkg = JSON.parse(readFileSync(join(root, "package.json"), "utf8"));
  const conf = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
  const cargo = readFileSync(join(root, "src-tauri/Cargo.toml"), "utf8");
  const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(cargo)?.[1];
  const problems = [];
  if (pkg.version !== next) problems.push(`package.json=${pkg.version}`);
  if (conf.version !== next) problems.push(`tauri.conf.json=${conf.version}`);
  if (cargoVersion !== next) problems.push(`Cargo.toml=${cargoVersion}`);
  return problems;
}
