#!/usr/bin/env python3
import argparse
import functools
import posixpath
import re
import time
import threading
from urllib.request import build_opener, HTTPRedirectHandler, Request
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlsplit


HEADER_PATH = re.compile(r"assets/(jp|cn|en|tw|kr)/(characters/resourceset/[A-Za-z0-9_-]+_rip/card_(normal|after_training)\.png|event/[A-Za-z0-9_-]+/(topscreen_rip/(trim_eventtop|bg_eventtop)|images_rip/logo)\.png)\Z")
HEADER_CACHE = {}
HEADER_LOCK = threading.Lock()
class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


class WebStaticHandler(SimpleHTTPRequestHandler):
    def __init__(self, *args, web_root: Path, game_data_root: Path, **kwargs):
        self.web_root = web_root
        self.game_data_root = game_data_root
        super().__init__(*args, directory=str(web_root), **kwargs)

    def do_GET(self):
        if not self.path.startswith("/bestdori/header/"):
            return super().do_GET()
        path = unquote(self.path.removeprefix("/bestdori/header/"))
        if not HEADER_PATH.fullmatch(path):
            self.send_error(400)
            return
        try:
            with HEADER_LOCK:
                entry = HEADER_CACHE.get(path)
            if entry and time.monotonic() - entry[0] < 86400:
                data = entry[1]
            else:
                with build_opener(NoRedirect).open(Request("https://bestdori.com/" + path, headers={"User-Agent": "Mozilla/5.0 BanG-Dream-Optimize/0.3"}), timeout=10) as response:
                    data = response.read(4 * 1024 * 1024 + 1)
                if len(data) > 4 * 1024 * 1024 or not data.startswith(b"\x89PNG\r\n\x1a\n"):
                    raise ValueError("Invalid header PNG")
                with HEADER_LOCK:
                    while HEADER_CACHE and (len(HEADER_CACHE) >= 12 or sum(len(v[1]) for v in HEADER_CACHE.values()) + len(data) > 8 * 1024 * 1024):
                        HEADER_CACHE.pop(next(iter(HEADER_CACHE)))
                    HEADER_CACHE[path] = (time.monotonic(), data)
            self.send_response(200)
            self.send_header("Content-Type", "image/png")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        except Exception as error:
            self.log_error("Header asset unavailable (%s): %s", type(error).__name__, error)
            self.send_error(502, "Header asset unavailable")

    def end_headers(self):
        self.send_header("Cache-Control", "no-store, no-cache, must-revalidate")
        self.send_header("Pragma", "no-cache")
        self.send_header("Expires", "0")
        super().end_headers()

    def translate_path(self, path):
        url_path = urlsplit(path).path
        if url_path == "/game-data" or url_path.startswith("/game-data/"):
            return self._game_data_path(url_path)
        return super().translate_path(path)

    def _game_data_path(self, url_path):
        relative = url_path.removeprefix("/game-data").lstrip("/")
        relative = posixpath.normpath(unquote(relative))
        if relative in ("", "."):
            return str(self.game_data_root)

        parts = [
            part
            for part in relative.split("/")
            if part not in ("", ".", "..")
        ]
        return str(self.game_data_root.joinpath(*parts))


def parse_args():
    parser = argparse.ArgumentParser(description="Serve web UI and /game-data.")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", default=8080, type=int)
    parser.add_argument("--web-root", default="apps/web")
    parser.add_argument("--game-data-root", default="var/game-data")
    return parser.parse_args()


def main():
    args = parse_args()
    web_root = Path(args.web_root).resolve()
    game_data_root = Path(args.game_data_root).resolve()

    if not web_root.exists():
        raise SystemExit(f"web root does not exist: {web_root}")
    if not game_data_root.exists():
        raise SystemExit(f"game-data root does not exist: {game_data_root}")

    handler = functools.partial(
        WebStaticHandler,
        web_root=web_root,
        game_data_root=game_data_root,
    )
    server = ThreadingHTTPServer((args.host, args.port), handler)
    print(f"Serving web UI from {web_root}")
    print(f"Serving /game-data from {game_data_root}")
    print(f"Listening on http://{args.host}:{args.port}")
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
