// Speaks the dialogue of the built-in templates with Windows' own voices and works out
// the mouth shapes from the voice's viseme events, so no recording or lip sync tool is
// needed to rebuild them:
//
//   node scripts/template-speech/gen-speech.mjs [scene ...]
//
// For each scene it writes crates/fanimate-editor/assets/templates/speech/<scene>.wav (mono,
// 16 kHz) and <scene>.json: every line with who says it, when, the words, and mouth cues
// named after Rhubarb's shapes (A to H and X), in seconds from the start of the sound.
// Needs Windows (System.Speech, the David and Zira voices that ship with it).
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, rmSync, writeFileSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, '../../crates/fanimate-editor/assets/templates/speech');
const FPS = 24;
const RATE = 16000;

const DAVID = 'Microsoft David Desktop';
const ZIRA = 'Microsoft Zira Desktop';

// who: voice, SSML pitch and rate. Characters of a scene are named in the Rust builder.
const VOICES = {
  gaffer: { voice: DAVID, pitch: '-30%', rate: 'medium' },
  striker: { voice: DAVID, pitch: '+18%', rate: 'fast' },
  manager: { voice: DAVID, pitch: '-12%', rate: 'medium' },
  reporter: { voice: ZIRA, pitch: '+6%', rate: 'fast' },
  keeper: { voice: DAVID, pitch: '-4%', rate: 'medium' },
  pundit: { voice: DAVID, pitch: '+0%', rate: 'medium' },
  presenter: { voice: ZIRA, pitch: '-4%', rate: 'medium' },
  head: { voice: ZIRA, pitch: '+0%', rate: 'medium' },
  head2: { voice: DAVID, pitch: '-10%', rate: 'medium' },
};

// [who, text, pause after in seconds]
const SCENES = {
  'dressing-room': [
    ['gaffer', 'Right lads, listen up. We are two nil down, and the whole world is laughing at us.', 0.4],
    ['striker', 'Boss, it is only the first half.', 0.3],
    ['gaffer', 'Exactly! So you have forty five minutes to stop being rubbish!', 0.5],
    ['striker', 'I will try, boss. Can I have my phone back first?', 0.2],
  ],
  'press-conference': [
    ['reporter', 'Gaffer, you lost eight nil. What went wrong?', 0.5],
    ['manager', 'Well, we created a lot of chances, and they created a lot of goals.', 0.5],
    ['reporter', 'Will you resign?', 0.4],
    ['manager', 'I will think about it after lunch.', 0.2],
  ],
  'penalty-shootout': [
    ['striker', 'I never miss penalties. I practise them every single day.', 0.4],
    ['keeper', 'Funny. I never save them. Only in my dreams.', 0.4],
    ['striker', 'Watch this, mate. Top corner.', 0.4],
    ['keeper', 'You do realise the top corner is in the stands?', 0.2],
  ],
  'pundits': [
    ['pundit', 'That tackle was a disgrace. In my day, we never fouled anybody. Well, hardly ever.', 0.4],
    ['presenter', 'Last week you called the same tackle brilliant.', 0.4],
    ['pundit', 'That was before I saw the replay.', 0.2],
  ],
  'talking-head': [
    ['head', 'Hello! I am your new talking head. Give me a voice, and I will do the rest.', 0.2],
  ],
  'mouth-styles': [
    ['head', 'Five mouth styles, one voice. Pick the one you like best.', 0.2],
  ],
};

// SAPI's 21 visemes mapped to Rhubarb's mouth shapes
const SHAPE = [
  'X', // 0 silence
  'C', // 1 ae ax ah
  'D', // 2 aa
  'E', // 3 ao
  'C', // 4 ey eh uh
  'E', // 5 er
  'B', // 6 y iy ih ix
  'F', // 7 w uw
  'E', // 8 ow
  'D', // 9 aw
  'E', // 10 oy
  'D', // 11 ay
  'C', // 12 h
  'B', // 13 r
  'H', // 14 l
  'B', // 15 s z
  'B', // 16 sh ch jh zh
  'B', // 17 th dh
  'G', // 18 f v
  'B', // 19 d t n
  'B', // 20 k g ng
  'A', // 21 p b m
];

function speak(who, text, dir, n) {
  const v = VOICES[who];
  const wav = join(dir, `${n}.wav`);
  const json = join(dir, `${n}.json`);
  execFileSync('powershell', [
    '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', join(HERE, 'speak.ps1'),
    '-Text', text, '-Voice', v.voice, '-Wav', wav, '-Json', json, '-Rate', v.rate, '-Pitch', v.pitch,
  ], { stdio: ['ignore', 'ignore', 'inherit'] });
  return { pcm: pcm(readFileSync(wav)), events: JSON.parse(readFileSync(json, 'utf8')) };
}

