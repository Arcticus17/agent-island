import assert from "node:assert/strict";
import { clampIslandX, getSafeIslandHeight, getSafeIslandWidth } from "../src/layout.js";

assert.equal(getSafeIslandWidth(520, 320), 304, "keeps a gutter on a 320px viewport");
assert.equal(getSafeIslandWidth(520, 240), 224, "does not make a native window wider than a tiny viewport");
assert.equal(getSafeIslandWidth(360, 1920), 360, "preserves a valid saved preference on a wide viewport");
assert.equal(getSafeIslandWidth(520, 0), 280, "uses a reachable fallback while viewport metrics are unavailable");
assert.equal(getSafeIslandHeight(570, 420), 404, "leaves a vertical gutter on compact displays");

assert.equal(clampIslandX(-40, 0, 1280, 420), 0, "prevents dragging beyond the left edge");
assert.equal(clampIslandX(1100, 0, 1280, 420), 860, "keeps the full island visible at the right edge");
assert.equal(clampIslandX(300, -1920, 0, 420), -420, "supports a monitor positioned left of the primary display");
assert.equal(clampIslandX(5000, -1920, 1920, 420), 1500, "keeps a drag inside a multi-monitor desktop");

console.log("layout compatibility tests passed");
