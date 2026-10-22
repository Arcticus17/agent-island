import { describe, expect, it } from "vitest";

import {
  ONBOARDED_KEY,
  ONBOARDING_STEPS,
  markOnboarded,
  shouldShowOnboarding,
} from "../src/onboarding/state";

describe("first-run onboarding", () => {
  it("shows until the flag is written", () => {
    const store = new Map<string, string>();
    const read = (key: string) => store.get(key) ?? null;
    expect(shouldShowOnboarding(read)).toBe(true);
    markOnboarded((key, value) => store.set(key, value));
    expect(store.get(ONBOARDED_KEY)).toBe("1");
    expect(shouldShowOnboarding(read)).toBe(false);
  });

  it("treats unexpected stored values as not-onboarded", () => {
    expect(shouldShowOnboarding(() => "0")).toBe(true);
    expect(shouldShowOnboarding(() => "")).toBe(true);
    expect(shouldShowOnboarding(() => null)).toBe(true);
    expect(shouldShowOnboarding(() => "1")).toBe(false);
  });

  it("covers the three entry points", () => {
    expect(ONBOARDING_STEPS.map((step) => step.keys)).toEqual(["Ctrl+Shift+A", "Ctrl+K", "允许 / 记住这条"]);
    expect(ONBOARDING_STEPS.every((step) => step.detail.length > 4)).toBe(true);
  });
});
