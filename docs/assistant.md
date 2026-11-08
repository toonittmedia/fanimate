# The Assistant

*New in the development version (0.5); not in a release yet.*

The Assistant is a panel beside the canvas where you ask for an edit or an animation in plain words ("fade the title in over 12 frames"). A model turns the request into calls of the [scripting API](scripting.md), and Fanimate runs them on the project you have open. It is optional, off until you set it up, and one switch hides every AI feature.

## What it does and does not do

- **It works through the API, not around it.** Every call is a transaction, so it is one undo step, all or nothing, and it is refused while the animation plays (the Assistant pauses playback for you before it edits). The tools it can call are the call table of the API, generated from `fanimate_api::rpc::METHODS`: a call added to the API is available to it without more work.
- **It cannot save, close or quit.** It gets the `read` and `edit` permissions a script can be given, never `save`. You save when you are happy.
- **Everything it creates is labelled.** Each layer it makes carries a record saying a tool made it (`assistant`, with the model's name, Fanimate's version and the date), inside the same undo step as the layer. A studio can tell what was generated. Layers you drew carry nothing.
- **You see what it changed.** Each request ends in a card listing the changes in plain words, with **Undo** (takes back everything in the card as one step, while nothing else has been done since) and **Show** (selects the layers and goes to the frame).

## Starting

Open it with **Ctrl+K** (Ask Assistant in the Edit menu), the **Ask** button at the right of the title bar, or Window > Assistant. The first time, the panel shows **Get started**:

1. **Provider.** Anthropic (Claude), OpenRouter (one key reaches many models), or any service or local server that speaks the OpenAI chat API (OpenAI, Ollama, LM Studio, a studio's own gateway).
2. **Model.** The name the provider uses; a few suggestions are listed and any name can be typed.
3. **Key.** Paste it and choose Save. Keys are kept in `ai-keys.json` in Fanimate's data folder, readable by your account only (mode 0600 where the system has modes), never in a project, in the settings saved with the layout, in a log or on a command line. An environment variable (`ANTHROPIC_API_KEY`, `OPENROUTER_API_KEY` or `OPENAI_API_KEY`) always wins and nothing is saved then. An address on this computer (such as Ollama's) needs no key. Using the operating system's credential store instead is planned.
4. **Allow sending.** Fanimate asks once for each destination before the first message is sent there, and says what a message contains.

## What a message sends

Your words, the project's name, size, frame rate and frame range, the playhead frame, the names and ids of the selected layers, and whatever the model then asks to look at through the read tools (the layer list, a layer's keys and so on). The line under the message box ("Context: Title, frame 12") says what a message is about. Nothing is sent until you press Send, and no project file is uploaded. Fanimate itself sends nothing else and has no telemetry. What a provider does with what it receives is governed by that provider's own terms.

## Working offline, and turning it off

- **Work offline** (Edit > Preferences) stops the Assistant reaching any service that is not on this computer. A local model still works.
- **Turn off all AI features** (in the panel's settings) hides the panel, Ctrl+K, the Ask button and the link for other programs.
- **Policy files** (`fanimate-policy.json` beside the program, `policy.json` in the user folder; see [install.md](install.md)) can set `ai` to false or `offline` to true for a whole computer. The Assistant starts only if the policy allows it.

## Using your own agent (Claude Code, Codex, Antigravity)

Fanimate can be driven by an agent you already use, through the Model Context Protocol. Turn on **Let other programs control Fanimate** in the Assistant's settings, then run one command once in a terminal (the panel has a Copy button for each, with the path to `fanimate-cli` filled in):

```
claude mcp add --scope user fanimate -- fanimate-cli mcp --app
codex mcp add fanimate -- fanimate-cli mcp --app
agy mcp add fanimate fanimate-cli mcp --app
```

The agent then lists Fanimate's tools and works on the project open in the window: you watch it happen, and Ctrl+Z works. It has the same limits as the Assistant (it can change the project but not save or close it, and what it creates is labelled `external-agent`).

`fanimate-cli mcp project.fnm` serves the same tools on a project file with no window, saving after each change, for an agent running on a server. `--permissions read` lets it look and not touch.

The link is a socket on `127.0.0.1` only, protected by a random token written to `host.json` in Fanimate's data folder (the folder `FANIMATE_HOST_FILE` can override). It exists only while the switch is on.

## For developers

- `crates/fanimate-agent` holds everything that is not the window: the tools (a JSON Schema worked out from each method's parameter list), the MCP server, the Anthropic and OpenAI-style adapters (requests are sent by the system's `curl`, with the key on its standard input), the agent loop and the plain-words descriptions of changes. It has no UI code.
- `crates/fanimate-app/src/assistant/` runs requests on the UI thread: the model's thread and the socket's clients send each request line to the frame loop, which runs it through `fanimate_api::rpc` on the live editor. Nothing edits a project from another thread.
- `Api::with_provenance` is what labels created layers; use it for any tool that is a model.
