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

There are three ways to run it:

| | Best for |
|---|---|
| [From the VoxCtrl AppImage](#run-it-from-the-voxctrl-appimage) (`--server`) | A desktop or laptop that has, or can have, the VoxCtrl app. Nothing to build |
| [Docker](#install-with-docker) | A home server or NAS that already runs containers |
| [A system service](#install-as-a-system-service) | A headless machine without Docker. On Arch Linux there is [a package](#arch-linux-aur) for it |

**Contents**

1. [What runs where](#what-runs-where)
2. [Before you start](#before-you-start)
3. [Choose a model](#choose-a-model)
4. [Run it from the VoxCtrl AppImage](#run-it-from-the-voxctrl-appimage)
5. [Install with Docker](#install-with-docker)
6. [Install as a system service](#install-as-a-system-service)
7. [Connect the VoxCtrl app](#connect-the-voxctrl-app)
8. [Configuration reference](#configuration-reference)
9. [Security](#security)
10. [GPU acceleration](#gpu-acceleration)
11. [Day-to-day operation](#day-to-day-operation)
12. [API reference](#api-reference)
13. [Troubleshooting](#troubleshooting)

---

## What runs where

The server runs speech recognition, S1-mini dictation cleanup (from 0.9.5) and
text-to-speech (from 0.9.7), and can pass AI processing on to a language model
(from 0.9.16).

| | Where it runs |
|---|---|
| Hotkeys, microphone, overlay, typing the text | Client (the VoxCtrl app) |
| **Speech recognition** (whisper.cpp, Moonshine, Parakeet) | **Server** |
| Filler removal, spoken punctuation, snippets, custom vocabulary | Client |
| **S1-mini dictation cleanup** | **Server**, when turned on at both ends (see [Dictation cleanup on the server](#dictation-cleanup-on-the-server)); otherwise client |
| **AI processing** (the OpenAI-compatible LLM step) | **Through the server** to the language model service it is pointed at, when turned on at both ends (see [AI processing through the server](#ai-processing-through-the-server)); otherwise wherever the client's own OpenAI API settings point |
| **Text-to-speech** | **Server**, when turned on there and the client's TTS engine is "VoxCtrl server" (see [Text-to-speech on the server](#text-to-speech-on-the-server)); otherwise client. The audio is always played on the client |

Cleanup, text-to-speech and AI processing are each off until you turn them on
at both ends, so a client keeps using its own until then.

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
- The connection is plain HTTP unless you turn [encryption](#encryption) on:
  by default the audio is **not encrypted** in transit. That is fine on a home
  or office network you trust. Do not expose the port to the internet; for
  access from outside, see [Security](#security).

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

## Run it from the VoxCtrl AppImage

The VoxCtrl AppImage is also the server. Started with `--server` it runs the
same server as `voxctrl-server`, with no window, tray icon or microphone, so
one download covers both ends and there is nothing to build. (From 0.9.17.)

```bash
chmod +x VoxCtrl-linux-x86_64-vulkan.AppImage
./VoxCtrl-linux-x86_64-vulkan.AppImage --server
```

The first start downloads the model, then prints the address, the API key and
a pairing code:

```text
voxctrl-server 0.9.19 is ready
  Endpoint: https://0.0.0.0:8787/v1  (use this machine's address from a client)
  Encryption: on. Certificate fingerprint 3F:9A:…
  API key:  vx_…
  Pairing code (paste it into the app instead of the address and key): vxpair_…
```

Paste the pairing code into the app on your other computer
([Connect the VoxCtrl app](#connect-the-voxctrl-app)). Stop the server with
Ctrl+C. To see the code again later:

```bash
./VoxCtrl-linux-x86_64-vulkan.AppImage --server --pairing-code
```

Everything after `--server` is a server option, the same ones `voxctrl-server`
takes ([Command-line flags](#command-line-flags)): `--server --help`,
`--server --print-config`, `--server --bind 0.0.0.0:9000`. The `.deb` package
works the same way: `voxctrl --server`.

**Settings** go in `~/.config/voxctrl/server.toml`, which the first start
creates, or in environment variables
([Configuration reference](#configuration-reference)):

```bash
VOXCTRL_SERVER_BACKEND=whisper-cpp VOXCTRL_SERVER_MODEL=large-v3-turbo \
  ./VoxCtrl-linux-x86_64-vulkan.AppImage --server
```

**What it shares with the app on the same machine.** The server keeps its own
settings file (`server.toml`, not the app's `config.json`) but uses the same
model folder, so a model the app already downloaded is not downloaded again.
The GPU build of the AppImage gives the server the same GPU acceleration as the
app. You can run the app and the server on one machine at once; each loads its
own copy of its model, so that takes the memory of both.

**What the machine needs.** The AppImage is built for desktop Linux and loads
the same system libraries in server mode as it does for the app (GTK, GLib,
GStreamer, Wayland and OpenGL client libraries), even though it opens no
window. Any desktop install has them, and it runs fine over SSH with nobody
logged in. A minimal server install without a desktop usually lacks them, and
the AppImage then stops with `error while loading shared libraries`. On such a
machine use [Docker](#install-with-docker) or
[a system service](#install-as-a-system-service).

### Start it automatically

To have the server start with the machine, without anyone logging in, run it
as a systemd user service. Put the AppImage somewhere permanent first, for
example `~/Applications/`, then save this as
`~/.config/systemd/user/voxctrl-server.service`:

```ini
[Unit]
Description=VoxCtrl speech server
After=network-online.target

[Service]
ExecStart=%h/Applications/VoxCtrl-linux-x86_64-vulkan.AppImage --server
Restart=on-failure
RestartSec=5
# A first start downloads the model before the server is up.
TimeoutStartSec=infinity

[Install]
WantedBy=default.target
```

The same file is in the repository as
`packaging/systemd/voxctrl-server-appimage.service`. Then:

```bash
systemctl --user daemon-reload
systemctl --user enable --now voxctrl-server
sudo loginctl enable-linger "$USER"     # keep it running when you are logged out
```

| Task | Command |
|---|---|
| See the logs (and the API key, first start) | `journalctl --user -u voxctrl-server -f` |
| Restart after changing `server.toml` | `systemctl --user restart voxctrl-server` |
| Stop and disable | `systemctl --user disable --now voxctrl-server` |

To upgrade, replace the AppImage file with the new release and restart the
service. The app's built-in updater updates the file it was started from, so an
AppImage used for both the app and the server is updated by the app; restart
the service afterwards.

---

## Install with Docker

This is the simplest way to run the server on a machine that already has
Docker. Each VoxCtrl release publishes the server as an image, so there is
nothing to clone and nothing to compile: one file and one command.

| Image | Contains |
|---|---|
| `ghcr.io/jrufer/voxctrl-server:latest` | The newest release. whisper.cpp, Moonshine and Parakeet on the CPU |
| `ghcr.io/jrufer/voxctrl-server:0.9.22` | One release, for staying on it |
| `ghcr.io/jrufer/voxctrl-server:latest-vulkan` | The same with whisper.cpp and dictation cleanup on a Vulkan GPU. See [GPU acceleration](#gpu-acceleration) |
| `ghcr.io/jrufer/voxctrl-server:edge` | The build of the next release, before it is published. For testing |

The images are for 64-bit x86 (`linux/amd64`). On anything else, [build the
image yourself](#build-the-image-yourself).

### 1. Prerequisites

- [Docker Engine](https://docs.docker.com/engine/install/) with the Compose
  plugin (`docker compose version` should print a version).
- Disk space for the image plus the size of your model.

### 2. Get the Compose file

```bash
mkdir voxctrl-server && cd voxctrl-server
curl -LO https://github.com/JRufer/VoxCtrl/releases/latest/download/docker-compose.yml
```

It is also on every [release page](https://github.com/JRufer/VoxCtrl/releases)
under Assets, and in the repository at `packaging/docker/docker-compose.yml`.

### 3. Choose your settings (optional)

With no settings the server runs Parakeet `tdt-0.6b-v3` on port 8787 and makes
its own API key. To change anything, download the list of settings, save it as
`.env` beside the Compose file, and edit it:

```bash
curl -L -o .env https://github.com/JRufer/VoxCtrl/releases/latest/download/voxctrl-server.env
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

### 4. Start

```bash
docker compose up -d
```

The first start downloads the image. Later ones start at once.

### 5. Watch the first start and collect the API key

```bash
docker compose logs -f
```

On the first start the server downloads the model, loads it, and then prints:

```
INFO voxctrl_server: voxctrl-server 0.9.19 is ready
INFO voxctrl_server:   Model:    parakeet/tdt-0.6b-v3
INFO voxctrl_server:   Endpoint: https://0.0.0.0:8787/v1  (use this machine's address from a client)
INFO voxctrl_server:   Encryption: on. Certificate fingerprint 3F:9A:…
INFO voxctrl_server:   API key:  vx_3f9a…
INFO voxctrl_server:   Pairing code (paste it into the app instead of the address and key): vxpair_…
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
curl -k https://localhost:8787/health
# {"contract":1,"min_client":"0.9.0","model":"parakeet/tdt-0.6b-v3","status":"ok","version":"0.9.19",…}
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

### Upgrade, or stay on one version

```bash
docker compose pull && docker compose up -d
```

That moves the server to the newest release. Your models and API key are in
the volume and are kept.

To stay on one release, name it in `.env`; change the number when you choose to
upgrade:

```ini
VOXCTRL_SERVER_VERSION=0.9.22
```

Keep the app and the server reasonably close in version. The app warns you
when the server's version does not suit it.

### Without Compose

```bash
docker run -d --name voxctrl-server --restart unless-stopped \
  -p 8787:8787 \
  -e VOXCTRL_SERVER_BACKEND=parakeet \
  -e VOXCTRL_SERVER_MODEL=tdt-0.6b-v3 \
  -v voxctrl-data:/var/lib/voxctrl \
  ghcr.io/jrufer/voxctrl-server:latest

docker logs -f voxctrl-server
```

### Build the image yourself

Only needed to work on the server, or to run it on a platform the published
image is not built for. It needs the whole repository, and compiles whisper.cpp,
llama.cpp and the server, which takes from several minutes to half an hour.

```bash
git clone https://github.com/JRufer/VoxCtrl.git
cd VoxCtrl/packaging/docker
cp .env.example .env        # optional
docker compose -f docker-compose.yml -f docker-compose.build.yml up -d --build
```

`docker-compose.build.yml` replaces the published image with one built from
your checkout, tagged `voxctrl-server:local`. Or, without Compose, from the
repository root:

```bash
docker build -f packaging/docker/Dockerfile -t voxctrl-server .
```

#### With Reverb-TTS

No audio.cpp release has the `reverb` family yet, so the published image cannot
run Reverb-TTS. A self-built one can: set `VOXCTRL_BUILD_AUDIOCPP_REF=reverb-tts`
in `.env` (or `--build-arg AUDIOCPP_REF=reverb-tts` with `docker build`). The
image then builds `audiocpp_server` from that branch of JRufer/audio.cpp, with
every model family, on the same Ubuntu base it runs on, and puts it in
`/usr/local/lib/voxctrl/`. The server uses it in place of the runtime it would
download, so `VOXCTRL_SERVER_AUDIOCPP_SERVER` stays blank.

The audio.cpp compile adds tens of minutes and needs memory:
`VOXCTRL_BUILD_AUDIOCPP_JOBS` (default 2) sets the parallel compiles, each up to
4–5 GB. With `VOXCTRL_BUILD_FEATURES=vulkan` it is a Vulkan build too, so
Reverb-TTS can use the GPU (`[tts.reverb] gpu = true`).

---

## Install as a system service

Use this to run the server directly on the machine, managed by systemd.

### Arch Linux (AUR)

On Arch and Arch-based systems the `voxctrl-server` package does all of the
steps below: it builds the server, installs the service, and creates the
`voxctrl` user and `/var/lib/voxctrl`. (From 0.9.18.)

```bash
paru -S voxctrl-server        # or: yay -S voxctrl-server
sudo systemctl enable --now voxctrl-server
sudo journalctl -u voxctrl-server -f      # first start: model download, then the API key
sudo -u voxctrl -H voxctrl-server --pairing-code
```

It is built from source on your machine, which takes a while (it compiles
whisper.cpp and llama.cpp) and needs network access during the build. It has
no desktop dependencies, and can be installed next to the `voxctrl` app
package. whisper.cpp and dictation cleanup use a GPU when a Vulkan driver is
installed (`vulkan-driver`), and the CPU otherwise.

Then continue at [5. Choose a model](#5-choose-a-model); the settings file is
`/var/lib/voxctrl/.config/voxctrl/server.toml`. After an upgrade, restart the
service: `sudo systemctl restart voxctrl-server`.

On other distributions, follow the steps below.

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
with `curl -k https://localhost:8787/health`, then stop it with `Ctrl-C`.

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

Do this on each computer that should use the server.

### With a pairing code

The quickest way, and the only way when the server has
[encryption](#encryption) on.

1. On the server, print its pairing code:

   ```bash
   docker exec voxctrl-server voxctrl-server --pairing-code     # Docker
   sudo -u voxctrl -H voxctrl-server --pairing-code             # system service
   ```

   The code starts with `vxpair_`. It holds the API key, so treat it like one.
2. In the app, open **Settings → Engine** and set **Backend** to **Remote
   Speech Engine**.
3. If the server runs in Docker, type the server machine's name or IP address
   into **Endpoint URL** first. A container cannot tell what address it is
   reached by, so its code leaves that out. Set `VOXCTRL_SERVER_PUBLIC_HOST`
   on the server to have the code carry the address.
4. Paste the code into **Pairing code** and click **Pair**. The endpoint, the
   API key and, with encryption on, the server's certificate are filled in, and
   the connection is tested.

Then continue with step 5 below.

### By hand

You need the server's address and the API key. This does not work for a server
with encryption on, which is every server set up from 0.9.19 on unless it was
turned off ([Encryption](#encryption)): use the pairing code for those.

1. Open **Settings → Engine** and set **Backend** to **Remote Speech
   Engine**. The server settings appear below it.
2. Enter the **Endpoint URL**. For a VoxCtrl server the machine's name or IP
   address is enough (`192.168.1.50`); it is completed to
   `http://192.168.1.50:8787/v1`.
3. Enter the **API Key** from the server's log or `server.toml`.
4. Click **Test Connection**. The line under it says whether the address and
   key are good and what the server offers: speech recognition, dictation
   cleanup, text-to-speech. The model name is filled in from the server.
5. Choose what else the server does, in the same section:

   | Setting | What it does |
   |---|---|
   | **Fallback engine** | The local engine that takes over when the server can't be reached ([fallback](#when-the-server-cant-be-reached)) |
   | **Run dictation cleanup on the server** | S1-mini cleanup runs on the server. Needs cleanup enabled there |
   | **Use the server for AI processing** | Hotkeys with AI processing on send their text to the server's language model instead of the service under **Settings → OpenAI API**. Needs AI processing enabled on the server ([AI processing through the server](#ai-processing-through-the-server)) |
   | **Use the server for text-to-speech** | The server speaks and this computer plays the audio. This sets the TTS engine to **VoxCtrl server**; pick the server's engine and voice under **Settings → TTS**. Switching it off goes back to the TTS engine in use before |

Settings save as you change them. Dictation goes to the server for as long as
**Remote Speech Engine** is the backend; choose another backend to go back to
a local engine.

Text-to-speech can use the server without dictation doing so: set **Settings →
TTS → Engine** to **VoxCtrl server**. It uses the address and key entered
here. The first-run wizard can also point a new install at a server, but it is
not needed for that.

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

- Docker: set `VOXCTRL_SERVER_CLEANUP=true` in `.env`, then
  `docker compose up -d`. The published image contains the cleanup helper.
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
Post-Processing → Enable S1-mini dictation cleanup**, and the per-hotkey override), the
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

- Docker: in `.env`, then `docker compose up -d`:

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
| `pocket_tts` | The audio.cpp runtime, the Pocket-TTS model and its five built-in voices | `[tts.pocket_tts] voice` |
| `kitten_tts_2` | The audio.cpp runtime and the Kitten TTS 2 model | `[tts.kitten_tts_2] voice` |
| `reverb` | The Reverb-TTS model. Needs an audio.cpp build with the `reverb` family, since no release has it yet: a self-built image with it ([With Reverb-TTS](#with-reverb-tts)), or `[tts] audiocpp_server` (`VOXCTRL_SERVER_AUDIOCPP_SERVER`) | `[tts.reverb] voice` |
| `breeze_tts_2` | The audio.cpp runtime and the Breeze-TTS-2 model | set in `[tts.breeze_tts_2]` |
| `vox_cpm_2` | The audio.cpp runtime and the VoxCPM2 model | set in `[tts.vox_cpm_2]` |

Every engine the server offers is downloaded at startup, before it starts
listening, so a client's first request never waits on a download. If one cannot
be made ready the server does not start, and says why. For `reverb` that
includes an audio.cpp runtime without the `reverb` family: the server stops
before downloading the model, rather than failing at the first request.

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
  (for Pocket-TTS, Kitten TTS 2, Reverb-TTS, Breeze-TTS-2 and VoxCPM2) **your own voices**.

The server used is the one under **Settings → Engine → Remote Speech Engine
Settings**; its address and key are used even if the client transcribes with a
local engine. If the server does not speak, the panel says what to change on it.

#### Your own voices on the server

Your voices are the clips in the client's cloned voices folder
(`~/.local/share/voxctrl/cloned-tts-voices/`): `<name>.wav`, with `<name>.txt`
beside it holding the words spoken in the clip, which Kitten TTS 2 and Reverb-TTS require.
They are the same clips the client's own engines use, so a voice you already
cloned locally is there to pick.

- **Uploading is automatic.** Picking one of your voices sends your clips to
  the server, and they are checked again before speech that uses one, so a
  clip you add or change is sent without you doing anything. **Send my voices
  now** does it on demand.
- **Each client sees only its own.** Each client's clips are stored in a
  folder of their own on the server
  (`<data>/voxctrl/client-voices/<client id>/`), and another VoxCtrl app does
  not see them in its voice list or speak with them. The client id is a hash
  of the computer's machine id and the user name; neither is sent. This keeps
  apps apart; it is not access control. The id is not a secret, and anyone
  with the server's API key who learns it can use those voices, so share the
  key only with people you would share the voices with.
- **Deleting a clip on the client removes it from the server** the next time
  the voices are synced: when the TTS tab checks the server, when you pick a
  voice, or before the next speech with one of your voices.
- A clip's name may use letters, digits, spaces, `-` and `_`, up to 64
  characters. Clips with other names stay local. A client can keep up to 50
  voices on a server, each within the server's upload limit (`max_upload_mb`).


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
- **Stopping speech stops the work.** When you start dictating again or press
  the stop key while the server is still synthesizing, the client gives the
  request up at once and closes the connection. The server skips any request
  whose client has gone before its turn came. A sentence already being
  synthesized can't be interrupted and runs to its end, so on a slow CPU engine
  one stopped sentence can still occupy the server for a while.
- **Speech yields to transcription.** The neural engines' helper process runs
  at a lower scheduling priority than the rest of the server, so a dictation
  that arrives while a sentence is being synthesized is transcribed first.
- **Memory on the server:** the model stays loaded for as long as the server
  runs.
- The server logs how long each request took, its size and which engine and
  voice were asked for, never the text.

### AI processing through the server

AI processing is the optional step where a hotkey sends what you dictated to a
language model to be rewritten (**Settings → OpenAI API**, switched on per
hotkey). Normally each computer connects to the model's service itself. With
this turned on, each computer sends the request to the VoxCtrl server, and the
server passes it to one language model service that you set up once.

Use it when:

- the service runs on the server machine (Ollama, llama.cpp) and you would
  rather not open its port to the network;
- the service is a hosted provider and you want its API key kept on the
  server, not on every computer;
- the server has [encryption](#encryption) on, so the text is encrypted on the
  way.

The model does not run inside `voxctrl-server`. You need an OpenAI-compatible
service the server can reach.

**1. On the server**, in `.env` (Docker):

```bash
VOXCTRL_SERVER_LLM=true
VOXCTRL_SERVER_LLM_ENDPOINT=http://host.docker.internal:11434
VOXCTRL_SERVER_LLM_MODEL=llama3.2:1b
# Only if the service needs a key:
VOXCTRL_SERVER_LLM_API_KEY=
```

or in `server.toml`:

```toml
[llm]
enabled = true
endpoint = "http://localhost:11434"
model = "llama3.2:1b"
```

Restart the server. Its log shows `AI processing: on (llama3.2:1b at …)`, and
warns if the service is not reachable or does not list that model. Neither
stops the server from starting.

In Docker, `localhost` is the container, not the machine. For a service on the
same machine use `http://host.docker.internal:11434`, and make that service
listen on more than loopback (for Ollama, `OLLAMA_HOST=0.0.0.0`).

**2. On each client**, turn on **Use the VoxCtrl server** under **Settings →
OpenAI API**, or **Use the server for AI processing** under **Settings →
Engine**. They are the same setting. **Check the server** reports the model the
server answers with, or what to change on the server.

What changes on the client and what does not:

| | |
|---|---|
| Prompts (system and user, per hotkey or global) | Still the client's |
| Which hotkeys use AI processing | Still the client's |
| Model | The server's. The client's model, and a hotkey's model override, are not used |
| API URL and key under OpenAI API | Not used while this is on; kept for when you turn it off |
| Timeout | The client's timeout still applies to its request; `llm.timeout_secs` on the server (60) limits the model's answer |

It works with any speech engine selected on the client: only the server's
address and key under **Engine** are needed, not the Remote Speech Engine
backend.

If the request fails for any reason (server unreachable, AI processing off
there, the model's service down), the text is typed as it was dictated. The
client's log and the server's log say why. Chat targets under **Output
Commands** are separate and still connect to their own service.

The server logs how many characters each request had and how long it took,
never the text.

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

# The name or IP address clients reach this machine by, for the pairing code.
# Left empty, the server works it out; in a container it cannot, and the
# client types the address instead.
public_host = ""

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
# "piper", "espeak", "pocket_tts", "breeze_tts_2", "vox_cpm_2", "kitten_tts_2"
# or "reverb"
engine = "espeak"
# The Piper voice. Other engines keep their voice in their own sub-section,
# with the same keys as the app's config: [tts.pocket_tts], [tts.kitten_tts_2],
# [tts.reverb], [tts.breeze_tts_2], [tts.vox_cpm_2].
voice = "en-us-lessac-medium"
# An audiocpp_server of your own for the audio.cpp engines; Reverb-TTS needs
# one with the reverb family. Empty = the managed runtime.
# audiocpp_server = "/opt/audiocpp/audiocpp_server"

[speech]
# Engines offered to clients besides tts.engine, which is always offered and
# is the default. Each is downloaded at startup.
engines = []

[llm]
# AI processing for clients: an OpenAI-compatible language model service this
# server passes their requests on to. See "AI processing through the server".
enabled = false
endpoint = "http://localhost:11434"
model = "llama3.2:1b"
# The service's own key, if it needs one. Clients never see it.
api_key = ""
# How long one answer may take, in seconds.
timeout_secs = 60

[tls]
# true = encrypt every connection (HTTPS only). Clients then connect with the
# pairing code. See "Encryption".
enabled = false
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
| `VOXCTRL_SERVER_TLS` | `tls.enabled` | `true` |
| `VOXCTRL_SERVER_PUBLIC_HOST` | `public_host` | `192.168.1.50` |
| `VOXCTRL_SERVER_BACKEND` | `engine.backend` | `parakeet` |
| `VOXCTRL_SERVER_MODEL` | `model_size` of the selected backend | `tdt-0.6b-v3` |
| `VOXCTRL_SERVER_DEVICE` | `device` of the selected backend | `cpu` |
| `VOXCTRL_SERVER_LANGUAGE` | `language` of the selected backend | `en` |
| `VOXCTRL_SERVER_CLEANUP` | `cleanup.enabled` | `true` |
| `VOXCTRL_SERVER_CLEANUP_GPU` | `cleanup.gpu` | `false` |
| `VOXCTRL_SERVER_TTS` | `tts.enabled` | `true` |
| `VOXCTRL_SERVER_TTS_ENGINE` | `tts.engine` | `piper` |
| `VOXCTRL_SERVER_TTS_ENGINES` | `speech.engines`: every engine clients may choose between, comma-separated. The first is the default unless `VOXCTRL_SERVER_TTS_ENGINE` is set | `kitten_tts_2,piper` |
| `VOXCTRL_SERVER_TTS_VOICE` | The default voice of the default engine (`piper`, `pocket_tts`, `kitten_tts_2`, `reverb`) | `en-us-lessac-medium` |
| `VOXCTRL_SERVER_AUDIOCPP_SERVER` | `tts.audiocpp_server`: an `audiocpp_server` binary to use instead of the one in the image or the managed one. In Docker, mount it into the container; it must be built against the image's glibc | `/opt/audiocpp/audiocpp_server` |
| `VOXCTRL_SERVER_LLM` | `llm.enabled` | `true` |
| `VOXCTRL_SERVER_LLM_ENDPOINT` | `llm.endpoint` | `http://host.docker.internal:11434` |
| `VOXCTRL_SERVER_LLM_MODEL` | `llm.model` | `llama3.2:1b` |
| `VOXCTRL_SERVER_LLM_API_KEY` | `llm.api_key` | `sk-…` |

A variable that is set but empty is ignored. Environment variables are never
written into `server.toml`.

### Command-line flags

| Flag | Meaning |
|---|---|
| `-c`, `--config <PATH>` | Config file to use |
| `-b`, `--bind <ADDR>` | Address and port to listen on |
| `--print-config` | Print the effective settings and exit |
| `--pairing-code` | Print the code a VoxCtrl app pastes to connect (address, API key and, with encryption on, the certificate fingerprint) and exit |
| `--healthcheck` | Ask the running server whether it is up; exit code 0 if so. Works with encryption on |
| `-h`, `--help` | Usage |
| `-V`, `--version` | Version |

From the AppImage or the `.deb`, put `--server` first:
`./VoxCtrl-linux-x86_64-vulkan.AppImage --server --print-config`.

Set `RUST_LOG=debug` for more detailed logs.

---

## Security

What you send to the server is a recording of your microphone, so treat the
port accordingly.

- **Keep the API key on.** Every request must carry it. Without it, anyone who
  can reach the port can use your server.
- **Keep encryption on.** A server set up from 0.9.19 on is encrypted from
  its first start. One set up before then still speaks plain HTTP, so audio
  and the API key cross the network in the clear, and its log warns about it
  on every start: turn on [encryption](#encryption).
- **Do not forward the port from the internet**, with or without encryption.
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

### Encryption

With encryption on, everything between the app and the server (audio, text,
voices, the API key) travels over HTTPS, and the app talks only to your server.

**A new server is encrypted.** From 0.9.19, a server that starts with no
`server.toml` turns encryption on: it makes a certificate for itself and keeps
it beside `server.toml` (`server-cert.pem` and `server-key.pem`, in the data
volume on Docker), and writes `enabled = true` under `[tls]` when it saves the
file. The log shows `Encryption: on` and the certificate's fingerprint. Connect
each app with the [pairing code](#with-a-pairing-code).

**A server set up before 0.9.19 is left as it was.** A `server.toml` with no
`[tls]` section keeps serving plain HTTP, so the upgrade disconnects no one.
The log warns `Encryption: off` on every start. One case is not covered: a
server that has no `server.toml` at all (its key set only through
`VOXCTRL_SERVER_API_KEY`) looks like a new install and starts encrypted. Set
`VOXCTRL_SERVER_TLS=false` to keep it on plain HTTP.

**Turn it on for an existing server.** Set `VOXCTRL_SERVER_TLS=true` in `.env`
(Docker) or add this to `server.toml`, then restart:

```toml
[tls]
enabled = true
```

**Pair each client again.** The server now answers HTTPS only, so clients set
up before stop connecting until they are paired: print the
[pairing code](#with-a-pairing-code) and paste it into each app. A client whose
fallback engine is set keeps dictating locally in the meantime. A client that
still sends plain HTTP gets an answer saying so (HTTP 426, *This server only
accepts encrypted connections*), which the app shows as the reason it is
offline.

**Turn it off** with `enabled = false` under `[tls]` or
`VOXCTRL_SERVER_TLS=false`, only on a network where you trust everyone who can
reach the port: the API key and every recording then cross it unencrypted.

**How the app knows it is your server.** No certificate authority vouches for
the server's certificate. Instead the pairing code carries its fingerprint, and
the app accepts that one certificate and nothing else. **Settings → Engine**
shows `🔒 Encrypted` with the fingerprint; it should match the one in the
server's log.

- A paired app never falls back to an unencrypted connection. If the address
  is changed to `http://`, it refuses to connect rather than send in the clear.
- If the app reports that *the server's certificate is not the one this app
  was paired with*, either the server's certificate files were replaced (pair
  again) or something else is answering at that address.
- **Remove pairing** in the app forgets the certificate, for going back to a
  server without encryption.
- Deleting `server-cert.pem` and `server-key.pem` makes the server create a
  new certificate on the next start. Every client must then be paired again.
- Other OpenAI-compatible clients (`curl`, scripts) need the certificate to
  connect: `curl --cacert server-cert.pem https://localhost:8787/health` from
  the server itself, or `curl -k` where you accept not checking it.

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

**Docker — experimental.** A Vulkan build of the image is published with each
release (`…:latest-vulkan`), and `docker-compose.gpu.yml` runs it with the GPU
passed through. Neither has been tested on real hardware yet, so expect to
adjust them:

```bash
curl -LO https://github.com/JRufer/VoxCtrl/releases/latest/download/docker-compose.gpu.yml

# in .env
VOXCTRL_SERVER_BACKEND=whisper-cpp
VOXCTRL_SERVER_MODEL=large-v3-turbo

docker compose -f docker-compose.yml -f docker-compose.gpu.yml up -d
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
| Is it healthy? | `docker compose ps` | `curl -k https://localhost:8787/health` |
| Change the model | Edit `.env`, then `docker compose up -d` | Edit `server.toml`, then restart |
| Upgrade | `docker compose pull && docker compose up -d` | `git pull`, rebuild, reinstall the binary, restart |

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
With a slow one (a large Whisper model on a CPU) live text lags.

**Dictations come first.** The model transcribes one request at a time, and
from 0.9.20 the server orders them:

- A **dictation** (someone has stopped speaking and is waiting for the text)
  waits its turn. Up to 8 may wait; a ninth is answered `503` with
  `Retry-After: 1` (*The server is busy*), and the app hands that dictation to
  its fallback engine instead of making its user wait minutes.
- A **live-text clip** runs only when the model is free and no dictation is
  waiting. Otherwise it is answered `503` at once and not run: the client's
  next clip carries more of the recording anyway. So one person's live text
  never delays another person's dictation.
- From 0.9.22 a dictation **stops a live-text clip that is running**. The
  model checks between steps whether it has been told to stop, so the dictation
  waits for one step, not for the rest of the clip; the clip's client is
  answered `503`, as if the clip had been skipped. A clip is also stopped when
  its client hangs up before the answer (the app gives a clip six seconds), so
  the server never finishes work nobody is waiting for.

The VoxCtrl app marks its live clips with the header `X-VoxCtrl-Purpose: live`
(from 0.9.20). Requests without it, from older apps and other clients, are
treated as dictations, as every request was before.

To stop live text, turn off **Settings → Visual & Feedback → Show live
transcript in overlay** on the client. The server log shows live clips as
ordinary `Transcribed 1.4s of audio in … ms` lines.

### Stalled connections

The server closes a connection that has not finished sending a request's
headers within 30 seconds (this also ends idle kept-alive connections), and
cuts off a request whose body pauses for 30 seconds. A dictation streamed while
it is spoken is not affected: the app sends audio continuously for as long as
the key is held. There is no limit on the number of connections; the server is
meant for your own network or a VPN (see [Security](#security)).

---

## API reference

The server implements the part of the OpenAI API that speech-to-text clients
use, so tools other than VoxCtrl can use it too.

Some of those clients are released separately from this repository, such as
[VoxCtrl Keys](https://github.com/JRufer/voxctrl-keys), the Android voice
keyboard. Before changing an endpoint, see
[Clients Outside This Repository](./clients.md) for what they rely on.

| Method and path | Key needed | Purpose |
|---|---|---|
| `GET /health` | No | Liveness check, version and compatibility (below) |
| `GET /v1/models` | Yes | The model the server is running |
| `POST /v1/audio/transcriptions` | Yes | Transcribe audio |
| `GET /v1/cleanup` | Yes | Whether cleanup is enabled: `{"available": true, "model": "s1-mini"}` |
| `POST /v1/cleanup` | Yes | Clean up a transcript |
| `GET /v1/chat/completions` | Yes | Whether AI processing is enabled: `{"available": true, "model": "llama3.2:1b"}` |
| `POST /v1/chat/completions` | Yes | An OpenAI-style chat request, answered by the server's language model |
| `GET /v1/audio/speech` | Yes | Whether text-to-speech is enabled: `{"available": true, "engine": "piper", "voice": "…"}` |
| `POST /v1/audio/speech` | Yes | Speak text; returns a WAV file |
| `GET /v1/voices` | Yes, plus client id | The voices this client has uploaded |
| `PUT /v1/voices/{name}` | Yes, plus client id | Upload or replace a voice (multipart: `audio`, optional `transcript`, `fingerprint`) |
| `DELETE /v1/voices/{name}` | Yes, plus client id | Remove a voice |

The voice endpoints, and speech with an uploaded voice, need an
`X-VoxCtrl-Client` header of 16 to 64 hex digits naming the client.

`GET /health` also reports `"cleanup"`, `"tts"` and `"llm"` as `true` or `false`. `/v1/cleanup` is
VoxCtrl's own and not part of the OpenAI API.

`GET /health` says which apps the server works with (from 0.9.19):

| Field | Meaning |
|---|---|
| `version` | The server's VoxCtrl version |
| `contract` | The [client contract revision](./clients.md#client-contract-revisions) it speaks. Absent before 0.9.19, which means revision 1 |
| `min_client` | The oldest VoxCtrl app it works with |

The app reads these with its regular check of the server. If the server speaks
a newer contract or needs a newer app, the app counts the server as offline,
uses its fallback engine, and says to update VoxCtrl. If the server is older
than the app expects (before 0.9.16, which lacks AI processing), Settings →
Engine says so and dictation carries on.

The `curl` examples below use `http://`. With encryption on, use `https://`
and either `--cacert server-cert.pem` or `-k`.

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

The optional header `X-VoxCtrl-Purpose: live` marks a live-text clip, which is
run only when the model is idle and otherwise answered `503`. Without it the
request is a dictation and waits its turn (up to 8 waiting; then `503`). See
[Live text and server load](#live-text-and-server-load).

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

### `POST /v1/chat/completions`

An OpenAI-style chat request, passed on to the service under `[llm]`:

```bash
curl -s http://SERVER:8787/v1/chat/completions \
  -H "Authorization: Bearer $KEY" -H "Content-Type: application/json" \
  -d '{"messages": [{"role": "system", "content": "Fix grammar only."},
                    {"role": "user", "content": "me and him was there"}]}'
```

The server sets `model` to its own (whatever the request named) and `stream`
to `false`, and returns the service's answer unchanged, status included, so
that service's own error messages reach the client. Other fields
(`temperature` and so on) are passed through.

| Status | Meaning |
|---|---|
| 400 | Not a chat request, or more than 200,000 characters of message text |
| 501 | AI processing is not enabled on this server (`"type": "not_enabled"`) |
| 502 | The server could not reach its language model service, or it did not answer in time (`"type": "upstream_error"`) |
| Other | Whatever the language model service answered |

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
glibc 2.43 or newer. The published Docker image has it. One you built yourself
before 0.9.11 was based on Debian 13, which is too old: build it again, or
switch to the published image. For a system service on an older distribution, use Piper or eSpeak,
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

### AI processing does nothing with "Use the VoxCtrl server" on

The text is typed as dictated whenever the request fails. Click **Check the
server** under **Settings → OpenAI API**:

- *AI processing is turned off on this server*: set it up as in
  [AI processing through the server](#ai-processing-through-the-server).
- *This server does not offer AI processing*: the server is older than 0.9.16.
- It reports a model but nothing is rewritten: look at the server's log. `could
  not reach its language model service` means the address under `[llm]` is
  wrong from where the server runs (in Docker, `localhost` is the container);
  `answered HTTP 404` usually means the service does not have that model.
- Also check that the hotkey itself has AI processing switched on.

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

Also check that cleanup is on for that dictation at all: **Settings → Post-Processing →
Enable S1-mini dictation cleanup**, or the hotkey's own setting.

### The tray icon has an amber badge

The app can't reach the server. Hover the icon for the reason, or open
**Settings → Engine**. "Could not connect to the server" is the network or the
server being down: work through the next entry. "The server rejected the API
key" is the entry after it. Dictation keeps working in the meantime if a
[fallback engine](#when-the-server-cant-be-reached) is set.

### "The server's certificate is not the one this app was paired with"

The app is paired with a server with [encryption](#encryption) on, and the
server answering now presents a different certificate. If the server was
reinstalled, its data volume was recreated, or its certificate files were
deleted, print the pairing code again and paste it into the app. If none of
that happened, check that the address is the right machine.

### "This server was paired with encryption on, so its address must start with https://"

The endpoint was changed to an `http://` address while a certificate is still
saved. Paste the pairing code again to restore the address, or click **Remove
pairing** under **Settings → Engine** if the server no longer encrypts.

### "The server only accepts encrypted connections"

The app is set up with an `http://` address, and the server has encryption on
(every server set up from 0.9.19 has). Paste the server's
[pairing code](#with-a-pairing-code) instead of its address.

### "Update VoxCtrl on this machine" or "The server runs VoxCtrl …"

The app and the server compared versions ([`GET /health`](#api-reference)).
*Update VoxCtrl on this machine*: the server is newer than this app can work
with, so dictation uses the fallback engine until the app is updated. *The
server runs VoxCtrl …*: the server is older than this app expects; dictation
works, and updating the server brings back what is missing.

### Clients stopped connecting after encryption was turned on

Expected: the server answers HTTPS only once `tls.enabled` is set. Pair each
client with the [pairing code](#with-a-pairing-code).

### The AppImage with `--server` stops with "error while loading shared libraries"

The machine has no desktop libraries. The AppImage needs them even in server
mode; see [What the machine needs](#run-it-from-the-voxctrl-appimage). Install
a desktop environment's base libraries, or use Docker or the system service.

### Test Connection says "Connection failed"

The client could not reach the server at all.

1. On the server, confirm it is running and ready: `curl -k https://localhost:8787/health`
   (`http://` if encryption is off). If that fails, look at the logs; the model
   may still be downloading.
2. From the client, try the server's address: `curl -k https://192.168.1.50:8787/health`.
   If this fails while step 1 works, it is the network: check the firewall
   ([step 7](#7-open-the-port)), that `bind` is not `127.0.0.1`, and with Docker
   that the port is published (`docker compose ps` shows `0.0.0.0:8787->8787`).
3. Check the Endpoint URL in the app ends with `/v1`, and starts with
   `https://` for an encrypted server (the pairing code sets this) or `http://`
   for one with encryption off.

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
