"""Live-test the real windowed app on a Modal sandbox: Xvfb + lavapipe, the bundled Linux server
the app starts itself (src/host.rs), the free Replay (dev) agent and OpenCode (use its free
`opencode/*-free` models for real turns) for turns, and a demo git project. Anonymous free use is
rate-limited per IP and Modal's shared IPs are usually over it, so `start` signs OpenCode in with
$OPENCODE_API_KEY (an opencode.ai Zen key; free models stay free) when it is set.

    tools/ci-build.sh OUT                        # bundle.tgz + s5code-server for HEAD
    PY=/home/ubuntu/modal-venv/bin/python
    $PY tools/modal_live.py start OUT            # boot; prints when the window is up
    $PY tools/modal_live.py shot out.png         # screenshot of the 1440x900 screen
    $PY tools/modal_live.py click X Y [--right] [--double]
    $PY tools/modal_live.py move X Y
    $PY tools/modal_live.py drag X1 Y1 X2 Y2
    $PY tools/modal_live.py scroll X Y N         # N wheel notches, negative scrolls up
    $PY tools/modal_live.py key ctrl+k [Return ...]   # xdotool key names
    $PY tools/modal_live.py type 'some text'
    $PY tools/modal_live.py sh 'cmd'             # shell in the sandbox (git, curl, sqlite3, ...)
    $PY tools/modal_live.py logs                 # app stderr + server log tails
    $PY tools/modal_live.py restart [OUT]        # new build (optional), restart the app, keep data
    $PY tools/modal_live.py stop                 # always stop when done

The sandbox id lives in $S5_LIVE_STATE (default ~/.cache/s5code-modal-live.json); give each
concurrent tester its own. The
sandbox also stops on its own after 20 idle minutes, and after 3 hours regardless. The app's data
is /root/.s5code; the demo project is /root/projects/demo. Linux shortcuts use ctrl, not cmd.
"""

import json
import os
import sys
import time

import modal

STATE = os.environ.get("S5_LIVE_STATE", os.path.expanduser("~/.cache/s5code-modal-live.json"))

image = (
    modal.Image.from_registry("ubuntu:24.04", add_python="3.12")
    .apt_install(
        "mesa-vulkan-drivers", "libvulkan1", "libxkbcommon0", "libxkbcommon-x11-0", "libfontconfig1",
        "libx11-6", "libx11-xcb1", "libxcb1", "libxcursor1", "libxrandr2", "libxi6", "libwayland-client0",
        "fontconfig", "fonts-dejavu-core", "fonts-liberation", "xvfb", "xdotool", "imagemagick", "x11-utils",
        "git", "vim", "htop", "tmux", "less", "curl", "ca-certificates", "sqlite3", "iproute2",
    )
    .run_commands("curl -fsSL https://deb.nodesource.com/setup_24.x | bash - && apt-get install -y nodejs")
    # Real turns with OpenCode's free `opencode/*-free` models; see OPENCODE_API_KEY below.
    .run_commands("npm install -g opencode-ai@latest && opencode --version")
)

ENV = "export DISPLAY=:99 VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json HOME=/root"

SETTINGS = {
    "providerInstances": {
        "devReplay": {
            "displayName": "Replay (dev)",
            "driver": "acpRegistry",
            "config": {"source": "local", "commandPath": "/usr/bin/node",
                       "commandArgs": ["/work/replay/scripts/dev-replay-agent.ts"]},
        }
    }
}

SETUP = r"""
set -e
mkdir -p /work && tar xzf /tmp/bundle.tgz -C /work
install -m 755 /tmp/s5code-server /work/target/release/s5code-server
mkdir -p /root/.s5code/userdata
git config --global user.email live@s5code.test
git config --global user.name "Live Test"
git config --global init.defaultBranch main
mkdir -p /root/projects/demo/src && cd /root/projects/demo
printf '# Demo\n\nA small project for live tests.\n' > README.md
printf 'export function clamp(v: number, lo: number, hi: number) {\n  return Math.min(hi, Math.max(lo, v));\n}\n' > src/clamp.ts
printf 'node_modules\n' > .gitignore
git init -q && git add -A && git commit -qm "Initial commit"
"""

XVFB = r"""
setsid nohup Xvfb :99 -screen 0 1440x900x24 -nolisten tcp >/tmp/xvfb.log 2>&1 &
for _ in $(seq 1 50); do xdpyinfo -display :99 >/dev/null 2>&1 && break; sleep 0.2; done
"""

LAUNCH = ENV + r"""
cd /root
setsid nohup /work/target/release/s5code-rune >/tmp/app.log 2>&1 &
echo $! >/tmp/app.pid
for _ in $(seq 1 120); do
  w=$(xdotool search --name '^S5 Code$' 2>/dev/null | head -1)
  [ -n "$w" ] && { xdotool windowmove "$w" 0 0 windowsize "$w" 1440 900 windowfocus "$w" 2>/dev/null; echo "window $w"; exit 0; }
  kill -0 "$(cat /tmp/app.pid)" 2>/dev/null || { echo "app exited"; tail -40 /tmp/app.log; exit 1; }
  sleep 0.5
done
echo "no window after 60s"; tail -40 /tmp/app.log; exit 1
"""

