import { describe, test, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/svelte";
import BugReportTab from "../../src/lib/Settings/BugReportTab.svelte";

const invoke = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invoke(...args),
}));
vi.mock("@tauri-apps/plugin-shell", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: vi.fn() }));

const CONTEXT = {
  relay_configured: true,
  issues_new_url: "https://github.com/JRufer/VoxCtrl/issues/new",
  support_email: "voxctrl-bugs@proton.me",
  log_path: "/home/tester/.local/share/voxctrl/startup_errors.log",
  install_id: "0123456789abcdef0123456789abcdef",
  limits: {
    cooldown_seconds: 120,
    per_day: 5,
    per_month: 20,
    min_description_chars: 30,
    max_description_chars: 4000,
  },
  submissions_last_day: 0,
  submissions_last_month: 0,
};

const PREVIEW = {
  markdown: "### What happened\n\nIt stopped recording.",
  title: "[Linux] Recording stops",
  fingerprint: "abcdef0123456789",
  blocked_reason: null,
  can_submit: true,
  github_url: "https://github.com/JRufer/VoxCtrl/issues/new?title=x",
  mailto_url: "mailto:voxctrl-bugs@proton.me?subject=x",
};

const A_REAL_DESCRIPTION =
  "I press the dictation hotkey, the overlay appears, and no text is ever typed anywhere.";

function respondWith(overrides: Record<string, unknown> = {}) {
  invoke.mockImplementation(async (cmd: string) => {
    if (cmd === "bug_report_context") return { ...CONTEXT, ...(overrides.context ?? {}) };
    if (cmd === "preview_bug_report") return { ...PREVIEW, ...(overrides.preview ?? {}) };
    if (cmd === "submit_bug_report") return overrides.outcome ?? { ok: true, issue_url: null, message: "Filed." };
    return {};
  });
}

/** Fill the form in far enough that a report can be built. */
async function fillForm() {
  await fireEvent.input(screen.getByLabelText(/One-line summary/i), {
    target: { value: "Recording stops" },
  });
  await fireEvent.input(screen.getByLabelText(/What you did, what happened/i), {
    target: { value: A_REAL_DESCRIPTION },
  });
}

describe("BugReportTab", () => {
  beforeEach(() => {
    invoke.mockReset();
    respondWith();
  });

  test("says what is collected and what is not before asking for anything", async () => {
    // The disclosure has to be readable without typing a word — a user
    // deciding whether to trust this page should not have to fill in a form
    // to find out what pressing Send would do.
    render(BugReportTab);
    expect(screen.getByText(/Included/)).toBeTruthy();
    expect(screen.getByText(/Never included/)).toBeTruthy();
    expect(screen.getByText(/Anything you have dictated/i)).toBeTruthy();
    expect(screen.getByText(/API keys and access tokens/i)).toBeTruthy();
  });

  test("no report is built until there is something to report", async () => {
    render(BugReportTab);
    await vi.waitFor(() => expect(invoke).toHaveBeenCalledWith("bug_report_context"));
    await new Promise((r) => setTimeout(r, 600));
    expect(invoke.mock.calls.some(([cmd]) => cmd === "preview_bug_report")).toBe(false);
  });

  test("a too-short description is called out before it can be sent", async () => {
    render(BugReportTab);
    await fireEvent.input(screen.getByLabelText(/What you did, what happened/i), {
      target: { value: "broken" },
    });
    expect(screen.getByText(/at least 30 please/i)).toBeTruthy();
    await new Promise((r) => setTimeout(r, 600));
    expect(invoke.mock.calls.some(([cmd]) => cmd === "preview_bug_report")).toBe(false);
  });

  test("the finished report can be read in full before it is sent anywhere", async () => {
    render(BugReportTab);
    await fillForm();
    const toggle = await screen.findByRole("button", { name: /exactly what will be sent/i }, { timeout: 3000 });
    await fireEvent.click(toggle);
    expect(screen.getByText(/It stopped recording\./)).toBeTruthy();
    // Reading the report must not have sent it.
    expect(invoke.mock.calls.some(([cmd]) => cmd === "submit_bug_report")).toBe(false);
  });

  test("a build with no relay offers the routes that need no server, and no Send button", async () => {
    // This is the shipping default until a relay is deployed, so it is the
    // state most users will see first.
    respondWith({ context: { relay_configured: false } });
    render(BugReportTab);
    await fillForm();
    await screen.findByRole("button", { name: /Save report to a file/i }, { timeout: 3000 });
    expect(screen.queryByRole("button", { name: /^Send report$/i })).toBeNull();
    expect(screen.getByRole("button", { name: /Open on GitHub/i })).toBeTruthy();
    expect(screen.getByText(/no automatic submission set up/i)).toBeTruthy();
  });

  test("a rate limit is explained in words and does not hide the other routes", async () => {
    respondWith({
      preview: { blocked_reason: "A report was just sent. You can send another in 90 seconds.", can_submit: false },
    });
    render(BugReportTab);
    await fillForm();
    await screen.findByText(/You can send another in 90 seconds/i, undefined, { timeout: 3000 });
    // Saving and copying are never rate-limited: a blocked user must still be
    // able to get their report out by hand.
    expect(screen.getByRole("button", { name: /Save report to a file/i })).toBeTruthy();
    expect((screen.getByRole("button", { name: /^Send report$/i }) as HTMLButtonElement).disabled).toBe(true);
  });

  test("sending reports the outcome back to the user", async () => {
    respondWith({ outcome: { ok: true, issue_url: "https://github.com/o/r/issues/12", message: "Thank you — filed." } });
    render(BugReportTab);
    await fillForm();
    const send = await screen.findByRole("button", { name: /^Send report$/i }, { timeout: 3000 });
    await fireEvent.click(send);
    await screen.findByText(/Thank you — filed\./);
    expect(screen.getByRole("button", { name: /View the issue/i })).toBeTruthy();
  });

  test("the report ID can be thrown away", async () => {
    // An identifier a user cannot reset is a tracking identifier whatever it
    // was meant to be.
    invoke.mockImplementation(async (cmd: string) => {
      if (cmd === "bug_report_context") return CONTEXT;
      if (cmd === "reset_bug_report_identity") return "ffffffffffffffffffffffffffffffff";
      return {};
    });
    render(BugReportTab);
    const reset = await screen.findByRole("button", { name: /Reset ID/i });
    await fireEvent.click(reset);
    await screen.findByRole("button", { name: /Reset ✓/i });
    expect(screen.getByText(/ffffffffffff/)).toBeTruthy();
  });
});
