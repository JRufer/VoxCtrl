/**
 * Tests for the bug-report relay.
 *
 * The relay is the only thing standing between an anonymous "file an issue"
 * button and the issue tracker, so the cases that matter here are the refusals:
 * what it will not file, and what it tells the person whose report it turned
 * away.
 */
import { describe, test, expect, vi, beforeEach } from "vitest";
// @ts-expect-error - a Worker module, not typed for the app's tsconfig
import worker, { __test__ } from "../../scripts/bug-report-relay/worker.js";

const { validate, checkLimits, readLimits, DEFAULTS } = __test__;

/** A KV namespace that lives in a Map. */
function fakeKv(initial: Record<string, string> = {}) {
  const store = new Map(Object.entries(initial));
  return {
    store,
    async get(key: string, options?: { type?: string }) {
      const value = store.get(key) ?? null;
      if (value !== null && options?.type === "json") return JSON.parse(value);
      return value;
    },
    async put(key: string, value: string) {
      store.set(key, value);
    },
  };
}

function envelope(overrides: Record<string, any> = {}) {
  return {
    schema: 1,
    title: "[Windows] Recording stops after one second",
    body: "### What happened\n\nIt stops.",
    fingerprint: "a".repeat(64),
    install_id: "0123456789abcdef0123456789abcdef",
    app_version: "0.5.1",
    os: "windows",
    report: {
      statement: {
        summary: "Recording stops",
        description: "I press the hotkey, it records for a second, then stops on its own.",
        area: "Hotkeys",
        frequency: "always",
      },
      system: { os: "windows", app_version: "0.5.1" },
    },
    ...overrides,
  };
}

function post(body: unknown) {
  return new Request("https://relay.example/report", {
    method: "POST",
    headers: { "Content-Type": "application/json", "CF-Connecting-IP": "203.0.113.9" },
    body: JSON.stringify(body),
  });
}

function env(kv: ReturnType<typeof fakeKv>, overrides: Record<string, string> = {}) {
  return {
    REPORTS: kv,
    GITHUB_REPO: "JRufer/VoxCtrl",
    GITHUB_TOKEN: "ghp_not_a_real_token",
    IP_SALT: "test-salt",
    ...overrides,
  };
}

describe("relay validation", () => {
  test("a report from a schema this relay does not know is refused, kindly", () => {
    const message = validate(envelope({ schema: 99 }));
    expect(message).toMatch(/no longer understands/i);
    expect(message).toMatch(/save the report to a file/i);
  });

  test("a description that is one character held down is not a bug report", () => {
    const message = validate(
      envelope({ report: { statement: { description: "a".repeat(200) } } }),
    );
    expect(message).toMatch(/in words/i);
  });

  test("a description too short to act on is refused", () => {
    const message = validate(envelope({ report: { statement: { description: "broke" } } }));
    expect(message).toMatch(/more detail/i);
  });

  test("a fingerprint that is not a hash is refused", () => {
    // The fingerprint is a key in the store; anything else there is either a
    // bug in a client or someone probing.
    expect(validate(envelope({ fingerprint: "../../etc/passwd" }))).toBeTruthy();
  });

  test("an ordinary report passes", () => {
    expect(validate(envelope())).toBeNull();
  });
});