# The app's server notices its launcher is gone (parent pid watch) and exits; wait for its port.
STOP_APP = r"""
pid=$(cat /tmp/app.pid 2>/dev/null) && kill "$pid" 2>/dev/null
for _ in $(seq 1 60); do ss -ltn | grep -q ':3773 ' || break; sleep 0.5; done
"""


def sandbox() -> modal.Sandbox:
    try:
        sid = json.load(open(STATE))["id"]
    except FileNotFoundError:
        sys.exit(f"no live sandbox ({STATE}); run `start` first")
    return modal.Sandbox.from_id(sid)


def sh(sb: modal.Sandbox, script: str, check: bool = True) -> str:
    p = sb.exec("bash", "-c", script)
    p.wait()
    out = p.stdout.read() + p.stderr.read()
    if check and p.returncode != 0:
        sys.exit(f"exit {p.returncode}\n{out}")
    return out


def upload_build(sb: modal.Sandbox, out_dir: str):
    sb.filesystem.copy_from_local(os.path.join(out_dir, "bundle.tgz"), "/tmp/bundle.tgz")
    sb.filesystem.copy_from_local(os.path.join(out_dir, "s5code-server"), "/tmp/s5code-server")


def xdo(args: str) -> str:
    return sh(sandbox(), f"{ENV}; xdotool {args}")


def main(argv: list[str]):
    cmd, rest = (argv[0], argv[1:]) if argv else ("help", [])
    if cmd == "start":
        out_dir = rest[0]
        if os.path.exists(STATE):
            sys.exit(f"{STATE} exists: `stop` the running sandbox first")
        app = modal.App.lookup("s5code-rune-live", create_if_missing=True)
        sb = modal.Sandbox.create("sleep", "infinity", app=app, image=image, cpu=4, memory=8192,
                                  timeout=3 * 3600, idle_timeout=20 * 60)
        os.makedirs(os.path.dirname(os.path.abspath(STATE)), exist_ok=True)
        json.dump({"id": sb.object_id}, open(STATE, "w"))
        upload_build(sb, out_dir)
        sh(sb, SETUP)
        sb.filesystem.write_text(json.dumps(SETTINGS), "/root/.s5code/userdata/settings.json")
        if key := os.environ.get("OPENCODE_API_KEY"):
            sh(sb, "mkdir -p /root/.local/share/opencode")
            sb.filesystem.write_text(json.dumps({"opencode": {"type": "api", "key": key}}),
                                     "/root/.local/share/opencode/auth.json")
        sh(sb, XVFB)
        print(sh(sb, LAUNCH).strip())
        print(f"sandbox {sb.object_id} up; state in {STATE}")
    elif cmd == "restart":
        sb = sandbox()
        sh(sb, STOP_APP, check=False)
        if rest:
            upload_build(sb, rest[0])
            sh(sb, "tar xzf /tmp/bundle.tgz -C /work && install -m 755 /tmp/s5code-server /work/target/release/s5code-server")
        print(sh(sb, LAUNCH).strip())
    elif cmd == "shot":
        sb = sandbox()
        sh(sb, f"{ENV}; import -window root /tmp/shot.png")
        sb.filesystem.copy_to_local("/tmp/shot.png", rest[0])
        print(rest[0])
    elif cmd == "click":
        x, y = rest[0], rest[1]
        button = "3" if "--right" in rest else "1"
        repeat = "--repeat 2 --delay 80 " if "--double" in rest else ""
        xdo(f"mousemove {x} {y} sleep 0.05 click {repeat}{button}")
    elif cmd == "move":
        xdo(f"mousemove {rest[0]} {rest[1]}")
    elif cmd == "drag":
        x1, y1, x2, y2 = rest[:4]
        xdo(f"mousemove {x1} {y1} mousedown 1 sleep 0.1 mousemove {x1} {int(y1) + 4} sleep 0.05 "
            f"mousemove {x2} {y2} sleep 0.1 mouseup 1")
    elif cmd == "scroll":
        x, y, n = rest[0], rest[1], int(rest[2])
        xdo(f"mousemove {x} {y} click --repeat {abs(n)} --delay 30 {5 if n > 0 else 4}")
    elif cmd == "key":
        xdo("key --delay 60 " + " ".join(rest))
    elif cmd == "type":
        xdo("type --delay 15 -- " + json.dumps(" ".join(rest)))
    elif cmd == "sh":
        print(sh(sandbox(), " ".join(rest), check=False), end="")
    elif cmd == "logs":
        print(sh(sandbox(), "echo '== app'; tail -60 /tmp/app.log; echo '== server';"
                 " tail -80 /root/.s5code/userdata/logs/desktop-rune-server.log", check=False))
    elif cmd == "stop":
        try:
            sandbox().terminate()
        finally:
            if os.path.exists(STATE):
                os.remove(STATE)
        print("stopped")
    else:
        print(__doc__)
    # Let the shot or input land before the caller's next step reads the screen.
    if cmd in ("click", "drag", "scroll", "key", "type", "move"):
        time.sleep(0.3)


if __name__ == "__main__":
    main(sys.argv[1:])
