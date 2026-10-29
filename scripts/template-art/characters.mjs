// The cartoon cast of the football scenes, as layered SVG: big heads, small bodies, flat
// colours. Everything is original: no real player, club or sponsor is drawn.
//
//   node scripts/template-art/characters.mjs
//
// writes crates/fanimate-editor/assets/templates/characters/<name>.svg (800 x 1000, feet at
// y 960, facing front). Each top-level group is one layer, back to front, named so the
// scene builder can bind it to a bone (see `crates/fanimate-editor/src/templates/cast.rs`):
//   legs, body, upper arm B, forearm B, upper arm A, forearm A,
//   head back, face, eyes open, eyes closed, head front
// The mouth is not drawn: the builder puts a switch layer of mouth shapes at MOUTH, between
// the eyes and `head front`. Arm A is on the viewer's left.
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';
import { write, path, group, svg, ellipseD, rectD } from './lib.mjs';

const OUT = join(dirname(fileURLToPath(import.meta.url)), '../../crates/fanimate-editor/assets/templates/characters');

/** Where the mouth goes and how wide it is, for every character. */
export const MOUTH = { x: 400, y: 418, width: 150 };

const SKIN = {
  pale: ['#f6cfae', '#e3a984'],
  tan: ['#e0a26f', '#c07f4c'],
  brown: ['#a8683f', '#8a4f2b'],
  ruddy: ['#e79a82', '#c8705c'],
  dark: ['#6f4429', '#563219'],
};

const INK = '#2a2230';

/** A polygon from points, with rounded corners made by a stroke of the same colour. */
const poly = (id, pts, fill, round = 0) =>
  path(id, `M${pts.map(([x, y]) => `${x} ${y}`).join('L')}Z`, round ? { fill, stroke: fill, sw: round } : { fill });

