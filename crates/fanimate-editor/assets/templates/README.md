# Template artwork

Everything the built-in templates are made from. The documents are assembled in
`crates/fanimate-editor/src/templates/` when a template opens; nothing here is a project file.

| Folder | What it is | Made by |
|---|---|---|
| `mouths/classic`, `flat`, `ink`, `line`, `cute` | Five styles of mouth shapes, nine SVG files each, named after Rhubarb Lip Sync's shapes (A to H and X) | `scripts/template-art/mouths.mjs` (original artwork) |
| `mouths/frontal` | Fourteen photo-real frontal mouth shapes (PNG), the set supplied for the football scenes: A is `M`, B is `S`, C is `Uh`, D is `Ah`, E is `Oh`, F is `W-Oo`, G is `F`, H is `L`, X is `Neutral`; `Ee`, `D` (shown as "Closed teeth"), `R`, `Smile` and `Surprised` are for keying by hand | Resized to 40 % from the supplied "Standard Frontal Mouth shapes" |
| `characters` | Eight cartoon characters, 800 x 1000, one SVG group per bone-bound part | `scripts/template-art/characters.mjs` (original artwork, nobody real) |
| `scenes` | The four sets, a back and a front file each | `scripts/template-art/scenes.mjs` (original artwork) |
| `speech` | The dialogue of each scene as a 16 kHz mono WAV, and its lines with mouth cues (Rhubarb's letters) | `scripts/template-speech/gen-speech.mjs`: Windows' Microsoft David and Zira voices, mouth timing from the voice's viseme events |

Rebuild: `node scripts/template-art/mouths.mjs`, `characters.mjs`, `scenes.mjs`, then
`node scripts/template-speech/gen-speech.mjs` (Windows only). `scripts/template-art/sheet.mjs` and
`scenes-sheet.mjs` write HTML contact sheets for checking the art by eye.

Before a release: confirm the licence of the `mouths/frontal` set (it was supplied without one) and
that Microsoft's terms allow shipping the generated speech.
