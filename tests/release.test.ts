import { describe, expect, it } from "vitest";

import {
  checkAppUpdate,
  decideAppUpdate,
  initialAppUpdateState,
  parseReleaseManifest,
  type ReleaseInfo,
} from "../src/app/release";

const latest: ReleaseInfo = {
  version: "1.9.3",
  url: "https://github.com/Arcticus17/agent-island/releases/tag/v1.9.3",
  notes: "fixes",
};

describe("app update check", () => {
  it("parses GitHub release manifests", () => {
    expect(
      parseReleaseManifest({
        tag_name: "v1.9.3",
        html_url: "https://github.com/owner/repo/releases/tag/v1.9.3",
        body: "notes",
      }),
    ).toEqual({ version: "1.9.3", url: "https://github.com/owner/repo/releases/tag/v1.9.3", notes: "notes" });
    expect(parseReleaseManifest({ version: "2.0.0", url: "https://example.com/rel" })).toMatchObject({ version: "2.0.0" });
  });

  it("rejects manifests without a usable version or https url", () => {
    expect(parseReleaseManifest(null)).toBeNull();
    expect(parseReleaseManifest({ tag_name: "not-a-version" })).toBeNull();
    expect(parseReleaseManifest({ tag_name: "v1.0.0" })).toBeNull();
    expect(parseReleaseManifest({ tag_name: "v1.0.0", html_url: "javascript:alert(1)" })).toBeNull();
    expect(parseReleaseManifest({ tag_name: "v1.0.0", html_url: "http://insecure.example" })).toBeNull();
  });

  it("decides newer, equal and unknown", () => {
    expect(decideAppUpdate("1.9.0", latest)).toMatchObject({ status: "available", message: "有新版本 v1.9.3" });
    expect(decideAppUpdate("1.9.3", latest)).toMatchObject({ status: "latest", message: "已是最新版本" });
    expect(decideAppUpdate("2.0.0", latest).status).toBe("latest");
    expect(decideAppUpdate("1.9.0", null)).toMatchObject({ status: "failed" });
  });

  it("starts idle with the current version", () => {
    expect(initialAppUpdateState("1.9.0")).toEqual({ status: "idle", current: "1.9.0", latest: null, message: "" });
  });

  it("turns network and http failures into failed states", async () => {
    const networkError = await checkAppUpdate("1.9.0", (async () => {
      throw new Error("offline");
    }) as unknown as typeof fetch, "https://api.github.com/x");
    expect(networkError.status).toBe("failed");
    expect(networkError.message).toContain("offline");

    const httpError = await checkAppUpdate("1.9.0", (async () => ({
      ok: false,
      status: 404,
      json: async () => ({}),
    })) as unknown as typeof fetch, "https://api.github.com/x");
    expect(httpError.status).toBe("failed");
    expect(httpError.message).toContain("404");
  });

  it("gives up when the network hangs", async () => {
    const hanging = ((_url: string, init?: RequestInit) => new Promise((_resolve, reject) => {
      init?.signal?.addEventListener("abort", () => {
        const error = new Error("aborted");
        error.name = "AbortError";
        reject(error);
      });
    })) as unknown as typeof fetch;
    const state = await checkAppUpdate("1.9.0", hanging, "https://api.github.com/x", 20);
    expect(state.status).toBe("failed");
    expect(state.message).toContain("超时");
  });

  it("succeeds with a stubbed manifest", async () => {
    const fetcher = (async () => ({
      ok: true,
      status: 200,
      json: async () => ({ tag_name: "v1.10.0", html_url: "https://github.com/owner/repo/releases/tag/v1.10.0" }),
    })) as unknown as typeof fetch;
    const state = await checkAppUpdate("1.9.0", fetcher, "https://api.github.com/x");
    expect(state.status).toBe("available");
    expect(state.latest?.version).toBe("1.10.0");
  });
});

