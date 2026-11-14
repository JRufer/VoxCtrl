// The "Clean up models" dialog's data and wording, kept apart from the
// component so they can be tested without rendering it.

/** One downloaded model, as the `plan_model_cleanup` command lists it. */
export interface CleanupItem {
  /** What `run_model_cleanup` takes back, e.g. "whisper:base". */
  id: string;
  kind: "speech" | "cleanup" | "voice";
  label: string;
  bytes: number;
  /** Why the model is in use and will be kept; `null` when it can go. */
  kept_because: string | null;
}

export interface CleanupPlan {
  items: CleanupItem[];
  removable_bytes: number;
}

export interface CleanupResult {
  removed: string[];
  freed_bytes: number;
  errors: string[];
}

/** "1.4 GB", "312 MB", "0 MB": the precision a disk-space decision needs. */
export function formatBytes(bytes: number): string {
  const mb = bytes / (1024 * 1024);
  if (mb >= 1024) return `${(mb / 1024).toFixed(1)} GB`;
  if (mb >= 10) return `${Math.round(mb)} MB`;
  if (mb >= 0.1) return `${mb.toFixed(1)} MB`;
  return bytes > 0 ? "under 0.1 MB" : "0 MB";
}

export function removableItems(plan: CleanupPlan | null): CleanupItem[] {
  return plan?.items.filter((item) => item.kept_because === null) ?? [];
}

export function keptItems(plan: CleanupPlan | null): CleanupItem[] {
  return plan?.items.filter((item) => item.kept_because !== null) ?? [];
}

/** Total size of the ticked models. */
export function selectedBytes(plan: CleanupPlan | null, selected: ReadonlySet<string>): number {
  return removableItems(plan)
    .filter((item) => selected.has(item.id))
    .reduce((total, item) => total + item.bytes, 0);
}

/** One line on how the cleanup went, and whether anything went wrong. */
export function resultSummary(result: CleanupResult): { ok: boolean; text: string } {
  const count = result.removed.length;
  const freed = `Removed ${count} ${count === 1 ? "model" : "models"} and freed ${formatBytes(result.freed_bytes)}.`;
  if (result.errors.length === 0) return { ok: true, text: freed };
  const failed = `${result.errors.length} could not be removed: ${result.errors.join("; ")}`;
  return { ok: false, text: count > 0 ? `${freed} ${failed}` : failed };
}
