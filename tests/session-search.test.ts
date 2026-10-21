import { describe, expect, it } from "vitest";

import {
  highlightSegments,
  matchLabel,
  searchRecords,
  type SearchableRecord,
} from "../src/session/search";

const records: SearchableRecord[] = [
  { event_id: "1", role: "User", text: "请检查布局保存与状态显示。" },
  { event_id: "2", role: "Assistant", text: "布局已读取，正在核对当前会话。" },
  { event_id: "3", role: "Tool", text: "npm run check && npm run build" },
  { event_id: "4", role: "Assistant", text: "NPM 检查通过，布局问题已修复。" },
];

describe("session search", () => {
  it("returns everything for an empty query", () => {
    const hits = searchRecords(records, "   ");
    expect(hits).toHaveLength(4);
    expect(hits.every((hit) => hit.ranges.length === 0)).toBe(true);
  });

  it("matches text case-insensitively and keeps the original order", () => {
    const hits = searchRecords(records, "npm");
    expect(hits.map((hit) => hit.record.event_id)).toEqual(["3", "4"]);
    expect(hits[0].ranges).toEqual([[0, 3], [17, 20]]);
  });

  it("matches the role as well as the text", () => {
    const hits = searchRecords(records, "tool");
    expect(hits.map((hit) => hit.record.event_id)).toEqual(["3"]);
    expect(hits[0].ranges).toEqual([]);
  });

  it("collects several matches per record and respects the limit", () => {
    const hits = searchRecords([{ event_id: "x", role: "User", text: "布局 布局 布局" }], "布局");
    expect(hits[0].ranges).toHaveLength(3);
    expect(searchRecords(records, "布局", 1)).toHaveLength(1);
  });

  it("splits text into highlighted segments", () => {
    const segments = highlightSegments("npm run npm", [[0, 3], [8, 11]]);
    expect(segments.filter((segment) => segment.hit).map((segment) => segment.text)).toEqual(["npm", "npm"]);
    expect(segments.map((segment) => segment.text).join("")).toBe("npm run npm");
    expect(highlightSegments("plain", [])).toEqual([{ text: "plain", hit: false }]);
  });

  it("labels the match counter", () => {
    expect(matchLabel("", 4, 4)).toBe("4");
    expect(matchLabel("npm", 2, 4)).toBe("2 / 4");
  });
});
