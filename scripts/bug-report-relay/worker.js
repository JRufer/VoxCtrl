/**
 * VoxCtrl bug-report relay.
 *
 * GitHub has no anonymous issue creation: `POST /repos/{owner}/{repo}/issues`
 * needs a credential, and there is no unauthenticated equivalent. Putting a
 * token in the app would put it in everyone's hands, and the first thing it
 * would be used for is the spam this exists to prevent.
 *
 * So the credential lives here instead, on a server the maintainer controls.
 * VoxCtrl posts a finished report; this decides whether to file it. The
 * reporter needs no GitHub account and never sees the token.
 *
 * This is also where the abuse limits that actually bind live. VoxCtrl's own
 * limits run on the reporter's machine, in a file they can delete — they stop
 * an accident, not an attacker. These ones see every reporter at once.
 *
 * Deploy: see README.md in this directory.
 */

const MAX_BODY_BYTES = 256 * 1024;
const MAX_TITLE_CHARS = 160;
const MAX_ISSUE_BODY_CHARS = 60_000;
const SUPPORTED_SCHEMAS = [1];

/** Limits, in requests per window. Tune in wrangler.toml, not here. */
const DEFAULTS = {
  PER_IP_HOURLY: 3,
  PER_IP_DAILY: 8,
  PER_INSTALL_DAILY: 5,
  PER_INSTALL_MONTHLY: 20,
  // A ceiling on issues opened from the app across every reporter, so a
  // distributed flood costs one hour of the tracker rather than all of it.
  GLOBAL_HOURLY: 25,
};

const HOUR = 3600;
const DAY = 86_400;
const MONTH = 30 * DAY;

export default {
  async fetch(request, env) {
    if (request.method === "OPTIONS") return preflight();
    if (request.method !== "POST") {
      return json(405, { message: "Send a bug report with POST." });
    }
    if (env.DISABLED === "1") {
      return json(503, {
        message:
          "Automatic bug reporting is switched off at the moment. Please use " +
          "“Save report to a file” and send it along instead.",
      });
    }

    let envelope;
    try {
      envelope = await readEnvelope(request);
    } catch (e) {
      return json(400, { message: e.message });
    }

    const rejection = validate(envelope);
    if (rejection) return json(400, { message: rejection });

    const limits = readLimits(env);
    const ip = request.headers.get("CF-Connecting-IP") || "unknown";
    const installId = String(envelope.install_id || "").slice(0, 64) || "unknown";

    // Hashed, so the store holds no addresses. It only ever has to answer
    // "have I seen this one before", and a hash answers that.
    const ipKey = await sha256Hex(`${ip}:${env.IP_SALT || "voxctrl"}`);

    const overLimit = await checkLimits(env.REPORTS, { ipKey, installId, limits });
    if (overLimit) return json(429, { message: overLimit });

    // A report already filed gets counted, not re-filed. The count is worth
    // having: five people hitting the same bug is a different priority from
    // one, and it is the only signal an anonymous reporting channel gives.
    const seen = await env.REPORTS.get(`fp:${envelope.fingerprint}`, { type: "json" });
    if (seen) {
      const count = (seen.count || 1) + 1;
      await env.REPORTS.put(
        `fp:${envelope.fingerprint}`,
        JSON.stringify({ ...seen, count }),
        { expirationTtl: MONTH },
      );
      await maybeNoteRecurrence(env, seen.issue_number, count);
      return json(200, {
        duplicate: true,
        issue_url: seen.issue_url || null,
        message:
          "Thank you — this matches a report already filed, so it has been added to " +
          "that one rather than opening a duplicate.",
      });
    }

    let issue;
    try {
      issue = await createIssue(env, envelope);
    } catch (e) {
      // The report is not lost: VoxCtrl tells the reporter to save it.
      return json(502, {
        message:
          "The bug tracker would not accept the report just now. Please try again shortly, " +
          "or use “Save report to a file” and send it along.",
      });
    }

    await Promise.all([
      env.REPORTS.put(
        `fp:${envelope.fingerprint}`,
        JSON.stringify({ count: 1, issue_url: issue.html_url, issue_number: issue.number }),
        { expirationTtl: MONTH },
      ),
      bump(env.REPORTS, `ip:h:${ipKey}`, HOUR),
      bump(env.REPORTS, `ip:d:${ipKey}`, DAY),
      bump(env.REPORTS, `id:d:${installId}`, DAY),
      bump(env.REPORTS, `id:m:${installId}`, MONTH),
      bump(env.REPORTS, `global:h`, HOUR),
    ]);

    return json(201, {
      issue_url: issue.html_url,
      message: `Thank you — your report is now issue #${issue.number}.`,
    });
  },
};

// ── Reading and checking the request ─────────────────────────────────────────

async function readEnvelope(request) {
  const declared = Number(request.headers.get("Content-Length") || 0);
  if (declared > MAX_BODY_BYTES) {
    throw new Error("That report is too large to accept.");
  }
  const text = await request.text();
  if (text.length > MAX_BODY_BYTES) {
    throw new Error("That report is too large to accept.");
  }
  try {
    return JSON.parse(text);
  } catch {
    throw new Error("That did not look like a bug report.");
  }
}

