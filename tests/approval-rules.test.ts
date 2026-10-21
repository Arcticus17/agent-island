import { describe, expect, it } from "vitest";

import { ruleLabel, suggestRulePattern } from "../src/approvals/rules";

describe("approval rule suggestions", () => {
  it("keeps the first two tokens and wildcards the rest", () => {
    expect(suggestRulePattern("npm install left-pad")).toBe("npm install *");
    expect(suggestRulePattern("git push --force origin main")).toBe("git push *");
    expect(suggestRulePattern("rm -rf ./build")).toBe("rm -rf *");
  });

  it("keeps single-token commands exact", () => {
    expect(suggestRulePattern("npm")).toBe("npm");
    expect(suggestRulePattern("  git  ")).toBe("git");
  });

  it("normalizes whitespace and empty input", () => {
    expect(suggestRulePattern("npm   run    build")).toBe("npm run *");
    expect(suggestRulePattern("")).toBe("");
    expect(suggestRulePattern("   ")).toBe("");
  });

  it("labels rules for the drawer", () => {
    expect(ruleLabel("Bash", "npm *")).toBe("Bash · npm *");
    expect(ruleLabel("", "git status")).toBe("任意工具 · git status");
  });
});
