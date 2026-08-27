import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import { DelayRisk, delayRiskBand } from "./DelayRisk";

describe("delayRiskBand", () => {
  it("bands the 0 to 1 score from the analytics service", () => {
    expect(delayRiskBand(0).label).toBe("Low");
    expect(delayRiskBand(0.32).label).toBe("Low");
    expect(delayRiskBand(0.33).label).toBe("Medium");
    expect(delayRiskBand(0.65).label).toBe("Medium");
    expect(delayRiskBand(0.66).label).toBe("High");
    expect(delayRiskBand(1).label).toBe("High");
  });

  it("uses a tone that matches the band", () => {
    expect(delayRiskBand(0.1).tone).toBe("success");
    expect(delayRiskBand(0.5).tone).toBe("warning");
    expect(delayRiskBand(0.9).tone).toBe("danger");
  });
});

describe("DelayRisk", () => {
  it("renders the band and the percentage", () => {
    render(<DelayRisk value="0.72" />);
    expect(screen.getByText(/High risk, 72%/)).toBeTruthy();
  });

  it("says so plainly when the shipment has not been scored", () => {
    render(<DelayRisk value={null} />);
    expect(screen.getByText("Not scored")).toBeTruthy();
  });

  it("does not treat unparseable values as a low score", () => {
    render(<DelayRisk value="unknown" />);
    expect(screen.getByText("Not scored")).toBeTruthy();
  });
});
