# Bug Reports

**Settings → Bug Report** collects what is needed to diagnose a problem and
sends it — but only when you press a button, and only after showing you exactly
what it is about to send.

This page is the long version of what that tab says. It exists so you can check
the claims rather than take them on trust, and every claim below names the file
that enforces it.

---

## Why this exists

VoxCtrl 0.5.0 shipped a Windows build for the first time, and a Windows GPU
build followed it. Both were built by one person on one machine. If something is
broken on yours, there is currently no way for anyone to find out except you
saying so — and a report that says "it doesn't work" cannot be acted on, while
one that says which build, which OS, which engine and what the log said usually
can.

---

## What is sent

Only when you press a button on the Bug Report page. Nothing here runs at
startup, on a timer, or in the background.

### Your words

The summary and description you type. **This is the only text of yours that ever
travels.** Everything else is either a fixed value from a list, a number, or a
count.

Two things happen to it on the way out, both visible in the preview:
`@name` and `#123` get an invisible character after the sigil, so a report
cannot be used to notify people or cross-link into unrelated issues; and text
that looks like an HTML tag is escaped, so a stray `</details>` cannot collapse
the rest of the report. (`crates/voxctrl-bugreport/src/report.rs`)

### Your machine

A fixed list — not "whatever a system-information library returns", so that this
list stays true when dependencies are upgraded
(`crates/voxctrl-bugreport/src/sysinfo.rs`):

- VoxCtrl's version, how it was installed (AppImage, installed, development),
  and which Cargo features it was built with
- What this build can put on the GPU, for whisper.cpp and Moonshine
- Operating system, distribution or Windows edition, kernel or NT build,
  CPU architecture
- CPU model, logical core count, total memory, display adapter name
- On Linux: desktop environment and whether the session is X11 or Wayland
- Your language, as a two-letter code such as `en` — speech recognition quality
  depends on it

### Your settings

Serialised, then redacted (`crates/voxctrl-bugreport/src/redact.rs`):

| Kind of setting | What appears in the report |
|---|---|
| Numbers and switches | Kept exactly — these explain most bugs, and a number cannot carry a name |
| Fixed choices (model size, engine, device, key names) | Kept |
| API keys and access tokens | `<set, not included>` or `<not set>` |
| Folder settings | `<platform default>` or `<custom path>` — never the path |
| Prompts, custom vocabulary, snippets | `<3 entries, not included>` — counted, never quoted |
| Server addresses | `http://localhost:11434`, `http://<private address>:1234` or `https://<remote host>` — never a hostname, path, query or credential |

### Your output targets and hotkeys

The **shape**, not the contents: how many targets, of which delivery kinds, with
which options set, and which key combinations are bound to which of them.
Labels, shell commands, file paths, URLs and webhook secrets are all replaced
with `<set>`. Target names become `target-1`, `target-2` and so on, so a binding
still points at a recognisable target without carrying the name you gave it.

