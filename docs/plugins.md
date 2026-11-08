# Plugins

A plugin is a small program that changes a Fanimate project through the [scripting API](scripting.md): add a title, rename layers by a rule, build a rig from names. Fanimate starts it, answers its calls, and holds it to the permissions it declared.

**Status: alpha.** The plugin host and the `fanimate-cli plugin` commands are built. The app does not run plugins yet (no Plugins menu, no permission prompt), so today a plugin is run from the command line, where whoever starts it has chosen to. The API is labelled `0.1.0-unstable` until 1.0.

## What a plugin is

A folder with two things in it:

```
title-maker/
  plugin.json     what it is and what it may do
  main.py         the entry: the program Fanimate starts
```

`plugin.json` is the manifest described in [scripting.md](scripting.md#permissions-and-plugins), plus one optional field, `timeout_seconds` (1 to 600, default 60), the longest a run may take:

```json
{
  "name": "Title Maker",
  "version": "1.0.0",
  "api": "0.1",
  "description": "Adds a title card.",
  "entry": "main.py",
  "permissions": ["read", "edit"],
  "timeout_seconds": 60
}
```

The entry is started by what its extension says: `.py` by Python 3 (`$FANIMATE_PYTHON`, else `python`, `py -3` or `python3`), `.js`, `.mjs` and `.cjs` by Node (`$FANIMATE_NODE`, else `node`), `.ts` and `.mts` by Node 22.6 or later (which strips the types), and anything else is run as a program of its own. The entry must be a file inside the plugin's folder.

## How it talks to Fanimate

The plugin writes one JSON-RPC request per line to its standard output and reads one answer per line from its standard input: the same calls, the same error codes, as `fanimate-cli script`. Print diagnostics to standard error; the host collects the first 64 KB and shows it. Anything else on standard output is taken for a request.

The starter plugin from `fanimate-cli plugin new` shows the loop in Python, JavaScript or TypeScript. In Node, `clients/node` offers it as `import { plugin } from 'fanimate'`; in Python use `clients/python`'s `Host` for a script that starts Fanimate, or the few lines in the starter for a plugin Fanimate starts.

Every call that changes the project is one undo step, all or nothing, and a call the manifest does not allow is answered `permission_denied` and does nothing. `save` is the host's: a plugin with the `save` permission may call it, and it writes to the path the host was given (`plugin run --save`) or to the `path` it names.

## Limits

A run ends when the plugin exits, asks to `quit`, or passes a limit, and then the process is killed: the time limit, 100,000 calls, or a line longer than 8 MB. The report says which.

## Not a sandbox

**The host controls what a plugin does to the project, not what it does to the computer.** A plugin is a program running as the user: it can read files, use the network and start other programs, whatever the manifest says. The permissions guard the project and the API, nothing else. So install plugins only from people you trust, the way you would install any program. There is no JavaScript or WebAssembly engine inside Fanimate, on purpose: an embedded engine would add a large dependency and a security surface of its own, and a separate process keeps a plugin's crash on its own side. A true sandbox (WebAssembly with no system access) is a possible later host for the same call table.

## Commands

```
fanimate-cli plugin new "Title Maker" [--lang py|js|ts] [-o DIR]    a starter plugin
fanimate-cli plugin check <folder|plugin.json>                      what it asks for, or what is wrong
fanimate-cli plugin test <folder|name> [project.fnm]                run it on a copy and say what it did; never saves
fanimate-cli plugin run <folder|name> [project.fnm] [--save out.fnm]
fanimate-cli plugin pack <folder> [-o name.fnplugin]
fanimate-cli plugin install <name.fnplugin> [--folder DIR]
fanimate-cli plugin list [--folder DIR]
```

`run` and `test` take a folder, a `plugin.json`, or the name of an installed plugin. They exit with 1 when the plugin fails or is stopped.

Installed plugins live in the `plugins` folder beside Fanimate's settings (`%APPDATA%\Fanimate\data\plugins` on Windows, `~/Library/Application Support/Fanimate/plugins` on macOS, `~/.local/share/fanimate/plugins` on Linux), or in `$FANIMATE_PLUGIN_DIR`. `--folder` adds one more for `list` and `run`, or names the target of `install`.

## Packages

`plugin pack` writes a `.fnplugin` file: a ZIP archive of the plugin's folder, `plugin.json` first, leaving out files whose names start with a dot. It checks the manifest and the entry first, so a package always installs. At most 2000 files and 64 MB.

`plugin install` treats the package as untrusted. It refuses a file whose name is absolute, climbs out with `..` or holds a drive letter, a name listed twice, a damaged entry (every checksum is verified), too many files or too much data, a package with no `plugin.json`, and a manifest that does not pass `plugin check` or whose entry is missing. It unpacks into a hidden folder and moves it into place only when everything passed, so a refused or failed install leaves what was installed before exactly as it was. Installing a plugin with the same name replaces it and says which version it replaced. **Nothing in a package is run by installing it.**

## Not built yet

- Running plugins from the app, with a menu and the prompt that grants a plugin its permissions (the grant is the user's choice, shown with what each permission means).
- A plugin gallery or any online source: packages are files you pass around.
- Signing of packages.
