# Server Setup: s5code fork + T3 Connect

Runbook for deploying this fork (`SparshKaushik/s5code`) on a headless Ubuntu VPS and
enabling **T3 Connect** against the official relay. Last executed: 2026-08-02.

## Target environment

| Thing            | Value                                                                                |
| ---------------- | ------------------------------------------------------------------------------------ |
| Host             | `35.208.222.110` (Oracle Cloud, Ubuntu 26.04 LTS, 2 vCPU / 952 MiB RAM / 28 GB disk) |
| SSH              | `ssh -i ~/myoci.key ubuntu@35.208.222.110`                                           |
| Fork             | `https://github.com/SparshKaushik/s5code` (`main`, HEAD was `4323ea24`)              |
| Code dir         | `~/s5code` on the server                                                             |
| T3 home / data   | `~/.t3` (default)                                                                    |
| Server port      | `3773`, bound to loopback only (tunnel handles public access)                        |
| Service          | systemd user unit `t3code.service` (auto-starts at boot via linger)                  |
| Connect relay    | `https://relay.t3.codes` (official)                                                  |
| Managed endpoint | `https://prod-93f0f90f6de71daa.t3coderelay.com`                                      |

> ⚠️ The box only has ~1 GB of RAM. The web+server bundle **cannot** be built on it
> (rolldown thrashes swap for an hour+). Build on a dev machine and rsync `dist/`
> instead — see [Build](#4-build) below. The installed `node_modules` stay on the
> server (native modules like `node-pty` must be built for Linux there).

## 0. Prefer the precompiled binary (no build, no install)

Every release ships a **precompiled, standalone Linux server binary** on the
GitHub Releases page, so you can skip the entire toolchain below (Node, pnpm,
vite-plus, `build-essential`, swap, build on a dev machine, rsync).

Two assets are published per release:

| Asset                                 | Arch             |
| ------------------------------------- | ---------------- |
| `s5code-server-<version>-linux-x64`   | x86_64 / amd64   |
| `s5code-server-<version>-linux-arm64` | ARM64 / Graviton |

Each is a `bun build --compile` executable: the Bun runtime and the bundled
server (`dist/bin.mjs`) are in one file. No Node, pnpm, or `node_modules` are
needed on the host.

> The binary serves the **API and WebSocket only** — the web client is not
> bundled into it. Connect from [app.t3.codes](https://app.t3.codes), the desktop
> app, or mobile. (Embedding a directory tree in a compiled binary needs Bun's
> `--asset`, which ships in Bun 1.4; on the pinned 1.3.x it is silently ignored.)

### 0a. Download and run

```bash
# Pick your arch (here: x64). Download from the GitHub release, or with curl:
curl -fL -o s5code-server https://github.com/SparshKaushik/s5code/releases/latest/download/s5code-server-<version>-linux-x64
chmod +x s5code-server

# Serve on the loopback port (default 3773):
./s5code-server serve

# You can pass the same flags as the Node CLI:
./s5code-server --version
./s5code-server serve --port 3773
./s5code-server pair
```

### 0b. T3 Connect env vars

The binary reads the same public-config env vars at runtime (no rebuild needed):

```bash
T3CODE_CLERK_PUBLISHABLE_KEY=pk_live_Y2xlcmsudDMuY29kZXMk \
T3CODE_CLERK_JWT_TEMPLATE=t3-relay \
T3CODE_CLERK_CLI_OAUTH_CLIENT_ID=hzxSgY2cH10sDU2r \
T3CODE_RELAY_URL=https://relay.t3.codes \
./s5code-server serve
```

### 0c. systemd service (download-and-run)

```ini
[Unit]
Description=T3 Code server (s5code fork, precompiled)
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=simple
WorkingDirectory=/home/ubuntu
Environment=T3CODE_HOME=/home/ubuntu/.t3
Environment=T3CODE_CLERK_PUBLISHABLE_KEY=pk_live_Y2xlcmsudDMuY29kZXMk
Environment=T3CODE_CLERK_JWT_TEMPLATE=t3-relay
Environment=T3CODE_CLERK_CLI_OAUTH_CLIENT_ID=hzxSgY2cH10sDU2r
Environment=T3CODE_RELAY_URL=https://relay.t3.codes
ExecStart=/home/ubuntu/s5code-server serve
KillMode=control-group
Restart=always
RestartSec=5
StandardOutput=append:/home/ubuntu/.t3/t3code-server.log
StandardError=append:/home/ubuntu/.t3/t3code-server.log

[Install]
WantedBy=default.target
```

```bash
systemctl --user daemon-reload
loginctl enable-linger ubuntu
systemctl --user enable --now t3code.service
systemctl --user status t3code.service
```

### 0d. Updating (one click, no SSH)

The binary updates **itself**. When a client notices a version mismatch it shows
**Update server**; the binary then downloads the matching release asset for its
own version and arch, runs `--version` on the download to prove it works, swaps
itself with an atomic `rename`, and restarts. (The public descriptor advertises
`boot-service` for T3 Connect / app.t3.codes schema compatibility; the update
RPC still uses the binary download path.)

Under any systemd unit (the binary detects it via `INVOCATION_ID`, which
systemd sets for every unit) the binary exits and `Restart=always` starts the
replacement, so the service manager stays the single owner of the process. Run
unsupervised, it spawns its replacement detached and exits.

If the download is missing, truncated, built for the wrong arch, or reports a
different version, the update aborts and the running binary is left untouched.
Updates are always user-triggered — nothing checks GitHub on a timer.

To update by hand instead, replace the file and restart the service:

```bash
curl -fL -o s5code-server.new https://github.com/SparshKaushik/s5code/releases/latest/download/s5code-server-<version>-linux-x64
chmod +x s5code-server.new && mv s5code-server.new s5code-server
systemctl --user restart t3code.service
```

### Why a binary is safe here

The server is Bun-first (`server.ts` picks `BunHttpServer` / `BunServices` /
`BunPtyAdapter` when `typeof Bun !== "undefined"`, and persistence uses
`bun:sqlite`), so the compiled executable needs none of the Node-only native
modules (`node-pty`) that make the source build require `build-essential`.
`tailscale` and the T3 Connect relay client (`cloudflared`) are still external
helpers spawned at runtime, exactly as in the source build.

Build it locally with:

```bash
vp run --filter t3 build            # produce apps/server/dist/bin.mjs
node scripts/build-server-binary.ts --arch x64 --output-dir release
node scripts/build-server-binary.ts --arch arm64 --output-dir release
```

Two build flags are load-bearing and documented in the script: `--splitting` is
**required** (without it the bundler hoists the Node SQLite driver's top-level
`node:sqlite` import into the entry chunk, and Bun does not implement that
module, so the binary dies at startup before it can choose `bun:sqlite`), and
`--bytecode` cannot be combined with `--splitting`.

---

The rest of this runbook documents the source build (clone + `vp i` + rsync),
which remains fully supported.

<details>
<summary>Source build (original runbook)</summary>

---

## 1. Provision the box

```bash
ssh -i ~/myoci.key ubuntu@35.208.222.110
```

### Base tools

```bash
sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y git curl ca-certificates build-essential rsync tmux
```

`build-essential` is required — `node-pty`'s install script (node-gyp) fails with
`not found: make` without it.

### Swap (4 GB)

The build + pnpm install need headroom on a 1 GB box:

```bash
sudo fallocate -l 4G /swapfile && sudo chmod 600 /swapfile
sudo mkswap /swapfile && sudo swapon /swapfile
echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab
```

### Node 24 + pnpm + vite-plus (`vp`)

The server requires `^22.16 || ^23.11 || >=24.10` (engines in `apps/server/package.json`):

```bash
curl -fsSL https://deb.nodesource.com/setup_24.x | sudo -E bash -
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y nodejs   # -> 24.18.1

sudo npm i -g pnpm@11                                           # -> 11.x

curl -fsSL https://viteplus.dev/install.sh | bash                # -> vp v0.2.7
export PATH="$HOME/.vite-plus/bin:$PATH"                        # add to ~/.bashrc
```

## 2. Clone the fork

```bash
git clone https://github.com/SparshKaushik/s5code.git ~/s5code
cd ~/s5code && git checkout main
```

## 3. Configure T3 Connect public values

Create `~/s5code/.env` with the **official** public identifiers — the same values baked
into the upstream `t3` package and this fork's release workflow
(`.github/workflows/release.yml`, job `relay_public_config`). These are **public,
not secrets**:

```dotenv
T3CODE_CLERK_PUBLISHABLE_KEY=pk_live_Y2xlcmsudDMuY29kZXMk
T3CODE_CLERK_JWT_TEMPLATE=t3-relay
T3CODE_CLERK_CLI_OAUTH_CLIENT_ID=hzxSgY2cH10sDU2r
T3CODE_RELAY_URL=https://relay.t3.codes
```

The server bundle reads these at build time via `scripts/lib/public-config.ts` and
bakes them into `dist/bin.mjs` (`vp pack` `define` in `apps/server/vite.config.ts`).
The bundled server also accepts the same vars as runtime env overrides.

## 4. Install dependencies (on the server)

```bash
cd ~/s5code
export PATH="$HOME/.vite-plus/bin:$PATH"
vp i --frozen-lockfile
```

Runs the pnpm workspace install plus the `prepare` script (`effect-tsgo patch`,
`vp config`). Cold pnpm store on a small box takes a while (~10 min); it only failed
once, on `node-pty` before `build-essential` was installed. Re-run after fixing
system deps — pnpm resumes from its store.

## 5. Build (on a dev machine — NOT the server)

The server bundle is `apps/server/dist/bin.mjs` + `dist/client/` (built web app) +
`dist/service-launcher.mjs` + `dist/pi-extension/`. Build on a machine with the same
toolchain (node 24.x, pnpm 11, vite-plus) and rsync it over.

### 5a. Build on the Mac

```bash
cd <repo>   # same fork checkout
export T3CODE_CLERK_PUBLISHABLE_KEY=pk_live_Y2xlcmsudDMuY29kZXMk
export T3CODE_CLERK_JWT_TEMPLATE=t3-relay
export T3CODE_CLERK_CLI_OAUTH_CLIENT_ID=hzxSgY2cH10sDU2r
export T3CODE_RELAY_URL=https://relay.t3.codes
vp run --filter t3 build
```

This runs `node scripts/cli.ts build` in `apps/server`, which builds the web app
(`@t3tools/web#build`), bundles the CLI (`vp pack`), copies `web/dist` →
`dist/client`, and copies the pi-extension.

### 5b. Verify the values were baked in

```bash
grep -c "relay.t3.codes"       apps/server/dist/bin.mjs   # 1
grep -c "pk_live_Y2xlcmsudDMuY29kZXMk" apps/server/dist/bin.mjs  # 1
```

### 5c. Sync to the server

```bash
rsync -az -e "ssh -i ~/myoci.key -o ConnectTimeout=25" \
  apps/server/dist/ ubuntu@35.208.222.110:~/s5code/apps/server/dist/
```

Native modules (node-pty etc.) are **external** to the bundle and resolve from the
server's own Linux `node_modules` at runtime, so a macOS-built bundle runs fine.

## 6. Run the server as a systemd service

> Do **not** use `t3 service install` here: it installs a pinned runtime
> `t3@<version>` from npm, which would pull the **upstream** package, not this fork.

Write `~/.config/systemd/user/t3code.service`:

```ini
[Unit]
Description=T3 Code server (s5code fork)
StartLimitIntervalSec=300
StartLimitBurst=5

[Service]
Type=simple
WorkingDirectory=/home/ubuntu/s5code
Environment=T3CODE_HOME=/home/ubuntu/.t3
Environment=T3CODE_CLERK_PUBLISHABLE_KEY=pk_live_Y2xlcmsudDMuY29kZXMk
Environment=T3CODE_CLERK_JWT_TEMPLATE=t3-relay
Environment=T3CODE_CLERK_CLI_OAUTH_CLIENT_ID=hzxSgY2cH10sDU2r
Environment=T3CODE_RELAY_URL=https://relay.t3.codes
ExecStart=/usr/bin/node /home/ubuntu/s5code/apps/server/dist/bin.mjs serve
KillMode=control-group
Restart=always
RestartSec=5
StandardOutput=append:/home/ubuntu/.t3/t3code-server.log
StandardError=append:/home/ubuntu/.t3/t3code-server.log

[Install]
WantedBy=default.target
```

Enable + start (linger makes it survive logout/boot):

```bash
systemctl --user daemon-reload
loginctl enable-linger ubuntu
systemctl --user enable --now t3code.service
systemctl --user status t3code.service
tail -f ~/.t3/t3code-server.log
```

Expected log lines: `Listening on http://127.0.0.1:3773` and
`agent activity publishing standby; waiting for T3 Connect link reconciliation`.

## 7. T3 Connect: link the environment

The CLI runs from the built bundle. `link` installs the managed `cloudflared`
binary, runs OAuth (out-of-band on a headless box), and records the link intent:

```bash
cd ~/s5code
node apps/server/dist/bin.mjs connect link --headless
```

Interactive flow (drive it inside a `tmux` session so it survives SSH drops):

1. **Install relay client?** — type `y` (downloads pinned `cloudflared`).
2. **Headless authorization** — the CLI prints a `https://app.t3.codes/connect#state=...&challenge=...`
   URL. Open it in a browser, sign in with the T3 account, copy the code the page
   shows, paste it back into the prompt.
3. Output: `✓ Authorized as <email>`.

Check state:

```bash
node apps/server/dist/bin.mjs connect status --json
# {"desired":true,"authenticated":true,"linked":true, "relayUrl":"https://relay.t3.codes", ...}
```

## 8. Restart to launch the tunnel

The tunnel is reconciled on server start. Restart the service:

```bash
systemctl --user restart t3code.service
sleep 15
tail -20 ~/.t3/t3code-server.log   # look for "Relay client tunnel connection registered"
```

Healthy output shows `cloudflared` registering QUIC connections to Cloudflare edge
(`Registered tunnel connection connIndex=0..3 ... protocol=quic`).

## 9. Verify end-to-end

```bash
# State
cd ~/s5code && node apps/server/dist/bin.mjs connect status   # Exposure: enabled, link: provisioned

# Public endpoint (hostname derived from the tunnel name, see note below)
curl -sS -o /dev/null -w "%{http_code}\n" https://prod-93f0f90f6de71daa.t3coderelay.com/   # 200

# WebSocket surface (401 = reachable, auth required — correct)
curl -sS -o /dev/null -w "%{http_code}\n" -H "Connection: Upgrade" -H "Upgrade: websocket" \
  -H "Sec-WebSocket-Version: 13" -H "Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==" \
  https://prod-93f0f90f6de71daa.t3coderelay.com/ws
```

Connecting: open the endpoint URL in a browser and pair, or sign in at
`https://app.t3.codes` / the desktop app / mobile app with the same account — the
environment appears under **Connections**.

### How the endpoint hostname is derived

The server log names the tunnel `t3coderelay-managedendpoint-prod-<hash16>`. The
relay builds the public hostname as `prod-<hash16>.<tunnel-zone>` (see
`infra/relay/src/deploymentConfig.ts` → `managedEndpointHostname`). The official
tunnel zone is `t3coderelay.com` (found by DNS: only that zone's record resolves the
hash to Cloudflare IPs, and curl returned 200). Verify a candidate with:

```bash
dig +short prod-93f0f90f6de71daa.t3coderelay.com CNAME  # Cloudflare-proxied → A 172.67.x.x
```

## Updating the server (pull + rebuild + restart)

```bash
# 1. Server: pull the fork
ssh -i ~/myoci.key ubuntu@35.208.222.110 'cd ~/s5code && git pull --ff-only'

# 2. Dev machine: rebuild with the T3 Connect env vars (step 5a) and rsync (step 5c)

# 3. Server: restart
ssh -i ~/myoci.key ubuntu@35.208.222.110 'systemctl --user restart t3code.service'
```

## Maintenance commands

```bash
ssh -i ~/myoci.key ubuntu@35.208.222.110

# State / health
cd ~/s5code && node apps/server/dist/bin.mjs connect status --json
systemctl --user status t3code.service --no-pager
tail -f ~/.t3/t3code-server.log

# Mint a fresh pairing URL + QR for the running server
cd ~/s5code && node apps/server/dist/bin.mjs pair

# Unlink / log out (retains or clears stored auth)
node apps/server/dist/bin.mjs connect unlink
node apps/server/dist/bin.mjs connect logout

# Stop / disable the service
systemctl --user stop t3code.service && systemctl --user disable t3code.service
```

## Troubleshooting

| Symptom                                             | Fix                                                                                                                  |
| --------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| `node-pty` install: `gyp ERR! not found: make`      | `sudo apt-get install -y build-essential`, re-run `vp i --frozen-lockfile`                                           |
| Server bundle missing at `apps/server/dist/bin.mjs` | Build on a dev machine (step 5) and rsync — do not build on a 1 GB box                                               |
| `connect` command group missing subcommands         | `.env` values not set before build → rebuild with env vars exported                                                  |
| `connect status` shows `linked: false` after `link` | Restart the service so it reconciles and launches the tunnel (step 8)                                                |
| Tunnel up but endpoint returns 502/error            | Verify DNS record and `systemctl --user status t3code.service`; check `~/.t3/t3code-server.log`                      |
| Interactive prompts die over SSH                    | Wrap in `tmux` (`tmux new-session -d -s t3link "..."`, `tmux capture-pane -t t3link -p`, `tmux send-keys -t t3link`) |

</details>