Key **names** are included, because "which combination did you register" is the
whole question when a hotkey does not fire. Key **presses** are not, and cannot
be: VoxCtrl never receives them — see [Privacy](privacy.md#keystrokes).

### The log

The last 400 lines (at most 64 KB) of `startup_errors.log`, from
`~/.local/share/voxctrl/` on Linux or `%LOCALAPPDATA%\voxctrl\` on Windows. The
exact path is shown on the Bug Report page so you can read it first.

That file is already written with this in mind: `src-tauri/src/startup_log.rs`
drops any line whose message mentions transcription, speech or payloads *before
it is written*, so dictated text cannot be in the file and therefore cannot be
in a report.

---

## What is never sent

- **Anything you have dictated.** No transcript, no fragment, no audio.
- **Audio.** No recording, no sample, no device fingerprint.
- **API keys or access tokens** — OpenAI, HuggingFace, chat targets, webhook
  secrets. Only whether one is set.
- **Your name, username, hostname, email address or IP address.** Home
  directories become `~` and your account name becomes `<user>`, in settings and
  in every log line (`crates/voxctrl-bugreport/src/scrub.rs`).
- **File paths.**
- **Custom vocabulary, snippets or prompts.**
- **Target labels, shell commands, URLs or webhook secrets.**
- **Anything about other applications**, running processes, or your files.

### The rule that keeps this true

Redaction is an allowlist, not a blocklist. Every text-bearing setting is
classified by hand in `CONFIG_HANDLING`, and anything unclassified is **omitted**
rather than guessed at.

`crates/voxctrl-bugreport/tests/config_coverage.rs` walks the real `AppConfig`
and fails the build naming any setting nobody has classified. So a new setting
cannot leak into a public issue by being forgotten — the worst it can do is go
missing from reports until someone classifies it, and CI says so first.

---

## How a report reaches the maintainer

You will notice there are several routes. That is because of a hard constraint:

> **GitHub does not allow anonymous issue creation.** Opening an issue requires
> an authenticated account, and there is no unauthenticated equivalent.

And the obvious workaround is worse than the problem: a GitHub token shipped
inside VoxCtrl would be extractable by anyone who downloads it, and the first
use anyone would find for it is the spam this feature is meant to avoid.

So there are four routes, and you pick:

### Send report — no account needed

Posts the report to a small [relay](../scripts/bug-report-relay/README.md) the
maintainer runs. The relay holds the GitHub credential, applies the abuse limits,
and opens the issue itself. You need no GitHub account and never touch a token.

This button only appears in builds where a relay endpoint was compiled in. It is
baked in at build time so that a config edit cannot redirect your report
somewhere else.

### Open on GitHub — for reporters who have an account

Opens GitHub's own new-issue form in your browser with everything filled in.
**Nothing is sent until you read it and press Submit on GitHub's page.** If the
report is too long for a link, the page says so and tells you to use **Copy
report** and paste.

### Save report to a file

Writes the whole report as Markdown wherever you choose. Attach it to an email,
put it in a forum post, hand it over on a USB stick. This route is never
rate-limited and never needs a network.

### Email it

Opens your mail client with a covering note and the report ID. The report itself
goes as the saved file, attached by hand — mail clients truncate long message
bodies without saying so, and half a report is worse than none.

---

## Limits, and the ID that enforces them

A public "file an issue" button is a spam vector, so there are limits in two
places.

**In VoxCtrl**, so it can tell you why it is not sending: two minutes between
reports, five a day, twenty a month, and the same report twice is recognised and
refused. These run on your machine, in a file you can delete
(`crates/voxctrl-bugreport/src/throttle.rs`). They stop a stuck button; they are
not a security measure, and the code says so.

**In the relay**, where the limits that actually bind live: per address, per
installation, and a global hourly ceiling that a flood from many machines cannot
get past — plus a kill switch that turns the channel off in one deploy. See the
[relay README](../scripts/bug-report-relay/README.md).

**Saving to a file is never limited.** Every refusal message says so. If you
have a real bug, you always have a way to report it.

### The report ID

A random 32-character value made on your machine the first time you open the Bug
Report page. It exists so the relay can count reports per installation rather
than only per address (several people behind one address should not share an
allowance), and so a resent report can be recognised.

It is derived from the clock, the process id and a memory address — deliberately
**not** from anything about you or your machine, because a value derived from
those would follow you across reinstalls and would be exactly the tracking
identifier VoxCtrl promises not to have. **Reset ID** on the Bug Report page
throws it away and starts a fresh one.

The local history file stores only a **hash** of each report you have sent, never
the text, so it cannot become a second place your words pile up.

---

## Checking it yourself

```sh
# Every redaction rule, the leak tests, and the limits.
cargo test -p voxctrl-bugreport

# The test that fails when a new setting is unclassified.
cargo test -p voxctrl-bugreport --test config_coverage

# The Bug Report page, and the relay's refusal paths.
npm run test:unit -- tests/svelte/BugReportTab.test.ts tests/relay/worker.test.ts
```

And the one that needs no tooling: fill in the form, press **Show me exactly
what will be sent**, and read it. There is no second, fuller version — the text
in that box is the report.
