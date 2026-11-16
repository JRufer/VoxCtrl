# Clients Outside This Repository

The VoxCtrl server is used by the VoxCtrl app in this repository, and also by
clients that live elsewhere and are released on their own schedule. This page
is for whoever changes the server: it says which clients exist, exactly what
each one relies on, and what to check before changing it.

The app in this repository is covered by `client_contract_tests.rs`, which
runs its real client code against the server on every build. A client in
another repository gets no such protection: nothing here fails to compile when
its contract is broken. **This page is that protection.**

**Contents**

1. [Known clients](#known-clients)
2. [What clients rely on](#what-clients-rely-on)
3. [Before you change the server](#before-you-change-the-server)
4. [Safe changes and breaking changes](#safe-changes-and-breaking-changes)
5. [Client contract revisions](#client-contract-revisions)
6. [When a client changes](#when-a-client-changes)
7. [Adding a client to this page](#adding-a-client-to-this-page)

---

## Known clients

| Client | Repository | What it is | Uses |
|---|---|---|---|
| **VoxCtrl Keys** | [JRufer/voxctrl-keys](https://github.com/JRufer/voxctrl-keys) | A voice keyboard for Android: a thin client that records on the phone and has the server transcribe. Usually reaches the server over Tailscale | Speech recognition only; pairing codes; certificate pinning |

Its side of the contract is written down in its own repository:
[`docs/protocol.md`](https://github.com/JRufer/voxctrl-keys/blob/main/docs/protocol.md)
(every request it makes) and
[`docs/sync-with-voxctrl.md`](https://github.com/JRufer/voxctrl-keys/blob/main/docs/sync-with-voxctrl.md)
(the same checklist as this page, from the other side).

## What clients rely on

Each row is something a released client depends on. The "Defined in" column
is where to look in this repository; a change to one of those files is the
trigger for the checklist below.

| # | What | Defined in | Relied on by |
|---|---|---|---|
| 1 | `GET /health` needs no key and answers a JSON object. `version` and `model` are strings; `cleanup` and `tts` are booleans | `crates/voxctrl-server/src/http.rs` (`health`) | VoxCtrl Keys (status, version check) |
| 2 | `GET /v1/models` with `Authorization: Bearer <key>` answers 200 when the key is accepted and 401 when it is not; `data[0].id` names the model | `http.rs` (`models`, `require_key`) | VoxCtrl Keys (proving the key) |
| 3 | `POST /v1/audio/transcriptions` accepts `multipart/form-data` with a `Content-Length`, a `file` part holding a WAV file, and optional `response_format=json`, `prompt` and `language`; answers 200 with a JSON object whose `text` is a string | `http.rs` (`transcriptions`) | VoxCtrl Keys (every dictation, and live text) |
| 4 | WAV input as 16 kHz, mono, 16-bit PCM is accepted | `crates/voxctrl-server/src/audio.rs` | VoxCtrl Keys |
| 5 | Errors use the envelope `{"error": {"message", "type"}}`, and `message` is written to be shown to a person as is. A refused key is 401 | `http.rs` (`ApiError`) | VoxCtrl Keys (shows `message` unchanged) |
| 6 | A pairing code is `vxpair_` followed by unpadded URL-safe base64 of JSON with `h` (optional host), `p` (port), `k` (optional API key) and `f` (optional fingerprint, 64 hex digits, present exactly when the server encrypts). Unknown keys are ignored by clients | `crates/voxctrl-pairing/src/lib.rs` (`PairingCode`) | VoxCtrl Keys |
| 7 | The fingerprint is the SHA-256 of the server certificate's DER bytes. A paired client accepts that one certificate, does not check the name in it, and never falls back to `http://` | `voxctrl-pairing` (`fingerprint`, `PinnedCertificate`, `pin_for`); `crates/voxctrl-server/src/tls.rs` | VoxCtrl Keys |
| 8 | A bare host typed by a user means `http://<host>:8787/v1`; the default port is 8787 | `src/lib/Settings/server-setup.ts` (`normalizeServerAddress`); `crates/voxctrl-inference/src/remote_openai.rs` (`normalize_*_url`) | VoxCtrl Keys (completes addresses the same way) |
| 9 | Live text is done by sending the recording so far to `/v1/audio/transcriptions` repeatedly, one request at a time. There is no separate live endpoint | [Live text and server load](./server.md#live-text-and-server-load) | VoxCtrl Keys |
| 10 | The server handles one transcription at a time and queues the rest | `crates/voxctrl-server/src/engine.rs` | VoxCtrl Keys (a final upload waits behind a live clip) |

VoxCtrl Keys does **not** use `/v1/cleanup`, `/v1/chat/completions`,
`/v1/audio/speech` or `/v1/voices`, and does not send `X-VoxCtrl-Client`.
Those can change without affecting it.

## Before you change the server

If a pull request touches a file in the "Defined in" column:

1. **Find the rows it touches** in the table above.
2. **Decide whether a released client would notice.** The lists in the next
   section cover the common cases. When in doubt, read the client's
   `docs/protocol.md`: it states what the client sends and what it reads, field
   by field.
3. **If no client would notice**, carry on. If the change adds something
   clients could use (a new field, a new endpoint), nothing more is needed
   here.
4. **If a client would notice:**
   - Prefer a compatible form of the change: accept the old request as well as
     the new one, keep the old field alongside the new one.
   - If it cannot be made compatible, open the matching change in the client's
     repository first, and release the client **before or together with** the
     server. A user must never be left with a server their keyboard cannot use.
   - Raise the [client contract revision](#client-contract-revisions) here and
     in the client, and add a row to the revision table.
   - Say so in the release notes: which clients need updating, and to which
     version.
5. **Update this page** if the change alters any row of the table, including
   its "Defined in" column when code moves.

## Safe changes and breaking changes

**Safe.** No released client notices these:

- A new endpoint.
- A new field in `/health`, in a transcription answer, or in the error object.
- A new key in the pairing code's JSON.
- A new optional request field on `/v1/audio/transcriptions`.
- Accepting more audio formats, larger uploads, or chunked uploads as well.
- New engines, models and server settings.
- Anything about cleanup, AI processing, text-to-speech or voices.

**Breaking.** Each of these stops a released VoxCtrl Keys from working:

- Renaming or removing `text` in the transcription answer, or answering in a
  form other than JSON when `response_format=json` is sent.
- Requiring a request field it does not send. It sends `file`,
  `response_format`, and optionally `prompt` and `language`.
- Refusing 16 kHz mono 16-bit PCM WAV, or an upload with a `Content-Length`.
- Changing the `Authorization: Bearer` scheme, or requiring another header
  (such as `X-VoxCtrl-Client`) on `/health`, `/v1/models` or
  `/v1/audio/transcriptions`.
- Moving any of those three paths.
- Answering a refused key with a status other than 401 or 403.
- Changing the pairing code's prefix or encoding, or the meaning of `h`, `p`,
  `k` or `f`.
- Changing what the fingerprint is computed over, or presenting a certificate
  other than the one whose fingerprint the pairing code carries.
- Turning `/health` into an endpoint that needs a key is **not** breaking for
  VoxCtrl Keys, which treats a failed `/health` as "no details" and goes on to
  `/v1/models`, but it would lose its version check.

## Client contract revisions

A revision names one version of rows 1 to 7 above. A client records the
revision it was written against (VoxCtrl Keys shows it on its settings screen)
so that a bug report says which contract a build speaks.

| Revision | Server release | What changed |
|---|---|---|
| 1 | 0.9.0 (endpoints), 0.9.15 (pairing codes and encryption) | The contract as described on this page |

Raise the revision only for a breaking change. Additions do not count.

## When a client changes

A client that starts using more of the server (say, VoxCtrl Keys adding
dictation cleanup through `/v1/cleanup`) should open a pull request here that
adds the rows it now relies on, so the next person to change that endpoint
knows it has a second user. If the client needs something the server does not
offer, the server change is made here first; app and server changes stay in
this repository, client changes in the client's.

## Adding a client to this page

For a new client in another repository:

1. Add it to [Known clients](#known-clients) with a link to where its side of
   the contract is documented.
2. Add its name to the "Relied on by" column of every row it uses, and add
   rows for anything it relies on that is not listed.
3. Add what would break it to [Safe changes and breaking changes](#safe-changes-and-breaking-changes).
