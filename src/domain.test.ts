import { describe, expect, it } from "vitest";
import { defaultSession } from "./domain";

describe("Scratch Session safety defaults", () => {
  it("starts muted at a conservative master level with a safety limiter", () => {
    const session = defaultSession();

    expect(session.projectName).toBeNull();
    expect(session.emergencyMuted).toBe(true);
    expect(session.masterDb).toBe(-18);
    expect(session.rack.map((device) => device.id)).toEqual(["input", "safety", "output"]);
    expect(session.rack.find((device) => device.id === "safety")?.bypassed).toBe(false);
  });
});
