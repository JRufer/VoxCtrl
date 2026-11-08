import { describe, test, expect } from "vitest";
import {
  applyFilters,
  dayLabel,
  groupByDay,
  highlight,
  periodStart,
  terms,
  type Entry,
} from "../../src/lib/History/search";

// Noon on a fixed day, so "today" and "yesterday" don't depend on when the
// tests run.
const NOW = new Date(2026, 10, 7, 12, 0, 0);
const secs = (d: Date) => Math.floor(d.getTime() / 1000);
const HOUR = 3600;

function entry(id: number, at: number, target_id = "default", target_label = "Focused Window", text = "x"): Entry {
  return { id, at, target_id, target_label, text };
}

describe("terms", () => {
  test("splits words and quoted phrases, like the backend", () => {
    expect(terms('  Hello "New  York" there ')).toEqual(["hello", "new york", "there"]);
    expect(terms('dana "the report')).toEqual(["dana", "the report"]);
    expect(terms('  "  " ')).toEqual([]);
  });
});

describe("highlight", () => {
  const joined = (segs: { text: string; match: boolean }[]) =>
    segs.map((s) => (s.match ? `[${s.text}]` : s.text)).join("");

  test("marks every occurrence of every term, ignoring case", () => {
    expect(joined(highlight("Send the report, then report back", ["report", "send"]))).toBe(
      "[Send] the [report], then [report] back",
    );
  });

  test("merges overlapping matches", () => {
    expect(joined(highlight("bookkeeper", ["book", "okke"]))).toBe("[bookke]eper");
  });

  test("leaves text alone with no terms", () => {
    expect(highlight("plain", [])).toEqual([{ text: "plain", match: false }]);
    expect(highlight("", ["a"])).toEqual([]);
  });

  test("keeps every character, whatever matches", () => {
    const text = "Ünïcode café — naïve";
    expect(highlight(text, ["café", "na"]).map((s) => s.text).join("")).toBe(text);
  });
});

describe("filters", () => {
  const entries = [
    entry(4, secs(NOW) - 10, "notes", "Notes"),
    entry(3, secs(NOW) - 2 * HOUR, "default,notes", "Focused Window, Notes"),
    entry(2, secs(NOW) - 30 * HOUR),
    entry(1, secs(NOW) - 10 * 24 * HOUR),
  ];

  test("by target, including dictations sent to several", () => {
    expect(applyFilters(entries, "notes", "any", NOW).map((e) => e.id)).toEqual([4, 3]);
    expect(applyFilters(entries, "default", "any", NOW).map((e) => e.id)).toEqual([3, 2, 1]);
    expect(applyFilters(entries, "", "any", NOW)).toHaveLength(4);
  });

  test("by time", () => {
    expect(applyFilters(entries, "", "hour", NOW).map((e) => e.id)).toEqual([4]);
    expect(applyFilters(entries, "", "today", NOW).map((e) => e.id)).toEqual([4, 3]);
    expect(applyFilters(entries, "", "week", NOW).map((e) => e.id)).toEqual([4, 3, 2]);
    expect(periodStart("any", NOW)).toBeNull();
  });

  test("both together", () => {
    expect(applyFilters(entries, "default", "today", NOW).map((e) => e.id)).toEqual([3]);
  });
});

describe("day headings", () => {
  test("today, yesterday, then the date", () => {
    expect(dayLabel(secs(NOW) - HOUR, NOW)).toBe("Today");
    expect(dayLabel(secs(NOW) - 20 * HOUR, NOW)).toBe("Yesterday");
    expect(dayLabel(secs(NOW) - 40 * 24 * HOUR, NOW)).not.toMatch(/Today|Yesterday/);
  });

  test("groups keep the entries' order", () => {
    const groups = groupByDay(
      [entry(3, secs(NOW) - HOUR), entry(2, secs(NOW) - 2 * HOUR), entry(1, secs(NOW) - 20 * HOUR)],
      NOW,
    );
    expect(groups.map((g) => [g.day, g.entries.map((e) => e.id)])).toEqual([
      ["Today", [3, 2]],
      ["Yesterday", [1]],
    ]);
  });
});
