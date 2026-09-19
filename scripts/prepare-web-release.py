#!/usr/bin/env python3
"""Prepare static files with one content revision for the entire application graph.

Only the generated output is changed. Development/Tauri keep using apps/web.
No Node packages or bundler are required on the production host.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
from urllib.parse import parse_qsl, urlencode, urlsplit, urlunsplit


TEXT_SUFFIXES = {".html", ".js", ".css"}
# Static local URLs, including import(), new URL(), script/link tags and CSS imports.
# Deliberately exclude remote URLs, data URLs and interpolated pathnames.
ASSET_URL = re.compile(
    r"(?P<quote>['\"`])(?P<url>(?:\./|\.\./)?[\w./-]+\.(?:js|css|wasm)"
    r"(?:\?[^'\"`\s<>]*)?(?:#[\w-]*)?)(?P=quote)"
)
MARKER = ".web-build.json"
URL_CONTEXT = re.compile(
    r"(?:\bfrom\s+|\bimport\s*|\b(?:import|url)\s*\(\s*|"
    r"\bnew\s+URL\s*\(\s*|\b(?:src|href)\s*=\s*)$"
)


def rewrite_assets(text, revision):
    def replace(match):
        # A filename-looking string can also be a WASM ABI import key. Only
        # rewrite URL-bearing syntax, never keys in wasm-bindgen's import object.
        if not URL_CONTEXT.search(text[max(0, match.start() - 80):match.start()]):
            return match[0]
        parts = urlsplit(match["url"])
        # Worker and WASM paths currently use the same ASSET_VERSION expression.
        if "${" in parts.query and parts.query != "v=${ASSET_VERSION}":
            raise ValueError(f"Unsupported dynamic asset URL: {match['url']}")
        query = [(key, value) for key, value in parse_qsl(parts.query)
                 if key not in {"v", "rev"}]
        query.append(("rev", revision))
        url = urlunsplit((parts.scheme, parts.netloc, parts.path, urlencode(query), parts.fragment))
        return match["quote"] + url + match["quote"]
    return ASSET_URL.sub(replace, text)


def prepare(source, output):
    source, output = Path(source).resolve(), Path(output).resolve()
    if source == output or source in output.parents or output in source.parents:
        raise ValueError("Source and output directories must not overlap")
    for required in ["index.html", "design.css", "src/main.js", "package.json",
                     "pkg/bangdream_optimize_web_wasm.js", "pkg/bangdream_optimize_web_wasm_bg.wasm"]:
        if not (source / required).is_file():
            raise ValueError(f"Missing build input: {required}")
    files = sorted(path for path in source.rglob("*") if path.is_file()
                   and not set(path.relative_to(source).parts) & {"test", "__pycache__"}
                   and path.name not in {"config.desktop.js", MARKER})
    digest = hashlib.sha256()
    # Include the transformation itself, so a packaging fix also invalidates caches.
    digest.update(Path(__file__).read_bytes())
    for path in files:
        digest.update(path.relative_to(source).as_posix().encode() + b"\0")
        digest.update(hashlib.sha256(path.read_bytes()).digest())
    revision = digest.hexdigest()[:20]
    # Refuse to remove arbitrary directories; only replace our own generated output.
    if output.exists():
        if not (output / MARKER).is_file():
            raise ValueError(f"Output is not a generated web release: {output}")
        shutil.rmtree(output)
    output.mkdir(parents=True)
    for path in files:
        target = output / path.relative_to(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        if path.suffix in TEXT_SUFFIXES:
            target.write_text(rewrite_assets(path.read_text(encoding="utf-8"), revision), encoding="utf-8")
        else:
            shutil.copy2(path, target)
    (output / MARKER).write_text(json.dumps({"revision": revision}, indent=2) + "\n", encoding="utf-8")
    return revision


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", default="apps/web")
    parser.add_argument("--output", default="target/web-dist")
    args = parser.parse_args()
    print(f"Prepared web release {prepare(args.source, args.output)} in {args.output}")