function head(o) {
  const [skin, shade] = SKIN[o.skin];
  const jaw = o.jaw ?? 1; // 1 is round, above is squarer and wider
  const w = o.headW ?? 162;
  const top = 140, bot = 472, mid = 300;
  const face =
    `M400 ${top}C${400 + w * 0.56} ${top} ${400 + w} ${top + 62} ${400 + w} ${mid}` +
    `C${400 + w} ${mid + 100 * jaw} ${400 + w * 0.58 * jaw} ${bot} 400 ${bot}` +
    `C${400 - w * 0.58 * jaw} ${bot} ${400 - w} ${mid + 100 * jaw} ${400 - w} ${mid}` +
    `C${400 - w} ${top + 62} ${400 - w * 0.56} ${top} 400 ${top}Z`;
  const back = [];
  const front = [];
  const hairC = o.hair;

  // hair that sits behind the head
  if (o.hairStyle === 'long') {
    back.push(path('hair back', rectD(222, 170, 356, 360, 120), { fill: hairC }));
  }
  if (o.hairStyle === 'bob') {
    back.push(path('hair back', rectD(228, 170, 344, 250, 110), { fill: hairC }));
  }
  if (o.hairStyle === 'curly') {
    for (const [x, y, r] of [[250, 220, 62], [320, 165, 66], [400, 150, 70], [480, 165, 66], [550, 220, 62], [232, 300, 50], [568, 300, 50]]) {
      back.push(path('curl', ellipseD(x, y, r, r), { fill: hairC }));
    }
  }
  // ears
  back.push(path('ear A', ellipseD(238, 322, 30, 44), { fill: skin }));
  back.push(path('ear B', ellipseD(562, 322, 30, 44), { fill: skin }));
  back.push(path('inner ear A', ellipseD(240, 326, 14, 24), { fill: shade }));
  back.push(path('inner ear B', ellipseD(560, 326, 14, 24), { fill: shade }));

  const faceLayer = [
    path('head', face, { fill: skin }),
    path('cheek A', ellipseD(300, 380, 38, 24), { fill: o.blush ?? shade }),
    path('cheek B', ellipseD(500, 380, 38, 24), { fill: o.blush ?? shade }),
    path('nose', ellipseD(400, 352, o.nose ?? 30, (o.nose ?? 30) * 0.9), { fill: shade }),
    path('nose tip', ellipseD(400, 346, (o.nose ?? 30) * 0.8, (o.nose ?? 30) * 0.7), { fill: skin }),
  ];
  if (o.stubble) {
    faceLayer.push(path('stubble', `M265 410C290 480 510 480 535 410C520 462 470 474 400 474C330 474 280 462 265 410Z`, { fill: o.stubble }));
  }

  const eyeR = o.eyeR ?? 38;
  const eyes = (open) => {
    const out = [];
    for (const [side, x] of [['A', 332], ['B', 468]]) {
      if (open) {
        out.push(path(`eye ${side}`, ellipseD(x, 288, eyeR, eyeR * 1.08), { fill: '#ffffff', stroke: INK, sw: 4 }));
        const px = x + (side === 'A' ? 4 : -4) + (o.cross ? (side === 'A' ? 10 : -10) : 0);
        out.push(path(`pupil ${side}`, ellipseD(px, 294, eyeR * 0.36, eyeR * 0.4), { fill: INK }));
        out.push(path(`glint ${side}`, ellipseD(px - 5, 286, 4.5, 5), { fill: '#ffffff' }));
      } else {
        out.push(path(`blink ${side}`, `M${x - eyeR} 292Q${x} ${292 + eyeR * 0.55} ${x + eyeR} 292`, { stroke: INK, sw: 7 }));
      }
    }
    return out;
  };

  // eyebrows tilt: positive is angry (inner ends down)
  const tilt = o.brow ?? 0;
  const bw = o.browW ?? 9;
  for (const [side, x, dir] of [['A', 332, 1], ['B', 468, -1]]) {
    const inner = 232 + tilt * 14;
    const outer = 232 - tilt * 6;
    front.push(path(`brow ${side}`,
      `M${x - dir * 46} ${outer}Q${x} ${inner - 20} ${x + dir * 46} ${inner + 8}`, { stroke: o.browColour ?? o.hair, sw: bw }));
  }
  if (o.moustache) {
    front.push(path('moustache',
      `M400 372C380 358 340 360 322 376C340 392 372 388 400 382C428 388 460 392 478 376C460 360 420 358 400 372Z`, { fill: o.moustache }));
  }
  if (o.glasses) {
    for (const x of [332, 468]) front.push(path('glasses', ellipseD(x, 288, eyeR + 14, eyeR + 12), { stroke: INK, sw: 6 }));
    front.push(path('bridge', 'M368 280Q400 268 432 280', { stroke: INK, sw: 6 }));
  }
  // hair in front of the head
  const hair = [];
  switch (o.hairStyle) {
    case 'quiff':
      hair.push(path('hair', `M240 280C226 170 330 118 420 128C500 100 588 168 560 280C556 220 520 196 470 196C420 190 330 196 290 220C262 236 246 258 240 280Z`, { fill: hairC }));
      hair.push(path('quiff', `M300 190C320 90 470 60 540 130C490 110 420 120 380 160Z`, { fill: hairC }));
      break;
    case 'slick':
      hair.push(path('hair', `M240 285C220 175 320 124 400 124C490 124 588 176 560 285C552 232 500 190 400 190C300 190 248 232 240 285Z`, { fill: hairC }));
      break;
    case 'sides':
      hair.push(path('hair A', `M240 300C222 240 240 196 276 186C262 230 260 270 270 330Z`, { fill: hairC }));
      hair.push(path('hair B', `M560 300C578 240 560 196 524 186C538 230 540 270 530 330Z`, { fill: hairC }));
      hair.push(path('wisps', `M340 150C380 126 430 126 470 150C430 140 380 140 340 150Z`, { fill: hairC, stroke: hairC, sw: 5 }));
      break;
    case 'thin':
      hair.push(path('hair A', `M244 300C226 230 250 190 290 176C270 220 266 260 276 320Z`, { fill: hairC }));
      hair.push(path('hair B', `M556 300C574 230 550 190 510 176C530 220 534 260 524 320Z`, { fill: hairC }));
      break;
    case 'bob':
      hair.push(path('hair', `M236 330C206 170 330 112 400 112C480 112 604 170 564 330C560 250 520 190 400 190C300 190 244 250 236 330Z`, { fill: hairC }));
      hair.push(path('fringe', `M268 220C330 170 460 170 532 220C500 196 440 190 400 192C350 192 300 200 268 220Z`, { fill: hairC, stroke: hairC, sw: 14 }));
      break;
    case 'long':
      hair.push(path('hair', `M232 330C196 160 330 108 400 108C490 108 612 160 568 330C556 240 520 186 400 186C290 186 244 240 232 330Z`, { fill: hairC }));
      hair.push(path('part', `M400 120C380 150 330 180 290 200C340 190 380 176 400 150C420 176 470 190 510 200C470 180 420 150 400 120Z`, { fill: hairC, stroke: hairC, sw: 6 }));
      break;
    case 'curly':
      for (const [x, y, r] of [[290, 190, 44], [350, 160, 46], [410, 150, 48], [470, 160, 46], [528, 190, 44]]) {
        hair.push(path('curl', ellipseD(x, y, r, r), { fill: hairC }));
      }
      break;
    case 'bald':
      hair.push(path('shine', `M330 170C360 150 400 146 430 152C400 154 360 160 330 170Z`, { fill: shade, stroke: shade, sw: 6 }));
      break;
  }
  if (o.cap) {
    hair.push(path('cap', `M236 232C226 140 330 106 400 106C480 106 580 142 564 232Z`, { fill: o.cap }));
    hair.push(path('peak', `M236 236C300 222 500 222 564 236C560 252 470 246 400 246C330 246 240 252 236 236Z`, { fill: o.capShade ?? INK }));
  }
  return {
    'head back': back,
    face: faceLayer,
    'eyes open': eyes(true),
    'eyes closed': eyes(false),
    'head front': [...hair, ...front],
  };
}