describe("relay limits", () => {
  const limits = DEFAULTS;

  test("a connection that has had its hourly share is turned away", async () => {
    const kv = fakeKv({ [`ip:h:${"x".repeat(64)}`]: "3" });
    const message = await checkLimits(kv, {
      ipKey: "x".repeat(64),
      installId: "install",
      limits,
    });
    expect(message).toMatch(/this connection/i);
    // Turned away, but not left without a route.
    expect(message).toMatch(/saving is never limited/i);
  });

  test("an installation over its monthly share is turned away separately", async () => {
    const kv = fakeKv({ "id:m:install": "20" });
    const message = await checkLimits(kv, { ipKey: "ip", installId: "install", limits });
    expect(message).toMatch(/this installation/i);
  });

  test("a flood from many addresses at once trips the global ceiling", async () => {
    // The per-address limits are no defence against a botnet; this is.
    const kv = fakeKv({ "global:h": "25" });
    const message = await checkLimits(kv, { ipKey: "fresh", installId: "fresh", limits });
    expect(message).toMatch(/unusual number of reports/i);
  });

  test("a quiet reporter is not limited at all", async () => {
    const kv = fakeKv();
    expect(await checkLimits(kv, { ipKey: "ip", installId: "install", limits })).toBeNull();
  });

  test("limits can be tuned by configuration without touching the code", () => {
    expect(readLimits({ PER_IP_HOURLY: "1" }).PER_IP_HOURLY).toBe(1);
    // Nonsense configuration falls back rather than disabling the limit.
    expect(readLimits({ PER_IP_HOURLY: "not a number" }).PER_IP_HOURLY).toBe(
      DEFAULTS.PER_IP_HOURLY,
    );
    expect(readLimits({ PER_IP_HOURLY: "0" }).PER_IP_HOURLY).toBe(DEFAULTS.PER_IP_HOURLY);
  });
});

describe("relay end to end", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
  });

  test("a first report becomes an issue and the reporter is told which", async () => {
    const github = vi.fn(async () =>
      new Response(JSON.stringify({ number: 42, html_url: "https://github.com/o/r/issues/42" }), {
        status: 201,
      }),
    );
    vi.stubGlobal("fetch", github);

    const kv = fakeKv();
    const response = await worker.fetch(post(envelope()), env(kv));
    const payload = await response.json();

    expect(response.status).toBe(201);
    expect(payload.issue_url).toBe("https://github.com/o/r/issues/42");
    expect(payload.message).toMatch(/#42/);
    expect(github).toHaveBeenCalledTimes(1);
  });

  test("the same report twice is counted, not filed twice", async () => {
    const github = vi.fn(async () =>
      new Response(JSON.stringify({ number: 7, html_url: "https://github.com/o/r/issues/7" }), {
        status: 201,
      }),
    );
    vi.stubGlobal("fetch", github);

    const kv = fakeKv();
    await worker.fetch(post(envelope()), env(kv));
    const second = await worker.fetch(post(envelope()), env(kv));
    const payload = await second.json();

    expect(second.status).toBe(200);
    expect(payload.duplicate).toBe(true);
    expect(payload.issue_url).toBe("https://github.com/o/r/issues/7");
    expect(github).toHaveBeenCalledTimes(1);
  });

  test("no address is written to the store", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response(JSON.stringify({ number: 1, html_url: "u" }), { status: 201 })),
    );
    const kv = fakeKv();
    await worker.fetch(post(envelope()), env(kv));

    const keys = [...kv.store.keys()].join(" ");
    expect(keys).not.toContain("203.0.113.9");
    expect(keys).toMatch(/ip:h:[0-9a-f]{64}/);
  });

  test("the kill switch answers every report with a route that still works", async () => {
    const github = vi.fn();
    vi.stubGlobal("fetch", github);
    const response = await worker.fetch(post(envelope()), env(fakeKv(), { DISABLED: "1" }));

    expect(response.status).toBe(503);
    expect((await response.json()).message).toMatch(/Save report to a file/i);
    expect(github).not.toHaveBeenCalled();
  });

  test("a GitHub outage is reported as temporary, not as a refusal", async () => {
    // Telling someone their report was rejected when the tracker was merely
    // down sends them away for good.
    vi.stubGlobal("fetch", vi.fn(async () => new Response("nope", { status: 500 })));
    const response = await worker.fetch(post(envelope()), env(fakeKv()));

    expect(response.status).toBe(502);
    expect((await response.json()).message).toMatch(/try again shortly/i);
  });

  test("a rate-limited reporter gets a 429 and no issue is opened", async () => {
    const github = vi.fn();
    vi.stubGlobal("fetch", github);
    const kv = fakeKv({ "global:h": "25" });
    const response = await worker.fetch(post(envelope()), env(kv));

    expect(response.status).toBe(429);
    expect(github).not.toHaveBeenCalled();
  });

  test("anything but a POST is refused before any work is done", async () => {
    const response = await worker.fetch(
      new Request("https://relay.example/report"),
      env(fakeKv()),
    );
    expect(response.status).toBe(405);
  });
});