/** Returns a sentence for the user when the report cannot be filed, else null. */
function validate(envelope) {
  if (!SUPPORTED_SCHEMAS.includes(Number(envelope.schema))) {
    return (
      "This version of VoxCtrl sends reports in a format this service no longer " +
      "understands. Please update, or save the report to a file and send it along."
    );
  }
  const title = String(envelope.title || "").trim();
  const body = String(envelope.body || "");
  if (title.length < 3) return "Please give the report a one-line summary.";
  if (title.length > MAX_TITLE_CHARS) return "That summary is too long.";
  if (body.length > MAX_ISSUE_BODY_CHARS) return "That report is too large to accept.";
  if (!/^[0-9a-f]{16,128}$/.test(String(envelope.fingerprint || ""))) {
    return "That did not look like a bug report.";
  }

  const description = String(envelope.report?.statement?.description || "").trim();
  if (description.length < 30) {
    return "Please describe what happened in a little more detail.";
  }
  // A wall of one repeated character is the shape of a keyboard left face-down,
  // not of a bug report.
  if (new Set(description.replace(/\s/g, "")).size < 5) {
    return "Please describe what happened in words.";
  }
  return null;
}

function readLimits(env) {
  const limits = { ...DEFAULTS };
  for (const key of Object.keys(DEFAULTS)) {
    const configured = Number(env[key]);
    if (Number.isFinite(configured) && configured > 0) limits[key] = configured;
  }
  return limits;
}

async function checkLimits(kv, { ipKey, installId, limits }) {
  const [ipHour, ipDay, idDay, idMonth, globalHour] = await Promise.all([
    count(kv, `ip:h:${ipKey}`),
    count(kv, `ip:d:${ipKey}`),
    count(kv, `id:d:${installId}`),
    count(kv, `id:m:${installId}`),
    count(kv, `global:h`),
  ]);

  if (globalHour >= limits.GLOBAL_HOURLY) {
    return (
      "This service is handling an unusual number of reports right now and has paused. " +
      "Please try again in an hour, or save the report to a file and send it along."
    );
  }
  if (ipHour >= limits.PER_IP_HOURLY || ipDay >= limits.PER_IP_DAILY) {
    return (
      "That is as many reports as this connection can send for now. Please try again later, " +
      "or use “Save report to a file” — saving is never limited."
    );
  }
  if (idDay >= limits.PER_INSTALL_DAILY || idMonth >= limits.PER_INSTALL_MONTHLY) {
    return (
      "That is as many reports as this installation can send for now. Please use " +
      "“Save report to a file” and send it along instead."
    );
  }
  return null;
}

async function count(kv, key) {
  return Number((await kv.get(key)) || 0);
}

/**
 * Add one to a counter.
 *
 * Deliberately not atomic: KV has no increment, and two reports landing in the
 * same millisecond may cost one slot instead of two. That is the right way to
 * be wrong here — a rate limiter that occasionally lets one extra report
 * through is a nuisance, and one that needs a Durable Object per reporter is a
 * bill. Move to Durable Objects if this ever stops being true.
 */
async function bump(kv, key, ttl) {
  const next = (await count(kv, key)) + 1;
  await kv.put(key, String(next), { expirationTtl: ttl });
}

// ── Talking to GitHub ────────────────────────────────────────────────────────

async function createIssue(env, envelope) {
  const labels = (env.ISSUE_LABELS || "bug,from-app,needs-triage")
    .split(",")
    .map((l) => l.trim())
    .filter(Boolean);

  const response = await github(env, `/repos/${env.GITHUB_REPO}/issues`, {
    method: "POST",
    body: JSON.stringify({
      title: envelope.title,
      body: envelope.body,
      labels,
    }),
  });
  if (!response.ok) {
    throw new Error(`GitHub responded ${response.status}`);
  }
  return response.json();
}

/**
 * Leave a note on an existing issue when a bug turns out to be widespread.
 *
 * Only at thresholds, so a bug fifty people hit does not become an issue with
 * fifty comments saying "someone else hit this too".
 */
async function maybeNoteRecurrence(env, issueNumber, count) {
  const milestones = [5, 10, 25, 50, 100];
  if (!issueNumber || !milestones.includes(count)) return;
  try {
    await github(env, `/repos/${env.GITHUB_REPO}/issues/${issueNumber}/comments`, {
      method: "POST",
      body: JSON.stringify({
        body: `${count} separate installations have now reported this from VoxCtrl's Bug Report page.`,
      }),
    });
  } catch {
    // A missing note is not worth failing a report over.
  }
}

function github(env, path, init) {
  return fetch(`https://api.github.com${path}`, {
    ...init,
    headers: {
      Authorization: `Bearer ${env.GITHUB_TOKEN}`,
      Accept: "application/vnd.github+json",
      "X-GitHub-Api-Version": "2022-11-28",
      "Content-Type": "application/json",
      "User-Agent": "voxctrl-bug-report-relay",
    },
  });
}

// ── Plumbing ─────────────────────────────────────────────────────────────────

async function sha256Hex(text) {
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

function json(status, payload) {
  return new Response(JSON.stringify(payload), {
    status,
    headers: {
      "Content-Type": "application/json",
      "Cache-Control": "no-store",
      ...corsHeaders(),
    },
  });
}

function preflight() {
  return new Response(null, { status: 204, headers: corsHeaders() });
}

function corsHeaders() {
  return {
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Methods": "POST, OPTIONS",
    "Access-Control-Allow-Headers": "Content-Type, X-VoxCtrl-Version, X-VoxCtrl-Schema",
    "Access-Control-Max-Age": "86400",
  };
}

// Exported for the tests in worker.test.js.
export const __test__ = { validate, checkLimits, readLimits, DEFAULTS };