/** The shoulders, chest and hips: x edges at a given y. */
const torsoX = (y) => {
  // shoulders (y 500) 252..548, waist (y 745) 290..510
  const t = Math.min(1, Math.max(0, (y - 500) / 245));
  return [252 + 38 * t, 548 - 38 * t];
};

function body(o) {
  const [skin, shade] = SKIN[o.skin];
  const items = [path('neck', rectD(364, 452, 72, 80, 20), { fill: shade })];
  const top = 500, bot = 745;
  const [lt, rt] = torsoX(top);
  const [lb, rb] = torsoX(bot);
  items.push(poly('torso', [[lt, top], [rt, top], [rb, bot], [lb, bot]], o.shirt, 36));
  if (o.stripes) {
    // hoops follow the sides of the torso
    const n = o.stripes.count ?? 5;
    const h = (bot - top) / (n * 2);
    for (let i = 0; i < n; i++) {
      const y1 = top + h * (2 * i + 1) + 6, y2 = y1 + h - 6;
      const [a, b] = torsoX(y1), [c, e] = torsoX(y2);
      items.push(poly('hoop', [[a, y1], [b, y1], [e, y2], [c, y2]], o.stripes.colour));
    }
  }
  if (o.vest) {
    items.push(poly('v neck', [[360, 500], [440, 500], [400, 590]], skin));
  }
  if (o.collar) {
    items.push(poly('collar A', [[360, 500], [404, 500], [400, 570], [340, 530]], o.collar, 6));
    items.push(poly('collar B', [[440, 500], [396, 500], [400, 570], [460, 530]], o.collar, 6));
  }
  if (o.shirtFront) {
    items.push(poly('shirt', [[366, 500], [434, 500], [400, 640]], o.shirtFront));
  }
  if (o.tie) {
    items.push(poly('knot', [[388, 520], [412, 520], [408, 548], [392, 548]], o.tie));
    items.push(poly('tie', [[392, 548], [408, 548], [418, 660], [400, 690], [382, 660]], o.tie, 4));
  }
  if (o.zip) {
    items.push(path('zip', 'M400 505L400 745', { stroke: o.zip, sw: 6 }));
  }
  if (o.badge) {
    items.push(path('badge', ellipseD(458, 566, 20, 20), { fill: o.badge }));
  }
  if (o.belt) {
    items.push(path('belt', rectD(lb - 4, bot - 22, rb - lb + 8, 28, 6), { fill: o.belt }));
  }
  return items;
}

