import { describe, expect, it } from "vitest";
import { clampWindowPlacement, parseWindowPlacement } from "../src/bridge/window-placement";

const monitors = [
  { scaleFactor: 1.5, workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1040 } } },
  { scaleFactor: 1, workArea: { position: { x: -1280, y: 0 }, size: { width: 1280, height: 720 } } },
];
describe("window placement", () => {
  it("validates saved preference without trusting corrupted storage", () => {
    expect(parseWindowPlacement("{" )).toBeNull();
    expect(parseWindowPlacement('{"version":2,"x":0,"y":0,"width":420}')).toBeNull();
    expect(parseWindowPlacement('{"version":1,"x":0,"y":0,"width":-1}')).toBeNull();
    expect(parseWindowPlacement('{"version":1,"x":-1000,"y":10,"width":600}')).toEqual({ x: -1000, y: 10, width: 600 });
  });
  it("preserves a valid negative-coordinate secondary monitor", () => {
    expect(clampWindowPlacement({ x: -1200, y: 20, width: 600 }, monitors)).toEqual({ x: -1200, y: 20, width: 600 });
  });
  it("recovers a removed display and clamps mixed-DPI bounds", () => {
    expect(clampWindowPlacement({ x: 3000, y: 2000, width: 900 }, monitors, 400)).toEqual({ x: 804, y: 440, width: 744 });
  });
  it("fits screens narrower than the normal minimum", () => {
    expect(clampWindowPlacement({ x: 10, y: 20, width: 100 }, [{ scaleFactor: 2, workArea: { position: { x: 0, y: 0 }, size: { width: 600, height: 400 } } }], 500)).toEqual({ x: 0, y: 0, width: 300 });
  });
  it("fails explicitly when monitor discovery cannot establish bounds", () => {
    expect(() => clampWindowPlacement({ x: 0, y: 0, width: 420 }, [])).toThrow();
  });
});
