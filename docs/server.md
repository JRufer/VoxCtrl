# VoxCtrl Server

`voxctrl-server` runs VoxCtrl's speech models on one machine so that the
VoxCtrl app on another machine doesn't have to. The app keeps doing everything
that needs your desktop — hotkeys, the microphone, the overlay, typing the
result — and sends the audio to the server to be transcribed.

Use it when:

- your laptop is short on RAM, battery or GPU, and a desktop or home server has
  plenty;
- several computers should share one model instead of each loading its own;
- you want a large model without dedicating part of your workstation to it.

The server has no window, no tray icon and no audio device. It is one program
that listens on a network port.

**Contents**

1. [What runs where](#what-runs-where)
2. [Before you start](#before-you-start)
3. [Choose a model](#choose-a-model)
4. [Install with Docker](#install-with-docker)
5. [Install as a system service](#install-as-a-system-service)
6. [Connect the VoxCtrl app](#connect-the-voxctrl-app)
7. [Configuration reference](#configuration-reference)
8. [Security](#security)
9. [GPU acceleration](#gpu-acceleration)
10. [Day-to-day operation](#day-to-day-operation)
11. [API reference](#api-reference)
12. [Troubleshooting](#troubleshooting)

---

## What runs where

The server runs speech recognition, S1-mini dictation cleanup (from 0.9.5) and
text-to-speech (from 0.9.7).

| | Where it runs |
|---|---|
| Hotkeys, microphone, overlay, typing the text | Client (the VoxCtrl app) |
| **Speech recognition** (whisper.cpp, Moonshine, Parakeet) | **Server** |
| Filler removal, spoken punctuation, snippets, custom vocabulary | Client |
| **S1-mini dictation cleanup** | **Server**, when turned on at both ends (see [Dictation cleanup on the server](#dictation-cleanup-on-the-server)); otherwise client |
| The OpenAI-compatible LLM step | Wherever the endpoint in the client's AI settings points |
| **Text-to-speech** | **Server**, when turned on there and the client's TTS engine is "VoxCtrl server" (see [Text-to-speech on the server](#text-to-speech-on-the-server)); otherwise client. The audio is always played on the client |

Cleanup and text-to-speech are each off until you turn them on at both ends,
so a client keeps loading its own S1-mini and TTS voice until then.

Not in this release, and planned for later ones:

- sending custom voices from the client to the server;
- pairing with a code, and encrypted connections built in;

---

## Before you start

**The server machine**

- Linux, x86-64.
- Enough memory for the model you pick: see [Choose a model](#choose-a-model).
- Internet access the first time it starts, to download the model from
  Hugging Face. After that it runs offline.
- A fixed address on your network, so clients can keep finding it. Reserve its
  IP address in your router, or use its hostname.

**The network**

- Clients reach the server on one TCP port, **8787** by default.
- The connection is plain HTTP in this release: the audio is **not encrypted**
  in transit. That is fine on a home or office network you trust. Do not expose
  the port to the internet; for access from outside, see [Security](#security).

**The client**

- Any VoxCtrl version whose **Settings → Engine → Backend** list includes
  **Remote Speech Engine (OpenAI API)**. That existing engine is what talks to
  the server, so nothing new needs installing on the client.

---

## Choose a model

The server runs one model at a time. The engines and model names are the same
as in the app.

| Engine (`backend`) | Models (`model`) | Approx. memory | Notes |
|---|---|---|---|
| `parakeet` | `tdt-0.6b-v3`, `tdt-0.6b-v3-int8`, `tdt-0.6b-v3-redux`, `phonon-2` | ~665 MB (`redux` ~420 MB) | Fast on a CPU; no GPU needed. A good default for a server. |
| `whisper-cpp` | `tiny`, `base`, `small`, `medium`, `large-v2`, `large-v3`, `large-v3-turbo`, and the English-only `tiny.en`, `base.en`, `small.en`, `medium.en` | 75 MB (`tiny`) to 3.1 GB (`large-v3`) | Widest language coverage. `medium` and larger are slow without a GPU. |
| `moonshine` | `tiny`, `base` | ~240–530 MB | Small and fast on a CPU. English. |

The memory figures are the ones in [Speech Recognition](./speech-recognition.md),
which also compares the engines' accuracy.

If you start the server with no settings at all it uses `whisper-cpp` with the
`tiny` model, the same default as the app. That is enough to check everything
works, and not what you want to dictate with: pick a model before real use. The
Docker setup below defaults to `parakeet` / `tdt-0.6b-v3` instead.

---

## Install with Docker

This is the simplest way to run the server on a machine that already has
Docker.

There is no published image yet, so the image is built on your machine from the
source. The build compiles whisper.cpp and the server and can take from several
minutes to half an hour depending on the machine; it only happens on the first
start and after an upgrade.

### 1. Prerequisites

- [Docker Engine](https://docs.docker.com/engine/install/) with the Compose
  plugin (`docker compose version` should print a version).
- `git`.
- A few gigabytes of free disk space for the build, plus the size of your model.

### 2. Get the source

```bash
git clone https://github.com/JRufer/VoxCtrl.git
cd VoxCtrl/packaging/docker
```

### 3. Choose your settings

```bash
cp .env.example .env
nano .env        # or any editor
```

The file is short and commented. The lines most people change:

```ini
VOXCTRL_SERVER_BACKEND=parakeet
VOXCTRL_SERVER_MODEL=tdt-0.6b-v3

# Leave blank and the server generates a key for you (step 5).
VOXCTRL_SERVER_API_KEY=

VOXCTRL_SERVER_PORT=8787
```

### 4. Build and start

```bash
docker compose up -d --build
```

### 5. Watch the first start and collect the API key

```bash
docker compose logs -f
```

On the first start the server downloads the model, loads it, and then prints:

```
INFO voxctrl_server: voxctrl-server 0.9.0 is ready
INFO voxctrl_server:   Model:    parakeet/tdt-0.6b-v3
INFO voxctrl_server:   Endpoint: http://0.0.0.0:8787/v1  (use this machine's address from a client)
INFO voxctrl_server:   API key:  vx_3f9a…
```

**Copy the API key.** It is printed only this once, when it is first generated.
(If you set `VOXCTRL_SERVER_API_KEY` yourself in `.env`, that is the key and it
is not printed.) Press `Ctrl-C` to stop following the log; the server keeps
running.

If you miss it, read it back from the volume at any time:

```bash
docker compose exec voxctrl-server \
  grep '^api_key' /var/lib/voxctrl/config/voxctrl/server.toml
```

### 6. Check it from the server itself

```bash
curl http://localhost:8787/health
# {"model":"parakeet/tdt-0.6b-v3","status":"ok","version":"0.9.0"}
```

`docker compose ps` also shows the container as `healthy` once the model is
loaded. While a first-time download is still running it shows `starting`; that
is expected.

### 7. Open the port

If the server machine runs a firewall, allow the port from your network:

```bash
# ufw (Ubuntu, Debian)
sudo ufw allow from 192.168.1.0/24 to any port 8787 proto tcp

# firewalld (Fedora, RHEL)
sudo firewall-cmd --permanent --add-port=8787/tcp && sudo firewall-cmd --reload
```

Replace `192.168.1.0/24` with your own network's range. Then continue with
[Connect the VoxCtrl app](#connect-the-voxctrl-app).

### Where Docker keeps the data

Everything the server writes is in one named volume, `voxctrl-data`, mounted at
`/var/lib/voxctrl`:

| Path in the container | Contents |
|---|---|
| `/var/lib/voxctrl/config/voxctrl/server.toml` | Settings, including the generated API key |
| `/var/lib/voxctrl/data/voxctrl/models/` | Downloaded models |

The volume survives `docker compose down` and upgrades. `docker compose down -v`
deletes it: the models are downloaded again and a new API key is generated.

### Without Compose

```bash
# From the repository root
docker build -f packaging/docker/Dockerfile -t voxctrl-server .

docker run -d --name voxctrl-server --restart unless-stopped \
  -p 8787:8787 \
  -e VOXCTRL_SERVER_BACKEND=parakeet \
  -e VOXCTRL_SERVER_MODEL=tdt-0.6b-v3 \
  -v voxctrl-data:/var/lib/voxctrl \
  voxctrl-server

docker logs -f voxctrl-server
```

---

## Install as a system service

Use this to run the server directly on the machine, managed by systemd.

There is no prebuilt server binary in the releases yet, so this also builds
from source.

### 1. Install the build tools

```bash
# Debian / Ubuntu
sudo apt install build-essential cmake clang libclang-dev pkg-config libssl-dev git curl

# Arch
sudo pacman -S --needed base-devel cmake clang pkgconf openssl git

# Fedora
sudo dnf install gcc gcc-c++ cmake clang clang-devel pkgconf-pkg-config openssl-devel git
```

And Rust, if it is not installed: <https://rustup.rs>.

### 2. Build

```bash
git clone https://github.com/JRufer/VoxCtrl.git
cd VoxCtrl
cargo build --release -p voxctrl-server
cargo build --release -p voxctrl-llm-sidecar   # only needed for dictation cleanup
```

Only the server and the crates it needs are compiled; the desktop app and its
GTK/WebKit dependencies are not. The results are `target/release/voxctrl-server`
and `target/release/voxctrl-llm-sidecar`. The second is the helper that runs
the cleanup model; skip it if you won't turn on
[cleanup](#dictation-cleanup-on-the-server).

The default build includes all three engines and downloads ONNX Runtime while
compiling (for Moonshine and Parakeet). For a whisper.cpp-only build that needs
no network access to compile, add `--no-default-features`.

### 3. Try it in the foreground

```bash
./target/release/voxctrl-server
```

It writes a config file to `~/.config/voxctrl/server.toml`, generates an API
key and prints it, downloads the default model, and starts listening. Check it
with `curl http://localhost:8787/health`, then stop it with `Ctrl-C`.

### 4. Install it as a service

```bash
sudo install -m 755 target/release/voxctrl-server /usr/local/bin/voxctrl-server
# If you built it: the server looks for the cleanup helper beside its own binary.
sudo install -m 755 target/release/voxctrl-llm-sidecar /usr/local/bin/voxctrl-llm-sidecar
sudo useradd --system --create-home --home-dir /var/lib/voxctrl \
     --shell /usr/sbin/nologin voxctrl
sudo install -m 644 packaging/systemd/voxctrl-server.service /etc/systemd/system/
```

### 5. Choose a model

The service runs as the `voxctrl` user, whose config file is
`/var/lib/voxctrl/.config/voxctrl/server.toml`. Create it before the first
start:

```bash
sudo -u voxctrl mkdir -p /var/lib/voxctrl/.config/voxctrl
sudo -u voxctrl tee /var/lib/voxctrl/.config/voxctrl/server.toml > /dev/null <<'EOF'
bind = "0.0.0.0:8787"

[engine]
backend = "parakeet"

[engine.parakeet]
model_size = "tdt-0.6b-v3"
EOF
```

Any setting you leave out takes its default. See the
[Configuration reference](#configuration-reference) for all of them.

### 6. Start it

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now voxctrl-server
sudo journalctl -u voxctrl-server -f
```

Wait for `voxctrl-server 0.9.0 is ready` and **copy the API key** from the line
below it. It is printed once; afterwards it is in the config file:

```bash
sudo grep '^api_key' /var/lib/voxctrl/.config/voxctrl/server.toml
```

Open the port in your firewall as in [step 7 of the Docker
instructions](#7-open-the-port), then connect a client.

---

## Connect the VoxCtrl app

Do this on each computer that should use the server. You need the server's
address and the API key.

1. Open **Settings → Engine**.
2. Under **Backend**, choose **Remote Speech Engine (OpenAI API)**.
3. Fill in **Remote Speech Engine Settings**:

   | Field | Value |
   |---|---|
   | **Endpoint URL** | `http://<server address>:8787/v1` — for example `http://192.168.1.50:8787/v1` |
   | **API Key** | The key from the server's log or `server.toml` |
   | **Model** | Anything. The server runs the model it was configured with and ignores this name. After a test, the server's actual model appears as a chip you can click so the field shows it. |
   | **Language** | A language code such as `en`, or blank to use the server's setting |
   | **Timeout** | 30 seconds is fine on a local network. Raise it if you run a large model on a CPU. |

4. Click **Test Connection**. A success message means the address, the key and
   the server are all good.
5. Save, then dictate as usual.

The first-run wizard offers the same engine and the same fields, so a new
install can be pointed at the server during setup.

The same settings in `~/.config/voxctrl/config.json`:

```json
{
  "engine": {
    "backend": "remote-openai",
    "remote_openai": {
      "endpoint": "http://192.168.1.50:8787/v1",
      "api_key": "vx_…",
      "model": "parakeet/tdt-0.6b-v3",
      "language": "",
      "timeout_secs": 30,
      "fallback": "parakeet"
    }
  }
}
```

### Limitations on the client

- **Live text needs VoxCtrl 0.9.2 or newer on the client.** From 0.9.2 the
  overlay shows words as you speak with a remote engine too: while you talk,
  the app sends the server short clips of what you have said so far, a few
  times a second, alongside the dictation itself. Older clients show the text
  only when you finish. See [Live text and server load](#live-text-and-server-load).
- **A server outage needs a fallback to ride through.** Without one, dictation
  fails with an error until the server is back. See the next section.
- **Vocabulary and snippets still work.** The client sends your custom
  vocabulary as a hint with each request and applies snippets, punctuation and
  cleanup to the result itself, exactly as it does with a local engine.

### When the server can't be reached

Set a **Fallback engine** in the same settings group and the app keeps working
through an outage:

1. In **Settings → Engine → Remote Speech Engine Settings**, choose a
   **Fallback engine**: Whisper.cpp, Moonshine or Parakeet. Each is listed with
   the model it would run, which is the one chosen in that engine's own
   settings. If that model isn't downloaded, a warning with a **Download**
   button appears.
2. Save.

From then on:

- The fallback model is **not loaded** while the server is reachable. It costs
  disk space and nothing else.
- If the server can't be reached, the dictation you just made is transcribed by
  the fallback instead. Nothing you said is lost; the first one takes a little
  longer while the model loads.
- The tray icon shows an **amber badge with a slash**, its tooltip gives the
  reason, and one notification tells you the fallback is in use. **Settings →
  Engine** shows the same status, and the overlay shows a small **OFFLINE**
  mark while you dictate.
- The app checks the server every 15 seconds. When it answers, the badge
  clears, the next dictation goes back to the server, and the fallback model is
  unloaded.

With **Fallback engine** set to **None**, the badge and notification still
appear, and each dictation fails with an error until the server is back.

To change which model the fallback runs: switch **Backend** to that engine,
pick the model, switch **Backend** back to **Remote Speech Engine (OpenAI
API)**, and save.

The fallback needs a client that has it (VoxCtrl 0.9.2 or newer; 0.9.0 gained
it only after its first publication, so an early 0.9.0 does not). Details are
in [Speech Recognition](./speech-recognition.md#fallback-when-the-server-is-unreachable).

---

### Dictation cleanup on the server

S1-mini dictation cleanup (the model that turns a raw transcript into
punctuated, tidied text) can run on the server instead of on each client, so a
client loads no language model at all. It needs VoxCtrl 0.9.5 or newer at both
ends, and has to be turned on at both.

**On the server**, turn cleanup on and restart:

- Docker: set `VOXCTRL_SERVER_CLEANUP=true` in `packaging/docker/.env`, then
  `docker compose up -d --build`. An image built before 0.9.5 has to be
  rebuilt; the new one also contains the cleanup helper.
- System service: add this to `server.toml`, then
  `sudo systemctl restart voxctrl-server`.

  ```toml
  [cleanup]
  enabled = true
  ```

On that start the server downloads the S1-mini model (about 470 MB) next to its
speech models, loads it, and logs `Cleanup:  on (s1-mini)` with the other
startup lines. Check it from the server:

```bash
curl -H "Authorization: Bearer $KEY" http://localhost:8787/v1/cleanup
# {"available":true,"model":"s1-mini"}

curl -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
     -d '{"text": "um so can you send me the uh the report by friday", "styling": "semi-formal"}' \
     http://localhost:8787/v1/cleanup
# something like: {"text":"Can you send me the report by Friday?"}
```

**On each client**, in **Settings → Engine → Remote Speech Engine Settings**,
tick **Run dictation cleanup on the server**. The line under it says whether
the server at that address offers cleanup, and what to change on the server if
it does not.

What stays on the client: whether a dictation is cleaned at all (**Settings →
Features → Enable S1-mini dictation cleanup**, and the per-hotkey override), the
cleanup style, and the per-app styles. The client sends the style with each
request, so several clients can share one server with different settings. With
the option ticked, the client does not download or load S1-mini, and
[Clean up models](#free-the-disk-space-your-local-models-used) will offer to
remove a copy it already has.

Things to know:

- **If the server can't clean a dictation, it is typed as it was said.** That
  covers the server being unreachable (including while the fallback engine is
  transcribing), cleanup being turned off on the server, and a cleanup that
  takes longer than the client's **Timeout** setting (held between 5 and 30
  seconds for cleanup). The client does not fall back to cleaning it locally.
  The reason is in the client's log.
- **Speed depends on the cleanup helper.** With `voxctrl-llm-sidecar` installed
  beside the server (the Docker image includes it) cleanup runs on llama.cpp.
  Without it the server says so at startup and uses a much slower built-in CPU
  engine. For GPU cleanup the helper needs the `vulkan` build; see
  [GPU acceleration](#gpu-acceleration).
- **It adds memory use on the server**: roughly the size of the model, on top
  of the speech model.
- **Only `voxctrl-server` offers this.** Other OpenAI-compatible speech servers
  have no `/v1/cleanup`; with one of those, leave the option off and cleanup
  keeps running on the client.
- The server logs how long each cleanup took and how many characters went in
  and out, never the text.

### Text-to-speech on the server

The server can do the speaking for its clients: it synthesizes the speech and
the client plays the audio, so a client loads no TTS model. You decide on the
server which engines exist there. Each client then picks an engine and a voice
in its own Settings, including voices of its own that it uploads. It needs
VoxCtrl 0.9.11 or newer at both ends for that choice (0.9.7 to 0.9.10 speak
with one engine and voice fixed on the server), and has to be turned on at
both.

**On the server**, say which engines to offer, turn it on, and restart.

- Docker: in `packaging/docker/.env`, then `docker compose up -d --build`
  (an image built before 0.9.11 has to be rebuilt):

  ```bash
  VOXCTRL_SERVER_TTS=true
  # Every engine clients may choose between; the first is the default.
  VOXCTRL_SERVER_TTS_ENGINES=kitten_tts_2,piper
  ```

- System service: in `server.toml`, then `sudo systemctl restart voxctrl-server`:

  ```toml
  [tts]
  enabled = true
  engine = "kitten_tts_2"      # the default

  [speech]
  engines = ["piper"]          # offered as well
  ```

Each engine listed is downloaded when the server next starts. Only the default
is loaded then; the server keeps one engine's model in memory and swaps when a
request asks for another, so the first request after a client switches engine
is slower. **Nothing a client sends makes the server download anything:**
clients choose among what is listed here, and among voices that are already on
the server or that they upload themselves.

The `[tts]` section takes the same keys as the `tts` section of the app's own
config file, so anything you can set for an engine in the app you can set here;
see [Text-to-speech configuration](./configuration.md#tts-section). The keys
that only concern playing audio (`stop_key`, `response_overlay`, `speed`) do
nothing on a server.

| `engine` | What the server downloads on the next start | Voice setting |
|---|---|---|
| `piper` | The Piper program and the chosen voice (20 to 120 MB) | `voice` |
| `espeak` | Nothing. Needs `espeak-ng` installed; the Docker image includes it | none |
| `pocket_tts` | The audio.cpp runtime and the Pocket-TTS model | `[tts.pocket_tts] voice` |
| `kitten_tts_2` | The audio.cpp runtime and the Kitten TTS 2 model | `[tts.kitten_tts_2] voice` |
| `breeze_tts_2` | The audio.cpp runtime and the Breeze-TTS-2 model | set in `[tts.breeze_tts_2]` |
| `vox_cpm_2` | The audio.cpp runtime and the VoxCPM2 model | set in `[tts.vox_cpm_2]` |

Inflect-Micro is not available on the server yet. The voice settings above are
only each engine's **default**, used when a client picks none;
`VOXCTRL_SERVER_TTS_VOICE` sets it for the default engine. For Piper, clients
can pick among the Piper voices already downloaded on the server, which is the
default one unless you have put more in the server's `piper-voices` folder.

> **The neural engines need a recent system.** Pocket-TTS, Kitten TTS 2,
> Breeze-TTS-2 and VoxCPM2 run on the audio.cpp runtime, whose v0.9.4 build
> needs glibc 2.43 or newer (Ubuntu 26.04, or a rolling distribution such as Arch; **not**
> Debian 13 or Ubuntu 24.04). On an older system the server fails at startup
> with `GLIBC_2.43 not found`. The Docker image is built on Ubuntu 26.04 for
> this reason. Piper and eSpeak have no such requirement.

At startup the server logs `Speech:   on (kitten_tts_2, piper), clients may
upload voices`. Check it from the server:

```bash
curl -H "Authorization: Bearer $KEY" http://localhost:8787/v1/audio/speech
# {"available":true,"engine":"kitten_tts_2","voice":"Bruno","custom_voices":true,
#  "engines":[{"id":"kitten_tts_2","name":"Kitten TTS 2","voices":["Bella",…],
#              "default_voice":"Bruno","custom_voices":true}, …]}

curl -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
     -d '{"input": "Hello from the server."}' \
     http://localhost:8787/v1/audio/speech -o hello.wav
```

**On each client**, in **Settings → TTS**, set **Engine** to **VoxCtrl server**.
The panel under it then offers:

- **Engine**: the engines this server has set up, its default first.
- **Voice**: the server's default for that engine, the engine's own voices, and
  (for Pocket-TTS, Kitten TTS 2, Breeze-TTS-2 and VoxCPM2) **your own voices**.

The server used is the one under **Settings → Engine → Remote Speech Engine
Settings**; its address and key are used even if the client transcribes with a
local engine. If the server does not speak, the panel says what to change on it.

#### Your own voices on the server

Your voices are the clips in the client's cloned voices folder
(`~/.local/share/voxctrl/cloned-tts-voices/`): `<name>.wav`, with `<name>.txt`
beside it holding the words spoken in the clip, which Kitten TTS 2 requires.
They are the same clips the client's own engines use, so a voice you already
cloned locally is there to pick.

- **Uploading is automatic.** Picking one of your voices sends your clips to
  the server, and they are checked again before speech that uses one, so a
  clip you add or change is sent without you doing anything. **Send my voices
  now** does it on demand.
- **They are private to the client.** Each client's clips are stored in a
  folder of their own on the server
  (`<data>/voxctrl/client-voices/<client id>/`). Another client does not see
  them in its voice list and cannot speak with them. The client id is a hash of
  the computer's machine id and the user name; neither is sent.
- **Deleting a clip on the client removes it from the server** the next time
  the voices are synced: when the TTS tab checks the server, when you pick a
  voice, or before the next speech with one of your voices.
- A clip's name may use letters, digits, spaces, `-` and `_`, up to 64
  characters. Clips with other names stay local. A client can keep up to 50
  voices on a server, each within the server's upload limit (`max_upload_mb`).

This separation keeps clients' voices apart; it is not a defence against
someone who holds the server's API key and sets out to learn another client's
id. Everyone with the key is trusted with the server.

Things to know:

- **Each client chooses for itself.** The engine and voice travel with each
  request, so two clients of one server can use different ones. Switching
  between engines makes the server reload a model.
- **Speed, the stop key and what gets spoken stay on the client.** The client
  applies its pronunciation snippets and custom vocabulary to the text before
  sending it, plays the audio at its own **Speed**, and its stop key stops
  playback.
- **Speech arrives all at once.** The server synthesizes the whole passage
  before the client starts playing, so a long passage has a longer pause before
  it than a local engine that plays as it goes. At most 5,000 characters are
  spoken per request.
- **If the server can't be reached, nothing is spoken** and the client shows
  the reason. There is no local fallback for speech.
- **Memory on the server:** the model stays loaded for as long as the server
  runs.
- The server logs how long each request took and its size, never the text.

### Free the disk space your local models used

If this computer ran everything itself before you set up the server, its speech,
cleanup and voice models are still on disk, often several gigabytes of them,
and with a server doing the work most will never be loaded again.

**Settings → Engine → Downloaded Models → Clean up models…** lists them and
removes the ones you confirm:

1. **Pick the fallback engine.** The dialog asks first, because the fallback's
   model is the one speech model worth keeping on a client. Whatever you pick
   is saved as your fallback and its model is kept. Pick **None** and no speech
   model is kept: dictation then fails with an error whenever the server can't
   be reached, and the dialog says so.
2. **Review the two lists.** *Not in use* is what can go, each with its size
   and a checkbox, all ticked to start with. *Kept* is what stays, with the
   reason: the fallback engine, dictation cleanup being on, or the selected
   voice.
3. **Remove.** The button shows how many models go and how much space comes
   back.

Custom voices are never removed. Neither are the Piper and audio.cpp programs,
or files in a model folder of your own choosing that VoxCtrl did not download.
The full rules are in
[Cleaning up downloaded models](./speech-recognition.md#cleaning-up-downloaded-models).

A removed model is not gone for good: select it again in Settings and VoxCtrl
offers to download it.

## Configuration reference

Settings come from four places. Later ones win:

1. built-in defaults;
2. the config file, `server.toml`;
3. `VOXCTRL_SERVER_*` environment variables;
4. command-line flags.

Run `voxctrl-server --print-config` to see the result of combining them (the
API key is hidden).

### `server.toml`

Location: `~/.config/voxctrl/server.toml` for the user running the server, or
the path given with `--config` or `VOXCTRL_SERVER_CONFIG`. The server creates
it on first start if it has to save a generated API key. It is readable only by
its owner, because it holds the key.

Every key, with its default:

```toml
# Address and port to listen on. 0.0.0.0 = every network interface;
# 127.0.0.1 = this machine only.
bind = "0.0.0.0:8787"

# The key clients send. Left empty, one is generated on first start and
# written back here.
api_key = ""

# false accepts requests with no key. Only on a network you fully trust.
require_api_key = true

# Largest upload accepted, in megabytes. Dictation audio is about 2 MB a minute.
max_upload_mb = 256

[engine]
# "whisper-cpp", "moonshine" or "parakeet"
backend = "whisper-cpp"

[engine.whisper_cpp]
model_size = "tiny"      # or a path to a .bin model file
model_dir = ""           # where models are kept; empty = the default below
device = "auto"          # "auto" uses the GPU if this build has one; "cpu" never does
threads = 0              # 0 = one per physical core
language = "auto"        # a code such as "en", or "auto" to detect

[engine.moonshine]
model_size = "base"
language = "en"
device = "cpu"

[engine.parakeet]
model_size = "tdt-0.6b-v3"
language = "auto"
device = "cpu"

[cleanup]
# true = clean up dictation for clients that ask. Downloads the S1-mini model
# (about 470 MB) on the next start and keeps it loaded.
enabled = false
# Run the cleanup model on the GPU when the cleanup helper was built with GPU
# support (the `vulkan` build). No effect otherwise.
gpu = true

[tts]
# true = speak for clients whose TTS engine is "VoxCtrl server".
enabled = false
# "piper", "espeak", "pocket_tts", "breeze_tts_2", "vox_cpm_2" or "kitten_tts_2"
engine = "espeak"
# The Piper voice. Other engines keep their voice in their own sub-section,
# with the same keys as the app's config: [tts.pocket_tts], [tts.kitten_tts_2],
# [tts.breeze_tts_2], [tts.vox_cpm_2].
voice = "en-us-lessac-medium"

[speech]
# Engines offered to clients besides tts.engine, which is always offered and
# is the default. Each is downloaded at startup.
engines = []
```

A `server.toml` the server writes itself leaves `[tts]` out until you add it.

Only the `[engine.*]` section for the selected `backend` is used. You can leave out any key,
and any whole section, that you don't want to change.

Models are stored in `~/.local/share/voxctrl/models/` for the user running the
server. Changes to `server.toml` take effect when the server is restarted.

### Environment variables

| Variable | Sets | Example |
|---|---|---|
| `VOXCTRL_SERVER_CONFIG` | Path of the config file | `/etc/voxctrl/server.toml` |
| `VOXCTRL_SERVER_BIND` | `bind` | `0.0.0.0:8787` |
| `VOXCTRL_SERVER_API_KEY` | `api_key` | `vx_my-own-long-secret` |
| `VOXCTRL_SERVER_REQUIRE_API_KEY` | `require_api_key` | `false` |
| `VOXCTRL_SERVER_MAX_UPLOAD_MB` | `max_upload_mb` | `512` |
| `VOXCTRL_SERVER_BACKEND` | `engine.backend` | `parakeet` |
| `VOXCTRL_SERVER_MODEL` | `model_size` of the selected backend | `tdt-0.6b-v3` |
| `VOXCTRL_SERVER_DEVICE` | `device` of the selected backend | `cpu` |
| `VOXCTRL_SERVER_LANGUAGE` | `language` of the selected backend | `en` |
| `VOXCTRL_SERVER_CLEANUP` | `cleanup.enabled` | `true` |
| `VOXCTRL_SERVER_CLEANUP_GPU` | `cleanup.gpu` | `false` |
| `VOXCTRL_SERVER_TTS` | `tts.enabled` | `true` |
| `VOXCTRL_SERVER_TTS_ENGINE` | `tts.engine` | `piper` |
| `VOXCTRL_SERVER_TTS_ENGINES` | `speech.engines`: every engine clients may choose between, comma-separated. The first is the default unless `VOXCTRL_SERVER_TTS_ENGINE` is set | `kitten_tts_2,piper` |
| `VOXCTRL_SERVER_TTS_VOICE` | The default voice of the default engine (`piper`, `pocket_tts`, `kitten_tts_2`) | `en-us-lessac-medium` |

A variable that is set but empty is ignored. Environment variables are never
written into `server.toml`.

### Command-line flags

| Flag | Meaning |
|---|---|
| `-c`, `--config <PATH>` | Config file to use |
| `-b`, `--bind <ADDR>` | Address and port to listen on |
| `--print-config` | Print the effective settings and exit |
| `--healthcheck` | Ask the running server whether it is up; exit code 0 if so |
| `-h`, `--help` | Usage |
| `-V`, `--version` | Version |

Set `RUST_LOG=debug` for more detailed logs.

---

## Security

What you send to the server is a recording of your microphone, so treat the
port accordingly.

- **Keep the API key on.** Every request must carry it. Without it, anyone who
  can reach the port can use your server.
- **The connection is not encrypted.** In this release the server speaks plain
  HTTP, so audio and the API key cross the network in the clear. On your own
  LAN that is usually acceptable. Anywhere else it is not.
- **Do not forward the port from the internet.**
- **For access from outside your network**, use a VPN such as WireGuard or
  Tailscale and point the client at the server's VPN address, or put the server
  behind a reverse proxy that adds HTTPS. With [Caddy](https://caddyserver.com/),
  for example:

  ```
  voxctrl.example.com {
      reverse_proxy 127.0.0.1:8787
  }
  ```

  Set `bind = "127.0.0.1:8787"` so the server itself is reachable only through
  the proxy, and use `https://voxctrl.example.com/v1` as the client's Endpoint
  URL.
- **To change the key**, put a new value in `api_key` (or
  `VOXCTRL_SERVER_API_KEY`) and restart; or delete the `api_key` line and
  restart to have a new one generated and printed. Then update each client.
- **The server does not keep your audio or your text.** Audio is held in memory
  for the length of one request. The log records how long each clip was and how
  long it took, never what was said.

---

## GPU acceleration

**You may not need it.** Parakeet and Moonshine are fast on a CPU, and in
VoxCtrl's own measurements on an RTX 4090 they ran 5–7× *slower* with the GPU
provider than on the CPU (see [GPU / CPU per
engine](./speech-recognition.md#gpu--cpu-per-engine)). The engine that does
benefit from a GPU is whisper.cpp with `medium` and larger models.

The default build runs everything on the CPU. A `vulkan` build offloads
whisper.cpp to any Vulkan-capable GPU (NVIDIA, AMD or Intel), the same way the
desktop app's Linux release does.

**Native build** — needs the Vulkan SDK at build time (the project's CI
installs LunarG's `vulkan-sdk` package) and the GPU's Vulkan driver at run
time:

```bash
cargo build --release -p voxctrl-server --features vulkan
cargo build --release -p voxctrl-llm-sidecar --features vulkan   # GPU dictation cleanup
```

**Docker — experimental.** The files are provided but have not yet been tested
on real hardware, so expect to adjust them:

```bash
# in packaging/docker/.env
VOXCTRL_BUILD_FEATURES=vulkan
VOXCTRL_SERVER_BACKEND=whisper-cpp
VOXCTRL_SERVER_MODEL=large-v3-turbo

docker compose -f docker-compose.yml -f docker-compose.gpu.yml up -d --build
```

This needs the [NVIDIA Container
Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html)
on the host. The server log says which device whisper.cpp ended up on
(`Whisper acceleration: …`); if it reports none, the container could not see a
Vulkan device and is running on the CPU.

---

## Day-to-day operation

| Task | Docker | systemd |
|---|---|---|
| See the logs | `docker compose logs -f` | `sudo journalctl -u voxctrl-server -f` |
| Restart | `docker compose restart` | `sudo systemctl restart voxctrl-server` |
| Stop | `docker compose down` | `sudo systemctl stop voxctrl-server` |
| Is it healthy? | `docker compose ps` | `curl http://localhost:8787/health` |
| Change the model | Edit `.env`, then `docker compose up -d` | Edit `server.toml`, then restart |
| Upgrade | `git pull`, then `docker compose up -d --build` | `git pull`, rebuild, reinstall the binary, restart |

Changing the model downloads the new one on the next start. Models you no
longer use stay on disk until you delete them from the models directory.

The server handles one transcription at a time. Requests that arrive while one
is running wait their turn, so several clients can share a server; they just
don't run in parallel.

### Live text and server load

A client with live text on sends the server a short clip a few times a second
while its user is speaking, on top of the one request per dictation. Each is
small and the client waits for one to finish before sending the next, so the
load follows how fast the server answers. With a fast model it is unnoticeable.
With a slow one (a large Whisper model on a CPU) live text lags, and a live
clip that is still being transcribed when the user stops can delay the final
transcript by up to that clip's time.

To stop it, turn off **Settings → Visual & Feedback → Show live transcript in
overlay** on the client. The server log shows these as ordinary
`Transcribed 1.4s of audio in … ms` lines.

---

## API reference

The server implements the part of the OpenAI API that speech-to-text clients
use, so tools other than VoxCtrl can use it too.

| Method and path | Key needed | Purpose |
|---|---|---|
| `GET /health` | No | Liveness check |
| `GET /v1/models` | Yes | The model the server is running |
| `POST /v1/audio/transcriptions` | Yes | Transcribe audio |
| `GET /v1/cleanup` | Yes | Whether cleanup is enabled: `{"available": true, "model": "s1-mini"}` |
| `POST /v1/cleanup` | Yes | Clean up a transcript |
| `GET /v1/audio/speech` | Yes | Whether text-to-speech is enabled: `{"available": true, "engine": "piper", "voice": "…"}` |
| `POST /v1/audio/speech` | Yes | Speak text; returns a WAV file |
| `GET /v1/voices` | Yes, plus client id | The voices this client has uploaded |
| `PUT /v1/voices/{name}` | Yes, plus client id | Upload or replace a voice (multipart: `audio`, optional `transcript`, `fingerprint`) |
| `DELETE /v1/voices/{name}` | Yes, plus client id | Remove a voice |

The voice endpoints, and speech with an uploaded voice, need an
`X-VoxCtrl-Client` header of 16 to 64 hex digits naming the client.

`GET /health` also reports `"cleanup"` and `"tts"` as `true` or `false`. `/v1/cleanup` is
VoxCtrl's own and not part of the OpenAI API.

Send the key as `Authorization: Bearer <key>`.

### `POST /v1/audio/transcriptions`

A `multipart/form-data` request:

| Field | Required | Meaning |
|---|---|---|
| `file` | Yes | The audio, as a WAV file |
| `language` | No | Language code; overrides the server's setting for this request (whisper.cpp only) |
| `prompt` | No | Words to bias recognition toward, such as names and jargon |
| `response_format` | No | `json` (default), `text` or `verbose_json` |
| `model` | No | Accepted and ignored |

**Audio format.** WAV only: PCM (8, 16, 24 or 32-bit) or IEEE float, any sample
rate, any number of channels. It is converted to 16 kHz mono. 16 kHz mono
16-bit PCM is what VoxCtrl sends and needs no conversion. Compressed formats
(MP3, Ogg, FLAC, M4A) are rejected with an explanation.

```bash
curl http://192.168.1.50:8787/v1/audio/transcriptions \
  -H "Authorization: Bearer $VOXCTRL_KEY" \
  -F file=@recording.wav
```

```json
{"text":"Hello from the server.","language":"en","duration":2.4}
```

Errors use OpenAI's envelope, with a message written to be shown to a person:

```json
{"error":{"message":"The API key was not accepted. Check it against the api_key in the server's server.toml.","type":"authentication_error"}}
```

| Status | Meaning |
|---|---|
| 400 | No `file`, not a WAV file, or an unsupported `response_format` |
| 401 | Missing or wrong API key |
| 413 | Upload larger than `max_upload_mb` |
| 500 | The model failed; details are in the server log |

---

### `POST /v1/cleanup`

JSON in, JSON out.

| Field | Required | Meaning |
|---|---|---|
| `text` | Yes | The transcript to clean, at most 20,000 characters |
| `styling` | No | `semi-formal` (default), `casual`, `formal`, `verbatim` or `concise` |

```bash
curl -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
     -d '{"text": "um so lets meet at three tomorrow", "styling": "casual"}' \
     http://server:8787/v1/cleanup
# something like: {"text":"Let's meet at 3 tomorrow."}
```

Errors use the same envelope as the rest of the API. A server with cleanup
turned off answers HTTP 501 with `"type": "not_enabled"`. Empty text returns
empty text without running the model.

### `POST /v1/audio/speech`

The same path and request shape as OpenAI's speech endpoint, so OpenAI clients
can use it. JSON in, `audio/wav` out.

| Field | Required | Meaning |
|---|---|---|
| `input` | Yes | The text to speak, at most 5,000 characters |
| `response_format` | No | `wav` or left out. Anything else is refused: the server only makes WAV |
| `model` | No | One of this server's engine ids, e.g. `kitten_tts_2`. Anything else, such as an OpenAI client's `tts-1`, means the default engine |
| `voice` | No | One of that engine's voices, or `custom:<name>` for a voice this client uploaded. Left out, or a name the server does not know sent without one of its engine ids, means the engine's default |
| `speed` | No | Accepted and ignored: the client applies its own speed when it plays the audio |

```bash
curl -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
     -d '{"input": "The build finished."}' \
     http://server:8787/v1/audio/speech -o speech.wav
```

Asking one of the server's engines for a voice it does not have, or for an
uploaded voice this client has not sent, is HTTP 400. A server with
text-to-speech turned off answers HTTP 501 with `"type": "not_enabled"`. An OpenAI client that asks for its default format
(`mp3`) gets HTTP 400; set its format to `wav`.

---

## Troubleshooting

### A neural TTS engine will not start on the server: `GLIBC_2.43 not found`

Pocket-TTS, Kitten TTS 2, Breeze-TTS-2 and VoxCPM2 run on a runtime that needs
glibc 2.43 or newer. A Docker image built before 0.9.11 was based on Debian 13,
which is too old: rebuild it (`docker compose up -d --build`) to get the Ubuntu
26.04 base. For a system service on an older distribution, use Piper or eSpeak,
or run the server in Docker.

### Nothing is spoken with the "VoxCtrl server" engine

The client shows the reason when speech fails. In **Settings → TTS** on the
client, with **Engine** set to **VoxCtrl server**, press **Check the server**:

- *Text-to-speech is turned off on this server*: set `VOXCTRL_SERVER_TTS=true`
  or `[tts] enabled = true` on the server and restart it.
- *This server does not offer text-to-speech*: the server is older than 0.9.7
  (rebuild it), or the address is not a `voxctrl-server`.
- *No server address is set*: enter it under **Engine → Remote Speech Engine
  Settings**. It is used for speech even with a local speech engine selected.

If the server will not start with text-to-speech on, its log says what could
not be downloaded or loaded. The engines' programs and models are fetched from
GitHub and Hugging Face on the first start, so the server needs internet access
then.

### Dictation arrives uncleaned although cleanup is on

The client types the raw transcript whenever the server can't clean it. In
**Settings → Engine → Remote Speech Engine Settings** on the client, look at
the line under **Run dictation cleanup on the server**:

- *Cleanup is turned off on this server*: set `VOXCTRL_SERVER_CLEANUP=true` or
  `[cleanup] enabled = true` on the server and restart it.
- *This server does not offer dictation cleanup*: the server is older than
  0.9.5 (rebuild it), or the endpoint is not a `voxctrl-server`.
- *This server cleans up dictation*, and text still arrives uncleaned: cleanup
  is probably timing out. Check the server log for how long cleanups take. If
  the server logged that `voxctrl-llm-sidecar` was not found at startup,
  install it beside the server binary; it is far faster than the built-in
  engine.

Also check that cleanup is on for that dictation at all: **Settings → Features →
Enable S1-mini dictation cleanup**, or the hotkey's own setting.

### The tray icon has an amber badge

The app can't reach the server. Hover the icon for the reason, or open
**Settings → Engine**. "Could not connect to the server" is the network or the
server being down: work through the next entry. "The server rejected the API
key" is the entry after it. Dictation keeps working in the meantime if a
[fallback engine](#when-the-server-cant-be-reached) is set.

### Test Connection says "Connection failed"

The client could not reach the server at all.

1. On the server, confirm it is running and ready: `curl http://localhost:8787/health`.
   If that fails, look at the logs; the model may still be downloading.
2. From the client, try the server's address: `curl http://192.168.1.50:8787/health`.
   If this fails while step 1 works, it is the network: check the firewall
   ([step 7](#7-open-the-port)), that `bind` is not `127.0.0.1`, and with Docker
   that the port is published (`docker compose ps` shows `0.0.0.0:8787->8787`).
3. Check the Endpoint URL in the app starts with `http://` (not `https://`) and
   ends with `/v1`.

### "HTTP 401" or "The API key was not accepted"

The key in the app does not match the server's. Read the server's key back from
`server.toml` (commands are in the install sections above) and paste it again,
without spaces or quotes. If you set `VOXCTRL_SERVER_API_KEY`, that value wins
over the file.

### The container stays `starting`, or the server takes a long time to come up

The server does not listen until its model is downloaded and loaded. Follow the
log: it prints `Downloading the … model (first start only)` and then
`Loading …`. A large model on a slow connection can take many minutes. The
container is given 15 minutes before a failed health check counts.

### "Could not download the … model"

The server could not reach `huggingface.co`. Check the machine's internet
access and DNS, and any proxy. On a machine that can never reach it, copy the
model files into the models directory by hand from another machine where
VoxCtrl has already downloaded them (`~/.local/share/voxctrl/models/`).

### "engine.backend is 'parakeet', but this server was built without it"

The server was built with `--no-default-features`. Rebuild without that flag,
or choose `whisper-cpp`.

### "Could not listen on 0.0.0.0:8787"

Another program is using the port, or the server is already running. Choose a
different port with `bind` (and publish the same one in Docker), or stop the
other program. `ss -ltnp | grep 8787` shows what holds it.

### Dictation is slow

- Check the timings in the server log: `Transcribed 6.2s of audio in 480 ms`.
  If the server's time is short, the delay is the network or the client.
- If the server's time is long, the model is too large for the hardware. Try
  `parakeet` / `tdt-0.6b-v3`, or a smaller Whisper model.
- With a large Whisper model on a CPU, raise **Timeout** in the app so long
  dictations are not cut off.

### "The upload is larger than this server accepts"

A very long recording exceeded `max_upload_mb`. Raise it and restart.

### Permission errors with a bind-mounted directory in Docker

The server runs as user ID 1000 inside the container. If you replace the named
volume with a host directory, make it writable by that ID:
`sudo chown -R 1000:1000 /path/to/dir`.

### Reporting a problem

Open an issue at <https://github.com/JRufer/VoxCtrl/issues> with the server's
version (`voxctrl-server --version`), how it is installed (Docker or systemd),
the output of `voxctrl-server --print-config`, and the relevant part of the
log.
