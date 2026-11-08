/**
 * Search helpers for the History window.
 *
 * The backend does the matching (`voxctrl_history::terms` / `matches`); these
 * mirror its rules so the window can highlight what matched, and add the
 * window's own filters on top of the results.
 */

export type Entry = { id: number; at: number; target_id: string; target_label: string; text: string };

/**
 * The terms a query must all match: words, or phrases in double quotes,
 * lower-cased. An unclosed quote runs to the end. Same rules as
 * `voxctrl_history::terms`.
 */
export function terms(query: string): string[] {
  const out: string[] = [];
  query.split('"').forEach((part, i) => {
    if (i % 2 === 1) {
      const phrase = part.split(/\s+/).filter(Boolean).join(" ");
      if (phrase) out.push(phrase.toLowerCase());
    } else {
      out.push(...part.split(/\s+/).filter(Boolean).map((w) => w.toLowerCase()));
    }
  });
  return out;
}

export type Segment = { text: string; match: boolean };

/**
 * Split `text` into runs that are, or aren't, an occurrence of one of
 * `searchTerms` (ignoring case), for highlighting. Overlapping matches merge.
 */
export function highlight(text: string, searchTerms: string[]): Segment[] {
  if (!text) return [];
  const lower = text.toLowerCase();
  // Only treat a match as a plain range when lower-casing kept every
  // character's position; otherwise skip highlighting rather than mark the
  // wrong letters.
  if (lower.length !== text.length || searchTerms.length === 0) return [{ text, match: false }];

  const marked = new Array<boolean>(text.length).fill(false);
  for (const term of searchTerms) {
    if (!term) continue;
    let from = 0;
    for (;;) {
      const at = lower.indexOf(term, from);
      if (at < 0) break;
      marked.fill(true, at, at + term.length);
      from = at + 1;
    }
  }

  const segments: Segment[] = [];
  let start = 0;
  for (let i = 1; i <= text.length; i++) {
    if (i === text.length || marked[i] !== marked[start]) {
      segments.push({ text: text.slice(start, i), match: marked[start] });
      start = i;
    }
  }
  return segments;
}

/** The output target ids one entry went to (several when it was sent to many). */
export function targetIds(entry: Entry): string[] {
  return entry.target_id
    .split(",")
    .map((s) => s.trim())
    .filter(Boolean);
}

/** The "When" filter's choices, and how far back each reaches. */
export type Period = "any" | "hour" | "today" | "week";

export const PERIOD_OPTIONS: { value: Period; label: string }[] = [
  { value: "any", label: "Any time" },
  { value: "hour", label: "Last hour" },
  { value: "today", label: "Today" },
  { value: "week", label: "Last 7 days" },
];

/** The earliest time (Unix seconds) a period lets through, or null for any. */
export function periodStart(period: Period, now: Date = new Date()): number | null {
  const secs = Math.floor(now.getTime() / 1000);
  switch (period) {
    case "hour":
      return secs - 60 * 60;
    case "today": {
      const midnight = new Date(now);
      midnight.setHours(0, 0, 0, 0);
      return Math.floor(midnight.getTime() / 1000);
    }
    case "week":
      return secs - 7 * 24 * 60 * 60;
    default:
      return null;
  }
}

/** Entries that went to `target` (any target when empty) within `period`. */
export function applyFilters(entries: Entry[], target: string, period: Period, now: Date = new Date()): Entry[] {
  const since = periodStart(period, now);
  return entries.filter(
    (e) => (!target || targetIds(e).includes(target)) && (since === null || e.at >= since),
  );
}

/** A heading for the day an entry is from: Today, Yesterday, or the date. */
export function dayLabel(at: number, now: Date = new Date()): string {
  const d = new Date(at * 1000);
  const today = new Date(now);
  today.setHours(0, 0, 0, 0);
  const day = new Date(d);
  day.setHours(0, 0, 0, 0);
  const days = Math.round((today.getTime() - day.getTime()) / (24 * 60 * 60 * 1000));
  if (days === 0) return "Today";
  if (days === 1) return "Yesterday";
  const sameYear = d.getFullYear() === now.getFullYear();
  return d.toLocaleDateString([], {
    weekday: days < 7 ? "long" : undefined,
    month: "short",
    day: "numeric",
    year: sameYear ? undefined : "numeric",
  });
}

/** Entries grouped under their day heading, keeping their order. */
export function groupByDay(entries: Entry[], now: Date = new Date()): { day: string; entries: Entry[] }[] {
  const groups: { day: string; entries: Entry[] }[] = [];
  for (const e of entries) {
    const day = dayLabel(e.at, now);
    const last = groups[groups.length - 1];
    if (last && last.day === day) last.entries.push(e);
    else groups.push({ day, entries: [e] });
  }
  return groups;
}
