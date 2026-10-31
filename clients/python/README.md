# fanimate (Python client)

Drive [Fanimate](../../README.md) from Python. The client starts `fanimate-cli script` and speaks JSON-RPC 2.0 to it, so there is nothing to install but this folder and nothing but the standard library inside it.

**Status: alpha.** It follows the scripting API, which is labelled `0.1.0-unstable` until Fanimate 1.0 (see [docs/scripting.md](../../docs/scripting.md)). It is not on PyPI yet.

```python
import fanimate

with fanimate.Host(save="title.fnm") as host:
    host.set_document(width=1280, height=720)
    title = host.add_text_layer(content="Made by a script", x=120, y=90)
    host.set_text_style(layer=title, style={"size": 96, "weight": 700})
    host.add_text_animator(layer=title, preset="typewriter")
    host.save()
```

- **Every API call is a method of the host**, with the parameters as keyword arguments: `host.add_layer(kind="vector", name="Art")`. `host.methods()` lists what the running Fanimate offers.
- **A failed call raises `fanimate.Error`.** Match on `error.code`, never on the message: `not_found`, `invalid_argument`, `playing`, `io_error`.
- **Finding Fanimate:** `Host(cli="path")`, else `$FANIMATE_CLI`, else `fanimate-cli` on the path.
- `fanimate.Host("scene.fnm")` opens a project; `host.save()` writes it back (or to `save=`).

Use it from a plain script, a notebook or a build pipeline: it needs no window, so it runs on a render server. Render and export a saved file with `fanimate-cli render` and `fanimate-cli export`.

## Tests

```
cargo build -p fanimate-cli
py -3 -m unittest discover -s clients/python/tests      # python3 on Linux and macOS
```

They run against the real CLI and are skipped when it cannot be found.
