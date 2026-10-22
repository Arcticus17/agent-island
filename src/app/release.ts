/**
 * 应用自身的版本检查（**只提示，不自动安装**）。
 *
 * 为什么不做真正的自动更新：
 * - 需要签名密钥与一套发布产物（`.nsis.zip` + `.sig`），当前仓库没有发布链路；
 * - `tauri-plugin-updater` 还必须有一个可访问的更新清单，本机连不上 GitHub，无法验证。
 * 所以先做"能看见新版本 + 一键打开下载页"，把自动安装留到发布流水线就绪之后。
 */

import { compareVersions, parseVersion } from "../agent/updates";

/** 可配置：默认指向 GitHub 的 latest release。 */
export const RELEASE_MANIFEST_URL =
  "https://api.github.com/repos/Arcticus17/agent-island/releases/latest";

export interface ReleaseInfo {
  version: string;
  url: string;
  notes: string;
}

export interface AppUpdateState {
  status: "idle" | "checking" | "available" | "latest" | "failed";
  current: string;
  latest: ReleaseInfo | null;
  message: string;
}

export function initialAppUpdateState(current: string): AppUpdateState {
  return { status: "idle", current, latest: null, message: "" };
}

/** 从 GitHub release JSON（或任何 `{tag_name, html_url, body}` 形状）里取版本信息。 */
export function parseReleaseManifest(payload: unknown): ReleaseInfo | null {
  if (!payload || typeof payload !== "object") return null;
  const record = payload as Record<string, unknown>;
  const rawTag = typeof record.tag_name === "string"
    ? record.tag_name
    : typeof record.version === "string"
      ? record.version
      : "";
  const version = parseVersion(rawTag);
  if (!version) return null;
  const url = typeof record.html_url === "string"
    ? record.html_url
    : typeof record.url === "string"
      ? record.url
      : "";
  if (!url.startsWith("https://")) return null;
  const notes = typeof record.body === "string" ? record.body.slice(0, 200) : "";
  return { version, url, notes };
}

/** 决策：比当前版本新就算有新版本。 */
export function decideAppUpdate(current: string, release: ReleaseInfo | null): AppUpdateState {
  if (!release) {
    return {
      status: "failed",
      current,
      latest: null,
      message: "更新清单格式不认识",
    };
  }
  const newer = compareVersions(release.version, current) > 0;
  return {
    status: newer ? "available" : "latest",
    current,
    latest: release,
    message: newer ? `有新版本 v${release.version}` : "已是最新版本",
  };
}

/** 拉取并判断；任何网络/解析失败都落到 failed，不抛异常。 */
export async function checkAppUpdate(
  current: string,
  fetcher: typeof fetch = fetch,
  url: string = RELEASE_MANIFEST_URL,
  timeoutMs = 8_000,
): Promise<AppUpdateState> {
  // 网络不可达时 fetch 可能挂几十秒：加超时，别让按钮永远停在"检查中"
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetcher(url, {
      headers: { accept: "application/vnd.github+json" },
      signal: controller.signal,
    });
    if (!response.ok) {
      return { status: "failed", current, latest: null, message: `检查失败：HTTP ${response.status}` };
    }
    const payload: unknown = await response.json();
    return decideAppUpdate(current, parseReleaseManifest(payload));
  } catch (error) {
    const timedOut = error instanceof Error && (error.name === "AbortError" || controller.signal.aborted);
    return {
      status: "failed",
      current,
      latest: null,
      message: timedOut
        ? "检查超时：网络似乎不可达"
        : `检查失败：${error instanceof Error ? error.message : String(error)}`,
    };
  } finally {
    clearTimeout(timer);
  }
}

