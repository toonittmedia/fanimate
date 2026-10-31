"""Tests of the Python client against a real ``fanimate-cli``.

They need the CLI: set ``FANIMATE_CLI`` to its path, or build it first
(``cargo build -p fanimate-cli``) and they find ``target/debug``. Without one
they are skipped, so the folder can be tested on its own.
"""

import json
import os
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.dirname(HERE))

import fanimate  # noqa: E402


def locate_cli():
    if os.environ.get("FANIMATE_CLI"):
        return os.environ["FANIMATE_CLI"]
    root = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
    for name in ("fanimate-cli.exe", "fanimate-cli"):
        path = os.path.join(root, "target", "debug", name)
        if os.path.exists(path):
            return path
    return None


CLI = locate_cli()


@unittest.skipUnless(CLI, "fanimate-cli was not found (set FANIMATE_CLI or build it)")
class HostTests(unittest.TestCase):
    def test_a_title_is_built_and_read_back_with_non_ascii_text(self):
        with fanimate.Host(cli=CLI) as host:
            self.assertTrue(host.version())
            layer = host.add_text_layer(content="Héllo مرحبا 你好", x=10, y=20)
            info = host.text(layer=layer)
            self.assertEqual(info["content"], "Héllo مرحبا 你好")
            host.set_text_style(layer=layer, style={"size": 48})
            self.assertEqual(host.text(layer=layer)["size"], 48)

    def test_every_failure_raises_with_the_apis_stable_code(self):
        with fanimate.Host(cli=CLI) as host:
            with self.assertRaises(fanimate.Error) as e:
                host.layer(id=99)
            self.assertEqual(e.exception.code, "not_found")
            with self.assertRaises(fanimate.Error) as e:
                host.set_document(width=0)
            self.assertEqual(e.exception.code, "invalid_argument")
            with self.assertRaises(fanimate.Error) as e:
                host.no_such_method()
            self.assertEqual((e.exception.code, e.exception.rpc_code), ("not_found", -32601))
            # and the session goes on
            self.assertEqual(host.layers(), [])

    def test_undo_redo_and_the_method_list(self):
        with fanimate.Host(cli=CLI) as host:
            host.add_layer(kind="vector", name="Art")
            self.assertTrue(host.undo())
            self.assertEqual(host.layers(), [])
            self.assertTrue(host.redo())
            names = {m["name"] for m in host.methods()}
            self.assertIn("add_layer", names)
            self.assertIn("bake_spring", names)

    def test_a_scene_is_saved_and_opened_again(self):
        with tempfile.TemporaryDirectory() as folder:
            path = os.path.join(folder, "scene.fnm")
            with fanimate.Host(cli=CLI, save=path) as host:
                host.set_document(name="Scene", width=640, height=360)
                art = host.add_layer(kind="vector", name="Ball")
                host.add_shape(layer=art, shape={"kind": "ellipse", "x": 0, "y": 0, "width": 50, "height": 50, "fill": "#e63946"})
                host.set_layer_transform(id=art, x=100, y=100)
                host.save()
            self.assertTrue(os.path.getsize(path) > 0)
            with fanimate.Host(path, cli=CLI) as again:
                self.assertEqual(again.document()["name"], "Scene")
                self.assertEqual([l["name"] for l in again.layers()], ["Ball"])

    def test_closing_twice_and_calling_after_close_are_errors_not_hangs(self):
        host = fanimate.Host(cli=CLI)
        host.close()
        host.close()
        with self.assertRaises(fanimate.Error) as e:
            host.layers()
        self.assertEqual(e.exception.code, "io_error")

    def test_a_socket_host_is_shared_needs_its_token_and_stops_on_shutdown(self):
        server = subprocess.Popen([CLI, "serve"], stdout=subprocess.PIPE, text=True, encoding="utf-8")
        try:
            ready = json.loads(server.stdout.readline())
            with self.assertRaises(fanimate.Error) as e:
                fanimate.Host.connect(ready["port"], "wrong-token")
            self.assertEqual(e.exception.code, "unauthorized")
            first = fanimate.Host.connect(ready["port"], ready["token"])
            first.add_layer(kind="vector", name="Shared")
            first.close()
            second = fanimate.Host.connect(ready["port"], ready["token"])
            self.assertEqual([l["name"] for l in second.layers()], ["Shared"])
            second.shutdown()
            self.assertEqual(server.wait(timeout=20), 0)
        finally:
            if server.poll() is None:
                server.kill()
                server.wait()
            server.stdout.close()

    def test_connecting_to_nothing_is_an_error(self):
        with self.assertRaises(fanimate.Error) as e:
            fanimate.Host.connect(1, "x", timeout=2)
        self.assertEqual(e.exception.code, "io_error")

    def test_a_missing_cli_is_said_plainly(self):
        saved = os.environ.pop("FANIMATE_CLI", None)
        try:
            old_path = os.environ["PATH"]
            os.environ["PATH"] = ""
            try:
                with self.assertRaises(fanimate.Error) as e:
                    fanimate.Host()
                self.assertIn("fanimate-cli was not found", str(e.exception))
            finally:
                os.environ["PATH"] = old_path
        finally:
            if saved is not None:
                os.environ["FANIMATE_CLI"] = saved


if __name__ == "__main__":
    unittest.main()
