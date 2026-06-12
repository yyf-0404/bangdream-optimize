#!/usr/bin/env python3
import argparse
import functools
import posixpath
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import unquote, urlsplit


class WebStaticHandler(SimpleHTTPRequestHandler):
    def __init__(self, *args, web_root: Path, game_data_root: Path, **kwargs):
        self.web_root = web_root
        self.game_data_root = game_data_root
        super().__init__(*args, directory=str(web_root), **kwargs)

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
