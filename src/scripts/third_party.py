#!/usr/bin/env python3
"""Write THIRD_PARTY_LICENSES.txt: the license texts of everything shipped inside the tikontrol binary
(Rust crates in the server, JavaScript bundled into the UI) and of musl, the C library it links statically.

MIT, BSD, ISC and Apache-2.0 ask for their copyright and license text to travel with copies, so the file is
committed and copied into the container image next to the binary. Run it after changing dependencies:

    python3 src/scripts/third_party.py > THIRD_PARTY_LICENSES.txt

Reads the local cargo registry and src/ui/node_modules (run `cargo fetch` and `npm ci` first).
"""
import glob, json, os, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
SERVER, UI = os.path.join(ROOT, "src", "server"), os.path.join(ROOT, "src", "ui")
NAMES = ("LICENSE", "LICENCE", "COPYING", "NOTICE", "UNLICENSE")
# shipped in the UI bundle (runtime dependencies plus the Svelte runtime the compiler inlines)
UI_PACKAGES = ["svelte", "uplot", "qrcode-generator"]
# packages without a license file of their own: the license their metadata names, with its standard text
MIT = """Permission is hereby granted, free of charge, to any person obtaining a copy of this software and associated
documentation files (the "Software"), to deal in the Software without restriction, including without limitation the
rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies of the Software, and to permit
persons to whom the Software is furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions of the
Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE
WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR
COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE."""


def license_files(d):
    out = []
    for f in sorted(os.listdir(d)):
        if f.upper().startswith(NAMES) and os.path.isfile(os.path.join(d, f)):
            out.append((f, open(os.path.join(d, f), encoding="utf-8", errors="replace").read().strip()))
    return out


def section(title, url, lic, files):
    s = ["=" * 100, f"{title}", f"License: {lic}", f"Source: {url}", "=" * 100]
    for name, text in files:
        s += ["", f"--- {name} ---", "", text]
    return "\n".join(s) + "\n\n"


def crates():
    meta = json.loads(subprocess.run(["cargo", "metadata", "--locked", "--format-version", "1"], cwd=SERVER,
                                     capture_output=True, text=True, check=True).stdout)
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = meta["resolve"]["root"]
    # normal (non-dev) dependencies reachable from tikontrol, build scripts and proc macros included
    seen, todo = set(), [root]
    while todo:
        i = todo.pop()
        for d in nodes[i]["deps"]:
            if any(k["kind"] in (None, "build") for k in d["dep_kinds"]) and d["pkg"] not in seen:
                seen.add(d["pkg"])
                todo.append(d["pkg"])
    out = []
    for i in sorted(seen, key=lambda i: (pkgs[i]["name"], pkgs[i]["version"])):
        p = pkgs[i]
        files = license_files(os.path.dirname(p["manifest_path"]))
        if not files:
            # a few crates publish no license file; when MIT is one of the choices, use it with the crate's authors
            if "MIT" not in (p.get("license") or ""):
                sys.exit(f"no license file for crate {p['name']} {p['version']} and MIT is not an option: add its text by hand")
            who = ", ".join(p.get("authors") or []) or f"the {p['name']} authors"
            files = [("MIT license (chosen from the crate's metadata; the crate ships no license file)", f"Copyright (c) {who}\n\n{MIT}")]
        out.append(section(f"{p['name']} {p['version']} (Rust crate)", p.get("repository") or f"https://crates.io/crates/{p['name']}", p.get("license") or "?", files))
    return out


def ui():
    out = []
    for name in UI_PACKAGES:
        d = os.path.join(UI, "node_modules", name)
        p = json.load(open(os.path.join(d, "package.json")))
        files = license_files(d)
        if not files:
            author = p.get("author") if isinstance(p.get("author"), str) else (p.get("author") or {}).get("name", "")
            if p.get("license") != "MIT":
                sys.exit(f"no license file for {name} and its license is not MIT: add its text by hand")
            files = [("MIT license (from package.json; the package ships no license file)", f"Copyright (c) {author}\n\n{MIT}")]
        repo = p.get("repository")
        url = repo.get("url") if isinstance(repo, dict) else repo or p.get("homepage", "")
        out.append(section(f"{name} {p['version']} (JavaScript, bundled into the web UI)", url, p.get("license", "?"), files))
    return out


MUSL = section("musl libc (statically linked into the binary by the Alpine build)", "https://musl.libc.org/", "MIT",
               [("COPYRIGHT (summary)", "Copyright © 2005-2020 Rich Felker, et al.\n\n" + MIT +
                 "\n\nThe full COPYRIGHT file, with the list of contributors and the few files under other permissive "
                 "terms, is at https://git.musl-libc.org/cgit/musl/tree/COPYRIGHT")])

if __name__ == "__main__":
    head = ("tikontrol ships the third-party software below inside its binary and web UI.\n"
            "Each is used under its own license, reproduced here. Generated by src/scripts/third_party.py.\n\n")
    sys.stdout.write(head + "".join(crates()) + "".join(ui()) + MUSL)