/** The samples of a WAV file's data chunk. */
function pcm(buf) {
  let at = 12;
  while (at + 8 <= buf.length) {
    const id = buf.toString('ascii', at, at + 4);
    const size = buf.readUInt32LE(at + 4);
    if (id === 'data') return buf.subarray(at + 8, Math.min(buf.length, at + 8 + size));
    at += 8 + size + (size & 1);
  }
  throw new Error('no data chunk in the WAV');
}

function wavFile(samples) {
  const head = Buffer.alloc(44);
  head.write('RIFF', 0);
  head.writeUInt32LE(36 + samples.length, 4);
  head.write('WAVEfmt ', 8);
  head.writeUInt32LE(16, 16);
  head.writeUInt16LE(1, 20); // PCM
  head.writeUInt16LE(1, 22); // mono
  head.writeUInt32LE(RATE, 24);
  head.writeUInt32LE(RATE * 2, 28);
  head.writeUInt16LE(2, 32);
  head.writeUInt16LE(16, 34);
  head.write('data', 36);
  head.writeUInt32LE(samples.length, 40);
  return Buffer.concat([head, samples]);
}

const silence = (seconds) => Buffer.alloc(Math.round(seconds * RATE) * 2);
const seconds = (buf) => buf.length / 2 / RATE;

/**
 * Mouth cues for one line: the viseme in force on each frame, short flickers merged
 * into the shape before them (a closed lips shape, A, is always kept: a P or B you
 * cannot see looks wrong), and the whole thing a frame early, as animators lead the sound.
 */
function cues(events, length) {
  const frames = Math.ceil(length * FPS) + 1;
  const perFrame = [];
  let e = 0;
  let shape = 'X';
  for (let f = 0; f < frames; f++) {
    const t = f / FPS + 0.5 / FPS;
    // of events at one instant (a diphthong's two halves) the first counts
    while (e < events.length && events[e].t <= t) {
      if (e === 0 || events[e].t !== events[e - 1].t) shape = SHAPE[events[e].v];
      e++;
    }
    perFrame.push(shape);
  }
  // runs of one frame fold into the previous run, except closed lips and the teeth on the lip
  const runs = [];
  for (const [f, s] of perFrame.entries()) {
    const last = runs[runs.length - 1];
    if (last && last.s === s) last.n++;
    else runs.push({ f, s, n: 1 });
  }
  const kept = runs.filter((r, i) => i === 0 || r.n > 1 || r.s === 'A' || r.s === 'G');
  const out = [];
  for (const r of kept) {
    if (out.length && out[out.length - 1].value === r.s) continue;
    out.push({ start: Math.max(0, (r.f - 1) / FPS), value: r.s });
  }
  return out;
}

mkdirSync(OUT, { recursive: true });
const wanted = process.argv.slice(2);
for (const [name, lines] of Object.entries(SCENES)) {
  if (wanted.length && !wanted.includes(name)) continue;
  const dir = mkdtempSync(join(tmpdir(), 'fanimate-speech-'));
  try {
    const parts = [silence(0.6)];
    let t = 0.6;
    const done = [];
    for (const [i, [who, text, pause]] of lines.entries()) {
      const { pcm: samples, events } = speak(who, text, dir, i);
      const length = seconds(samples);
      done.push({
        who,
        text,
        start: Number(t.toFixed(3)),
        end: Number((t + length).toFixed(3)),
        cues: cues(events, length).map((c) => ({ start: Number(c.start.toFixed(3)), value: c.value })),
      });
      parts.push(samples, silence(pause));
      t += length + pause;
    }
    parts.push(silence(1.0));
    const sound = Buffer.concat(parts);
    writeFileSync(join(OUT, `${name}.wav`), wavFile(sound));
    // one line of JSON per spoken line, so the file stays readable and diffs small
    const body = done.map((l) => '  ' + JSON.stringify(l)).join(',\n');
    const head = `{ "seconds": ${seconds(sound).toFixed(3)}, "lines": [\n`;
    writeFileSync(join(OUT, `${name}.json`), `${head}${body}\n] }\n`);
    console.log(`${name}: ${seconds(sound).toFixed(1)} s, ${done.length} lines`);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}
