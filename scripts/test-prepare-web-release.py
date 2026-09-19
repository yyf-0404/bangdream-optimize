import importlib.util
from pathlib import Path
import tempfile
import unittest


spec = importlib.util.spec_from_file_location("web_release", Path(__file__).with_name("prepare-web-release.py"))
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class WebReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.source, self.output = self.root / "source", self.root / "output"
        self.original = {
            "index.html": '<link rel="stylesheet" href="./design.css?v=63"><script type="module" src="./src/main.js?v=64"></script>',
            "design.css": "@import url('./src/ui/style.css') layer(approved);",
            "src/ui/style.css": ".app {color: blue}",
            "src/main.js": "import {value} from './event.js?v=3'; import './player.js';",
            "src/player.js": "import {value} from './event.js';",
            "src/event.js": "export const value = 1;",
            "src/worker.js": "import(`../pkg/bangdream_optimize_web_wasm.js?v=${ASSET_VERSION}`); new URL(`../pkg/bangdream_optimize_web_wasm_bg.wasm?v=${ASSET_VERSION}`, import.meta.url);",
            "pkg/bangdream_optimize_web_wasm.js": "new URL('bangdream_optimize_web_wasm_bg.wasm', import.meta.url);",
            "pkg/bangdream_optimize_web_wasm_bg.wasm": b"\x00asm\x01\x00\x00\x00",
            "package.json": '{"version":"0.4.4","type":"module"}',
            "config.desktop.js": "// local desktop configuration",
            "test/example.test.js": "// test only",
        }
        for relative, content in self.original.items():
            path = self.source / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content if isinstance(content, bytes) else content.encode())

    def build(self):
        return release.prepare(self.source, self.output)

    def test_all_nested_urls_and_worker_wasm_share_revision(self):
        revision = self.build()
        for relative in ["index.html", "design.css", "src/main.js", "src/player.js",
                         "src/worker.js", "pkg/bangdream_optimize_web_wasm.js"]:
            text = (self.output / relative).read_text()
            self.assertIn(f"?rev={revision}", text)
            self.assertNotIn("?v=", text)
        # Previously these were two module identities and could hold different exports.
        self.assertIn(f"'./event.js?rev={revision}'", (self.output / "src/main.js").read_text())
        self.assertIn(f"'./event.js?rev={revision}'", (self.output / "src/player.js").read_text())
        self.assertNotIn("${ASSET_VERSION}", (self.output / "src/worker.js").read_text())
        wasm = "pkg/bangdream_optimize_web_wasm_bg.wasm"
        self.assertEqual((self.output / wasm).read_bytes(), self.original[wasm])

    def test_any_dependency_change_invalidates_entry_and_all_module_urls(self):
        previous = self.build()
        previous_entry = (self.output / "index.html").read_bytes()
        self.assertEqual(previous, self.build())
        for relative in ["src/event.js", "src/ui/style.css", "pkg/bangdream_optimize_web_wasm_bg.wasm"]:
            path = self.source / relative
            path.write_bytes(path.read_bytes() + b"\n")
            current = self.build()
            self.assertNotEqual(previous, current)
            self.assertNotEqual(previous_entry, (self.output / "index.html").read_bytes())
            self.assertIn(current, (self.output / "src/main.js").read_text())
            previous = current

    def test_external_urls_queries_and_layer_syntax(self):
        original = "@import url('./style.css?v=2&theme=dark#anchor') layer(runtime); import('https://cdn.example/a.js');"
        self.assertEqual(release.rewrite_assets(original, "abc"),
                         "@import url('./style.css?theme=dark&rev=abc#anchor') layer(runtime); import('https://cdn.example/a.js');")

    def test_wasm_abi_module_names_are_not_asset_urls(self):
        wrapper = 'const imports = {"./app_bg.js": import0}; imports["./app_bg.js"] = import0; new URL("app_bg.wasm", import.meta.url);'
        self.assertEqual(release.rewrite_assets(wrapper, "abc"),
                         'const imports = {"./app_bg.js": import0}; imports["./app_bg.js"] = import0; new URL("app_bg.wasm?rev=abc", import.meta.url);')

    def test_preserves_source_and_excludes_desktop_config_and_tests(self):
        self.build()
        self.assertFalse((self.output / "config.desktop.js").exists())
        self.assertFalse((self.output / "test").exists())
        for relative, content in self.original.items():
            self.assertEqual((self.source / relative).read_bytes(), content if isinstance(content, bytes) else content.encode())

    def test_rebuild_removes_obsolete_output(self):
        self.build()
        (self.output / "obsolete.js").write_text("old")
        self.build()
        self.assertFalse((self.output / "obsolete.js").exists())

    def test_refuses_overlapping_or_unowned_output_and_missing_wasm(self):
        for output in [self.source, self.root, self.source / "dist"]:
            with self.assertRaises(ValueError):
                release.prepare(self.source, output)
        self.output.mkdir()
        keep = self.output / "keep.txt"
        keep.write_text("keep")
        with self.assertRaises(ValueError):
            self.build()
        self.assertEqual(keep.read_text(), "keep")
        (self.source / "pkg/bangdream_optimize_web_wasm_bg.wasm").unlink()
        with self.assertRaisesRegex(ValueError, "Missing build input"):
            self.build()


if __name__ == "__main__":
    unittest.main()
