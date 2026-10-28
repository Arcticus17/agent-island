/**
 * Agent CLI 更新检查。
 *
 * 版本对比需要联网：最新版本直接 `fetch` npm registry（registry 带
 * `access-control-allow-origin: *`，WebView 里可以跨域拿）。本地版本走
 * Rust 的 `probe_cli_version`（跑 `<cli> --version`），不联网。
 */

export interface AgentPackage {
  /** npm 包名 */
  package: string;
  /** 用来跑 --version 的命令 */
  cli: string;
}

const AGENT_PACKAGES: Record<string, AgentPackage> = {
  "Claude Code": { package: "@anthropic-ai/claude-code", cli: "claude" },
  "Codex CLI": { package: "@openai/codex", cli: "codex" },
  OpenCode: { package: "opencode-ai", cli: "opencode" },
};

export function packageForAgent(agentName: string | null | undefined): AgentPackage | null {
  if (!agentName) return null;
  return AGENT_PACKAGES[agentName] ?? null;
}

/** 抓出第一个 `x.y.z`（允许 v 前缀与 -beta.1 后缀）。 */
export function parseVersion(text: string | null | undefined): string | null {
  if (!text) return null;
  const match = text.match(/(\d+)\.(\d+)\.(\d+)(-[0-9A-Za-z.-]+)?/);
  return match ? `${match[1]}.${match[2]}.${match[3]}${match[4] ?? ""}` : null;
}

function numericParts(version: string): number[] {
  return version.split("-")[0].split(".").map((part) => Number.parseInt(part, 10) || 0);
}

/** 比较两个 semver：>0 表示 a 比 b 新，忽略 prerelease。 */
export function compareVersions(a: string, b: string): number {
  const left = numericParts(a);
  const right = numericParts(b);
  for (let index = 0; index < 3; index += 1) {
    const diff = (left[index] ?? 0) - (right[index] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
}

const REGISTRY = "https://registry.npmjs.org";
const CACHE_TTL_MS = 10 * 60 * 1000;
const latestCache = new Map<string, { at: number; version: string }>();

/** 测试用：清掉最新版本缓存，避免一个用例的真实结果污染下一个。 */
export function resetUpdateCache(): void {
  latestCache.clear();
}

/** 取 npm 上的 latest 版本；网络/包名有问题时抛出可读错误。 */
export async function fetchLatestVersion(packageName: string): Promise<string> {
  const cached = latestCache.get(packageName);
  if (cached && Date.now() - cached.at < CACHE_TTL_MS) return cached.version;
  const response = await fetch(`${REGISTRY}/${encodeURIComponent(packageName).replace("%40", "@")}/latest`, {
    headers: { accept: "application/json" },
  });
  if (!response.ok) throw new Error(`registry ${response.status}`);
  const payload: unknown = await response.json();
  const version = typeof payload === "object" && payload !== null
    ? (payload as { version?: unknown }).version
    : undefined;
  if (typeof version !== "string" || !parseVersion(version)) {
    throw new Error("registry 返回里没有版本号");
  }
  const parsed = parseVersion(version)!;
  latestCache.set(packageName, { at: Date.now(), version: parsed });
  return parsed;
}

/** 安装命令（在岛内终端里执行，输出直接可看）。
 *
 * `--include=optional` 是 Claude Code 的原生二进制所在；省略它 npm 只装
 * JS 壳，`bin/claude.exe` 会留下 "native binary not installed" 的占位脚本。
 * `--foreground-scripts` 让 postinstall 的输出进入终端，而不是被 npm 吞掉。
 */
export function updateCommand(packageName: string, version = "latest"): string {
  const safeVersion = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/.test(version) ? version : "latest";
  return `npm install -g ${packageName}@${safeVersion} --include=optional --foreground-scripts`;
}
