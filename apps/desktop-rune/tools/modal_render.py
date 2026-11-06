"""Render headless scenes from a CI-built bundle on Modal, then exit (the app is ephemeral).

    tools/ci-build.sh                       # push this branch, wait for CI, download bundle.tgz
    modal run tools/modal_render.py --bundle BUNDLE.tgz --scenes home,thread-showcase \
        [--themes dark,light] [--args "--chrome"] [--files a.json,b.json] [--out shots/]

Writes OUT/<scene>[-light].png and .tree.json, the same names tools/snap.sh produces. Extra
`--files` are uploaded to /work/files/<basename>, so `--args "--script /work/files/x.json"` works.
"""

import io
import os
import subprocess
import tarfile

import modal

image = (
    modal.Image.from_registry("ubuntu:24.04", add_python="3.12")
    .apt_install(
        "mesa-vulkan-drivers",
        "libvulkan1",
        "libxkbcommon0",
        "libxkbcommon-x11-0",
        "libfontconfig1",
        "libx11-6",
        "libxcursor1",
        "libxrandr2",
        "libxi6",
        "libwayland-client0",
        "fontconfig",
        "fonts-dejavu-core",
        "fonts-liberation",
    )
)

app = modal.App("s5code-rune-render", image=image)


@app.function(cpu=4, memory=4096, timeout=900)
def render(bundle: bytes, jobs: list, files: dict) -> dict:
    os.makedirs("/work/files", exist_ok=True)
    tarfile.open(fileobj=io.BytesIO(bundle)).extractall("/work")
    for name, data in files.items():
        with open(f"/work/files/{name}", "wb") as f:
            f.write(data)
    env = dict(
        os.environ,
        VK_ICD_FILENAMES="/usr/share/vulkan/icd.d/lvp_icd.json",
        S5_FIXTURES="/work/fixtures",
    )
    out, logs = {}, []
    for scene, theme, extra in jobs:
        stem = scene + ("-light" if theme == "light" else "")
        cmd = ["/work/target/release/s5code-rune", "--snap", f"/tmp/{stem}.png", "--tree", f"/tmp/{stem}.tree.json",
               "--scene", scene, "--size", "1440x900", "--scale", "1", "--theme", theme, *extra]
        r = subprocess.run(cmd, env=env, capture_output=True, text=True, cwd="/work")
        noise = [l for l in (r.stdout + r.stderr).splitlines() if not l.startswith("rune: ")]
        logs.append(f"[{stem}] exit {r.returncode}" + ("".join("\n  " + l for l in noise[-20:]) if noise else ""))
        for ext in ("png", "tree.json"):
            path = f"/tmp/{stem}.{ext}"
            if os.path.exists(path):
                out[f"{stem}.{ext}"] = open(path, "rb").read()
    out["render.log"] = "\n".join(logs).encode()
    return out


@app.local_entrypoint()
def main(bundle: str, scenes: str, themes: str = "dark", args: str = "", files: str = "", out: str = "shots"):
    extra = args.split() if args else []
    jobs = [(s, t, extra) for s in scenes.split(",") if s for t in themes.split(",")]
    uploads = {os.path.basename(p): open(p, "rb").read() for p in files.split(",") if p}
    result = render.remote(open(bundle, "rb").read(), jobs, uploads)
    os.makedirs(out, exist_ok=True)
    for name, data in result.items():
        with open(os.path.join(out, name), "wb") as f:
            f.write(data)
    print(result["render.log"].decode())
    print(f"{len(result) - 1} files -> {out}/")