function legs(o) {
  const items = [];
  if (o.legs === 'kit') {
    const [skin] = SKIN[o.skin];
    for (const [side, x] of [['A', 348], ['B', 452]]) {
      items.push(path(`leg ${side}`, rectD(x - 30, 740, 60, 210, 28), { fill: skin }));
      items.push(path(`sock ${side}`, rectD(x - 33, 850, 66, 100, 26), { fill: o.socks }));
      items.push(path(`boot ${side}`, ellipseD(x + (side === 'A' ? -8 : 8), 950, 52, 22), { fill: INK }));
    }
    items.push(poly('shorts', [[290, 725], [510, 725], [528, 840], [412, 840], [400, 800], [388, 840], [272, 840]], o.shorts, 22));
  } else {
    for (const [side, x] of [['A', 348], ['B', 452]]) {
      items.push(path(`leg ${side}`, rectD(x - 36, 730, 72, 220, 20), { fill: o.trousers }));
      items.push(path(`shoe ${side}`, ellipseD(x + (side === 'A' ? -10 : 10), 950, 56, 22), { fill: INK }));
    }
  }
  return items;
}

/** One arm: sleeve and glove or skin. The shoulder is at (sx, 520). */
function arm(o, side) {
  const [skin] = SKIN[o.skin];
  const sx = side === 'A' ? 262 : 538;
  const sleeveBottom = o.longSleeves ? 805 : 640;
  const upper = [path(`sleeve ${side}`, rectD(sx - 34, 492, 68, 190, 32), { fill: o.sleeve ?? o.shirt })];
  const fore = [];
  if (o.longSleeves) {
    fore.push(path(`cuff ${side}`, rectD(sx - 29, 668, 58, 130, 26), { fill: o.sleeve ?? o.shirt }));
  } else {
    fore.push(path(`forearm skin ${side}`, rectD(sx - 28, 668, 56, 130, 26), { fill: skin }));
  }
  const handColour = o.gloves ?? skin;
  fore.push(path(`hand ${side}`, ellipseD(sx, 826, o.gloves ? 44 : 36, o.gloves ? 44 : 36), { fill: handColour }));
  if (o.gloves) fore.push(path(`glove band ${side}`, rectD(sx - 30, 792, 60, 16, 6), { fill: o.gloveBand ?? '#ffffff' }));
  if (o.microphone && side === 'B') {
    fore.push(path('mic handle', rectD(sx - 10, 760, 20, 80, 8), { fill: '#2b2b33' }));
    fore.push(path('mic head', ellipseD(sx, 748, 24, 28), { fill: '#7d8794' }));
    fore.push(path('mic band', rectD(sx - 22, 750, 44, 8, 3), { fill: '#d44b4b' }));
  }
  void sleeveBottom;
  return { upper, fore };
}

export function character(o) {
  const h = head(o);
  const a = arm(o, 'A');
  const b = arm(o, 'B');
  return svg(800, 1000,
    group('legs', legs(o)),
    group('body', body(o)),
    group('upper arm B', b.upper),
    group('forearm B', b.fore),
    group('upper arm A', a.upper),
    group('forearm A', a.fore),
    group('head back', h['head back']),
    group('face', h.face),
    group('eyes open', h['eyes open']),
    group('eyes closed', h['eyes closed']),
    group('head front', h['head front']),
  );
}

