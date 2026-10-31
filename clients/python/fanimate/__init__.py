"""Drive Fanimate from Python.

Starts ``fanimate-cli script`` as a child process and talks JSON-RPC 2.0 to it,
one request per line. There is nothing to install but this folder: it uses
only the standard library. Every Fanimate call is a method of the host::

    import fanimate

    with fanimate.Host() as host:
        title = host.add_text_layer(content="Hello", x=100, y=80)
        host.set_text_style(layer=title, style={"size": 96})
        host.save(path="title.fnm")

A call that fails raises :class:`Error`, whose ``code`` is one of the API's
stable strings (``not_found``, ``invalid_argument``, ``playing``,
``io_error``). ``host.methods()`` lists what the Fanimate you started offers,
and ``docs/scripting.md`` in the Fanimate repository describes each one.
"""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import threading
from typing import Any, Optional

__all__ = ["Host", "Error", "find_cli"]
__version__ = "0.1.0"


class Error(Exception):
    """A call Fanimate refused or could not do."""

    def __init__(self, code: str, message: str, rpc_code: Optional[int] = None):
        super().__init__(f"{code}: {message}")
        #: The API's stable code: ``not_found``, ``invalid_argument``, ``playing``, ``io_error``.
        self.code = code
        self.message = message
        #: The JSON-RPC error number, when there was one (``-32601`` for an unknown method).
        self.rpc_code = rpc_code


def find_cli(cli: Optional[str] = None) -> str:
    """The ``fanimate-cli`` to run: the one given, ``$FANIMATE_CLI``, or the one on the path."""
    for candidate in (cli, os.environ.get("FANIMATE_CLI")):
        if candidate:
            return candidate
    found = shutil.which("fanimate-cli")
    if found:
        return found
    raise Error("not_found", "fanimate-cli was not found: pass cli=..., set FANIMATE_CLI, or put it on the path")


class Host:
    """A running Fanimate with no window, scripted over standard input and output.

    ``project`` opens that file (``save()`` writes it back, or to ``save`` given here);
    left out, it starts an empty 1920 by 1080 project.
    """

    def __init__(
        self,
        project: Optional[str] = None,
        *,
        save: Optional[str] = None,
        cli: Optional[str] = None,
    ):
        command = [find_cli(cli), "script"]
        if project:
            command.append(project)
        if save:
            command += ["--save", save]
        self._process = subprocess.Popen(
            command,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            encoding="utf-8",
        )
        self._next_id = 0
        self._lock = threading.Lock()
        self._socket: Optional[socket.socket] = None
        self._reader = None
        self._writer = None

    @classmethod
    def connect(cls, port: int, token: str, *, host: str = "127.0.0.1", timeout: float = 30.0) -> "Host":
        """Connect to a Fanimate started with ``fanimate-cli serve``, which prints the port and token.

        The connection is to a project that other clients may use too. ``close()`` ends your
        connection only; the host keeps running (call ``shutdown()`` to stop it).
        """
        self = cls.__new__(cls)
        self._process = None
        self._next_id = 0
        self._lock = threading.Lock()
        try:
            self._socket = socket.create_connection((host, port), timeout=timeout)
        except OSError as e:
            raise Error("io_error", f"could not connect to {host}:{port}: {e}") from e
        self._socket.settimeout(None)
        self._reader = self._socket.makefile("r", encoding="utf-8", newline="\n")
        self._writer = self._socket.makefile("w", encoding="utf-8", newline="\n")
        try:
            self.call("auth", token=token)
        except Error:
            self.close()
            raise
        return self

    # -- the protocol -------------------------------------------------------

    def call(self, method: str, **params: Any) -> Any:
        """Run one method and return its result, or raise :class:`Error`."""
        with self._lock:
            self._next_id += 1
            request = {"jsonrpc": "2.0", "id": self._next_id, "method": method, "params": params}
            answer = self._exchange(json.dumps(request, ensure_ascii=False))
        if answer.get("id") != request["id"]:
            raise Error("invalid_argument", f"the answer was for request {answer.get('id')!r}, not {request['id']}")
        if "error" in answer:
            e = answer["error"]
            data = e.get("data") or {}
            raise Error(data.get("code", "invalid_argument"), e.get("message", ""), e.get("code"))
        return answer.get("result")

    def notify(self, method: str, **params: Any) -> None:
        """Run a method and do not wait for an answer (there is none)."""
        with self._lock:
            self._write(json.dumps({"jsonrpc": "2.0", "method": method, "params": params}, ensure_ascii=False))

    def _write(self, line: str) -> None:
        if self._process is None:
            stdin = self._writer
            if stdin is None:
                raise Error("io_error", "the connection is closed")
        else:
            stdin = self._process.stdin
            if stdin is None or self._process.poll() is not None:
                raise Error("io_error", "Fanimate is not running")
        try:
            stdin.write(line + "\n")
            stdin.flush()
        except (BrokenPipeError, OSError) as e:
            raise Error("io_error", f"Fanimate stopped: {e}") from e

    def _exchange(self, line: str) -> dict:
        self._write(line)
        stdout = self._reader if self._process is None else self._process.stdout
        try:
            reply = stdout.readline() if stdout else ""
        except (OSError, ValueError) as e:
            raise Error("io_error", f"Fanimate stopped: {e}") from e
        if not reply:
            raise Error("io_error", "Fanimate ended without answering")
        try:
            return json.loads(reply)
        except json.JSONDecodeError as e:
            raise Error("io_error", f"Fanimate sent something that is not JSON: {e}") from e

    # -- conveniences -------------------------------------------------------

    def version(self) -> str:
        """The API version of the Fanimate that is running."""
        return self.call("api.version")

    def methods(self) -> list:
        """What this Fanimate offers: name, parameters, whether it edits, and a sentence."""
        return self.call("api.methods")

    def __getattr__(self, name: str):
        # host.add_layer(kind="vector", name="Art") is host.call("add_layer", ...)
        if name.startswith("_"):
            raise AttributeError(name)

        def method(**params: Any) -> Any:
            return self.call(name, **params)

        method.__name__ = name
        return method

    # -- lifetime -----------------------------------------------------------

    def shutdown(self) -> None:
        """Stop the Fanimate this is connected to (a ``serve`` host). Save first."""
        self.call("shutdown")
        self.close()

    def close(self) -> None:
        """End the session. Unsaved changes are lost: call ``save`` first."""
        process = self._process
        if process is None:
            for stream in (self._writer, self._reader, self._socket):
                if stream:
                    try:
                        stream.close()
                    except OSError:
                        pass
            self._writer = self._reader = self._socket = None
            return
        if process.poll() is None:
            try:
                self.notify("quit")
            except Error:
                pass
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
        for stream in (process.stdin, process.stdout):
            if stream:
                try:
                    stream.close()
                except OSError:
                    pass

    def __enter__(self) -> "Host":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()
