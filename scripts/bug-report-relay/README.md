# Bug-report relay

This is the small service behind **Settings → Bug Report → Send report**. It
exists for one reason:

> **GitHub has no anonymous issue creation.** `POST /repos/{owner}/{repo}/issues`
> requires a credential, and there is no unauthenticated equivalent. A user
> without a GitHub account cannot open an issue, full stop.

The obvious workaround — ship a token inside VoxCtrl — is not one. A token in a
distributed binary is a public token: anyone can extract it in a minute, and the
first thing it gets used for is the spam this whole feature is trying to avoid.
Revoking it then breaks reporting for everyone.

So the credential lives here, on infrastructure the maintainer controls. VoxCtrl
posts a finished report; this decides whether to file it, using its own token.
The reporter needs no account and never sees the credential.

**VoxCtrl works without this.** With no relay deployed, the Bug Report page
hides the Send button and offers **Save to a file**, **Copy**, **Open on
GitHub** (which prefills GitHub's own new-issue form, for reporters who do have
an account) and **Email it**. Those need no infrastructure at all. The relay
only adds the account-free one-click route.

---

## What it does

1. Refuses anything that is not a well-formed report: unknown schema, missing
   summary, a description under 30 characters or made of one repeated
   character, a body over 60,000 characters, a request over 256 KB.
2. Applies the rate limits that actually bind — see below.
3. Recognises a report already filed, counts it, and returns the existing issue
   instead of opening a duplicate. At 5, 10, 25, 50 and 100 reports of the same
   bug it leaves one comment saying so; the priority signal is worth having and
   fifty "me too" comments are not.
4. Opens the issue with its own token, labelled `bug,from-app,needs-triage`.

## Why the limits live here and not in the app

VoxCtrl has its own limits (2 minutes between reports, 5 a day, 20 a month,
duplicates refused). Those run on the reporter's machine, in a file they can
delete, in a binary they can patch. **They stop an accident, not an attacker** —
a stuck button, a crash reported at every launch, the same report sent twice out
of frustration. They are there so the app can explain *why* it is not sending,
which a silent HTTP 429 cannot.

The limits here see every reporter at once, and are the ones that count:

| Limit | Default | Purpose |
|---|---|---|
| Per address, hourly | 3 | One person hammering the button |
| Per address, daily | 8 | One person over a day |
| Per installation, daily | 5 | Mirrors the app's own limit, server-side |
| Per installation, monthly | 20 | Same |
| **Global, hourly** | 25 | A flood from many addresses at once — the only thing that stops a botnet |
| Kill switch (`DISABLED`) | off | One deploy turns the channel off entirely |

Addresses are stored **hashed** with a salt you set, so the store holds no IP
addresses — only counters and an index of which report hashes have been seen.

Every refusal returns a sentence VoxCtrl shows verbatim, and every one of them
points at **Save report to a file**, which is never rate-limited. Someone with a
real bug always has a way to get it to you.

## Deploying it

Free-tier Cloudflare Workers is plenty; this is a handful of requests a day.

```sh
npm install -g wrangler
wrangler login

cd scripts/bug-report-relay

# 1. Storage for the counters and the seen-report index.
wrangler kv namespace create REPORTS
#    Put the id it prints into wrangler.toml.

# 2. The GitHub credential. Use a *fine-grained* personal access token,
#    scoped to JRufer/VoxCtrl only, with Issues: Read and write and
#    nothing else. Do not use a classic token: the narrowest classic
#    scope that can open issues is `repo`, which is write access to
#    everything you own.
wrangler secret put GITHUB_TOKEN

# 3. A random string. It salts the address hashes, so the stored
#    hashes cannot be reversed by trying every address.
wrangler secret put IP_SALT

wrangler deploy
```

Then point VoxCtrl at it. The endpoint is baked in at build time, so that a
config edit cannot redirect anyone's reports somewhere else:

```sh
VOXCTRL_BUGREPORT_ENDPOINT=https://voxctrl-bug-report.<your-subdomain>.workers.dev \
  cargo tauri build
```

Set the same variable in the release workflow's build step to ship it in the
release artifacts. Leave it unset for a build with no automatic submission.

### Check it before shipping it

```sh
curl -X POST https://voxctrl-bug-report.<your-subdomain>.workers.dev \
  -H 'Content-Type: application/json' \
  -d '{"schema":1,"title":"[Linux] Relay smoke test","body":"### What happened\n\nSmoke test.",
       "fingerprint":"'"$(head -c32 /dev/urandom | sha256sum | cut -d" " -f1)"'",
       "install_id":"smoke","app_version":"0.5.1","os":"linux",
       "report":{"statement":{"description":"Smoke testing the relay before shipping it to users."}}}'
```

A `201` with an issue URL means it is working. Close the issue you just made.

## Running the tests

The relay's refusal paths are covered by the project's frontend test suite:

```sh
npm run test:unit -- tests/relay/worker.test.ts
```

## If it is abused anyway

1. Set `DISABLED = "1"` in `wrangler.toml` and `wrangler deploy`. Reports stop
   immediately; users are told to save and send by hand.
2. Every issue the relay opened carries the `from-app` label, so the whole
   channel's output can be filtered, triaged or bulk-closed as a group.
3. Lower `GLOBAL_HOURLY` and redeploy if you would rather throttle than stop.