/** The cast. Shirts are kits of made-up clubs. */
export const CAST = {
  // an angry, round manager in a navy tracksuit
  gaffer: {
    skin: 'ruddy', jaw: 1.25, headW: 176, nose: 44, hair: '#9aa0aa', hairStyle: 'sides', moustache: '#8a8f99', brow: 1.6, browW: 13,
    shirt: '#1d2b53', sleeve: '#1d2b53', longSleeves: true, zip: '#e4e7ee', collar: '#2f4180', legs: 'trousers', trousers: '#1d2b53',
  },
  // a lanky striker in red and white hoops
  striker: {
    skin: 'pale', jaw: 0.9, headW: 150, nose: 24, hair: '#e8b53a', hairStyle: 'quiff', brow: -0.6, eyeR: 36, blush: '#f0a79a',
    shirt: '#d63a3a', stripes: { colour: '#ffffff', count: 5 }, legs: 'kit', shorts: '#ffffff', socks: '#d63a3a', collar: '#ffffff',
  },
  // a manager in a grey suit and glasses
  manager: {
    skin: 'tan', jaw: 1.1, headW: 160, nose: 34, hair: '#6e6a66', hairStyle: 'thin', glasses: true, brow: 0.3,
    shirt: '#4f5666', sleeve: '#4f5666', longSleeves: true, shirtFront: '#f3f3f3', tie: '#2c8f6b', legs: 'trousers', trousers: '#3d4452',
  },
  // a reporter holding a microphone
  reporter: {
    skin: 'brown', jaw: 0.9, headW: 150, nose: 26, hair: '#2a1b14', hairStyle: 'bob', brow: -0.3, blush: '#c47f5a',
    shirt: '#1fa3a3', sleeve: '#1fa3a3', longSleeves: true, shirtFront: '#ffffff', collar: '#168585', microphone: true, legs: 'trousers', trousers: '#33384a',
  },
  // a keeper with a big jaw, in green with yellow gloves
  keeper: {
    skin: 'tan', jaw: 1.35, headW: 168, nose: 32, hair: '#1c1a22', hairStyle: 'slick', stubble: '#7a5a43', brow: 1.0, browW: 12,
    shirt: '#2fa35b', gloves: '#f2d22f', gloveBand: '#ffffff', legs: 'kit', shorts: '#1c1a22', socks: '#2fa35b', collar: '#1f7d43',
  },
  // an old pundit with a big nose
  pundit: {
    skin: 'ruddy', jaw: 1.0, headW: 158, nose: 46, hair: '#d9dbe0', hairStyle: 'sides', brow: -0.4, browColour: '#c6c9d0', browW: 12,
    shirt: '#232a4a', sleeve: '#232a4a', longSleeves: true, shirtFront: '#f3f3f3', tie: '#c93a3a', legs: 'trousers', trousers: '#232a4a',
  },
  // a presenter
  presenter: {
    skin: 'pale', jaw: 0.85, headW: 148, nose: 24, hair: '#d8a13a', hairStyle: 'long', brow: -0.3, blush: '#f0a79a',
    shirt: '#7a3fc4', sleeve: '#7a3fc4', longSleeves: true, shirtFront: '#fff3ea', collar: '#5c2c98', legs: 'trousers', trousers: '#2e2a3d',
  },
  // for the basic templates: a friendly face and shoulders in a green jumper
  buddy: {
    skin: 'tan', jaw: 1.0, headW: 160, nose: 28, hair: '#3a2a22', hairStyle: 'curly', brow: -0.5, blush: '#d9876a',
    shirt: '#3b9d7a', sleeve: '#3b9d7a', longSleeves: true, collar: '#2c7a5e', legs: 'trousers', trousers: '#2e2a3d',
  },
};

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  for (const [name, o] of Object.entries(CAST)) write(join(OUT, `${name}.svg`), character(o));
  console.log(`wrote ${Object.keys(CAST).length} characters to ${OUT}`);
}
