// The sets of the four football scenes as SVG: a back file (behind the characters) and a
// front file (a desk, a table, a ball: in front of them). 1920 x 1080, flat colours.
//
//   node scripts/template-art/scenes.mjs
//
// writes crates/fanimate-editor/assets/templates/scenes/<scene>-back.svg and -front.svg.
// The cast stands where `crates/fanimate-editor/src/templates/football.rs` puts it, so the
// two have to agree: the desks here meet the characters' chests there.
import { fileURLToPath } from 'node:url';
import { join, dirname } from 'node:path';
import { write, path, group, svg, ellipseD, rectD } from './lib.mjs';

const OUT = join(dirname(fileURLToPath(import.meta.url)), '../../crates/fanimate-editor/assets/templates/scenes');
const W = 1920;
const H = 1080;

const rect = (id, x, y, w, h, fill, rad = 0, extra = {}) => path(id, rectD(x, y, w, h, rad), { fill, ...extra });
const oval = (id, x, y, rx, ry, fill, extra = {}) => path(id, ellipseD(x, y, rx, ry), { fill, ...extra });
const line = (id, x1, y1, x2, y2, stroke, sw) => path(id, `M${x1} ${y1}L${x2} ${y2}`, { stroke, sw });
const poly = (id, pts, fill) => path(id, `M${pts.map(([x, y]) => `${x} ${y}`).join('L')}Z`, { fill });

// small deterministic noise, so the crowds look the same every time
let seed = 7;
const rnd = () => ((seed = (seed * 16807) % 2147483647) / 2147483647);

function dressingRoom() {
  const lockers = [];
  const colours = ['#3e6fa3', '#4a7fb6'];
  for (let i = 0; i < 6; i++) {
    const x = 40 + i * 215;
    lockers.push(rect(`locker ${i + 1}`, x, 90, 195, 640, colours[i % 2], 10));
    lockers.push(rect(`locker door ${i + 1}`, x + 12, 104, 171, 612, i % 2 ? '#5a90c6' : '#4f83b8', 6));
    for (let v = 0; v < 4; v++) lockers.push(rect('vent', x + 40, 140 + v * 18, 115, 7, '#2f567f', 3));
    lockers.push(rect('handle', x + 150, 380, 12, 70, '#d8dde6', 6));
  }
  const board = [
    rect('board frame', 1190, 150, 640, 400, '#d9c9a6', 14),
    rect('board', 1210, 170, 600, 360, '#2f8a55', 8),
    path('board lines', 'M1510 170V530M1210 350H1250M1770 350H1810', { stroke: '#e8f5ec', sw: 4 }),
    path('board centre', ellipseD(1510, 350, 60, 60), { stroke: '#e8f5ec', sw: 4 }),
  ];
  for (const [x, y] of [[1340, 260], [1400, 400], [1620, 300], [1690, 430], [1450, 220]]) {
    board.push(path('o', ellipseD(x, y, 16, 16), { stroke: '#ffffff', sw: 5 }));
  }
  for (const [x, y] of [[1560, 250], [1360, 450], [1640, 400]]) {
    board.push(path('x', `M${x - 14} ${y - 14}L${x + 14} ${y + 14}M${x + 14} ${y - 14}L${x - 14} ${y + 14}`, { stroke: '#ff7a7a', sw: 6 }));
  }
  board.push(path('arrow', 'M1360 270C1420 330 1500 330 1560 260', { stroke: '#ffe27a', sw: 5 }));
  return {
    back: [
      group('Wall', rect('wall', 0, 0, W, 780, '#c7d4d2'), rect('wall band', 0, 640, W, 140, '#b3c3c0')),
      group('Floor', rect('floor', 0, 780, W, 300, '#a57a52'),
        [0, 1, 2, 3, 4, 5, 6, 7].map((i) => line('board', 0, 800 + i * 40, W, 800 + i * 40, '#946b45', 4))),
      group('Lockers', lockers),
      group('Tactics board', board),
      group('Bench', rect('bench seat', 60, 700, 1100, 34, '#7b5a3c', 8), rect('leg', 120, 730, 26, 80, '#5d442d'), rect('leg', 560, 730, 26, 80, '#5d442d'), rect('leg', 1060, 730, 26, 80, '#5d442d')),
      group('Light', rect('light', 560, 0, 800, 30, '#fffbe8', 10)),
    ],
    front: [
      group('Kit bag', rect('bag', 70, 930, 300, 110, '#d63a3a', 40), rect('bag stripe', 70, 972, 300, 20, '#ffffff'), path('strap', 'M130 930C150 880 290 880 310 930', { stroke: '#8b2323', sw: 10 })),
      group('Boots', oval('boot', 1560, 1030, 70, 26, '#22202a'), oval('boot', 1700, 1040, 70, 26, '#22202a')),
    ],
  };
}

function pressConference() {
  const wall = [rect('backdrop', 0, 0, W, H, '#1c2f5e')];
  for (let row = 0; row < 7; row++) {
    for (let col = 0; col < 12; col++) {
      const x = 80 + col * 170 + (row % 2) * 85;
      wall.push(oval('mark', x, 110 + row * 130, 28, 28, row % 2 ? '#2c4a8f' : '#27427f'));
      wall.push(poly('chevron', [[x - 12, 100 + row * 130], [x, 112 + row * 130], [x + 12, 100 + row * 130], [x + 12, 108 + row * 130], [x, 120 + row * 130], [x - 12, 108 + row * 130]], '#ffffff'));
    }
  }
  const table = [
    rect('cloth', 90, 800, 1260, 300, '#f2f3f7', 0),
    rect('cloth fold', 90, 800, 1260, 18, '#d9dce6'),
    rect('table top', 60, 780, 1320, 34, '#9aa3b8', 8),
    path('pleats', 'M300 818V1080M520 818V1080M740 818V1080M960 818V1080M1180 818V1080', { stroke: '#e1e4ee', sw: 6 }),
    rect('placard', 600, 880, 300, 90, '#2f4a8a', 10),
    oval('placard dot', 750, 925, 20, 20, '#ffffff'),
  ];
  const props = [];
  for (const x of [470, 900]) {
    props.push(rect('mic stand', x - 4, 700, 8, 80, '#1b1b22'));
    props.push(oval('mic', x, 692, 22, 26, '#3b3f4d'));
  }
  props.push(rect('jug', 1100, 700, 54, 84, '#bfe3f2', 10), rect('glass', 1200, 726, 34, 56, '#d5eef7', 6));
  return {
    back: [group('Backdrop', wall)],
    front: [group('Table', table), group('Props', props), group('Flash', rect('flash', 0, 0, W, H, '#ffffff'))],
  };
}

function penalty() {
  const sky = [rect('sky', 0, 0, W, 560, '#8fd0f0'), rect('sky low', 0, 330, W, 230, '#b6e3f7')];
  for (const [x, y, w] of [[260, 120, 150], [900, 80, 190], [1540, 140, 160]]) {
    sky.push(oval('cloud', x, y, w, 36, '#ffffff'), oval('cloud', x + 60, y - 22, w * 0.6, 34, '#ffffff'));
  }
  const crowd = [rect('stand', 0, 230, W, 330, '#4c4a62'), rect('stand roof', 0, 190, W, 46, '#2f2d40')];
  const shirts = ['#d63a3a', '#f2d22f', '#ffffff', '#3b6fd0', '#2fa35b', '#e88b2f'];
  for (let row = 0; row < 7; row++) {
    for (let col = 0; col < 48; col++) {
      const x = 20 + col * 40 + (row % 2) * 20;
      const y = 270 + row * 40;
      crowd.push(oval('fan', x, y, 15, 15, ['#f1c89e', '#d49a6a', '#8a5a3a'][Math.floor(rnd() * 3)]));
      crowd.push(rect('fan shirt', x - 14, y + 14, 28, 20, shirts[Math.floor(rnd() * shirts.length)], 8));
    }
  }
  const pitch = [rect('grass', 0, 560, W, 520, '#3f9a4d')];
  for (let i = 0; i < 8; i++) {
    // mowing stripes fan out from the far touchline
    const a = 960 + (i - 4) * 240, b = a + 240;
    pitch.push(poly('mow', [[a, 560], [b, 560], [3 * b - 1920, 1080], [3 * a - 1920, 1080]], i % 2 ? '#47a655' : '#3b9249'));
  }
  const lines = [
    path('box', 'M200 670H1720L1900 1070H20Z', { stroke: '#ffffff', sw: 6 }),
    path('six yard', 'M560 670H1360L1440 790H480Z', { stroke: '#ffffff', sw: 6 }),
    path('goal line', 'M0 670H1920', { stroke: '#ffffff', sw: 6 }),
    oval('spot', 960, 930, 11, 7, '#ffffff'),
  ];
  const goal = [];
  const gx = 480, gy = 240, gw = 960, gh = 430;
  goal.push(rect('net back', gx, gy, gw, gh, '#e9eef0', 0));
  for (let i = 0; i <= 48; i++) goal.push(line('net', gx + (gw / 48) * i, gy, gx + (gw / 48) * i, gy + gh, '#aeb8bc', 2));
  for (let j = 0; j <= 20; j++) goal.push(line('net', gx, gy + (gh / 20) * j, gx + gw, gy + (gh / 20) * j, '#aeb8bc', 2));
  goal.push(rect('post A', gx - 8, gy - 8, 16, gh + 16, '#ffffff', 6), rect('post B', gx + gw - 8, gy - 8, 16, gh + 16, '#ffffff', 6), rect('bar', gx - 8, gy - 8, gw + 16, 16, '#ffffff', 6));
  return {
    back: [group('Sky', sky), group('Stand', crowd), group('Pitch', pitch), group('Lines', lines), group('Goal', goal)],
    front: [group('Ball', oval('ball', 760, 1010, 34, 34, '#ffffff', { stroke: '#2a2230', sw: 4 }), poly('patch', [[760, 990], [778, 1004], [771, 1026], [749, 1026], [742, 1004]], '#2a2230'))],
  };
}

function pundits() {
  const wall = [rect('wall', 0, 0, W, H, '#241a3b')];
  for (let i = 0; i < 9; i++) wall.push(rect('panel', 40 + i * 210, 0, 190, 780, i % 2 ? '#2c2148' : '#271d42'));
  const screen = [
    rect('screen frame', 340, 110, 1240, 520, '#0e0a1a', 24),
    rect('screen', 364, 134, 1192, 472, '#2c8c52', 12),
    path('screen lines', 'M960 134V606M364 370H470V300H364M1556 370H1450V300H1556', { stroke: '#e8f5ec', sw: 5 }),
    path('screen centre', ellipseD(960, 370, 90, 90), { stroke: '#e8f5ec', sw: 5 }),
    path('arrow', 'M600 500C700 380 800 380 900 300', { stroke: '#ffe27a', sw: 8 }),
    path('arrow B', 'M1300 260C1250 360 1150 420 1050 440', { stroke: '#ff7a7a', sw: 8 }),
  ];
  for (const [x, y] of [[700, 420], [1100, 250], [1350, 480]]) screen.push(oval('player', x, y, 20, 20, '#ffffff'));
  const lights = [];
  for (let i = 0; i < 7; i++) lights.push(rect('light', 120 + i * 260, 24, 150, 18, '#ffe9a8', 9));
  const chairs = [
    rect('chair A', 440, 520, 380, 420, '#3b2f5c', 70),
    rect('chair B', 1090, 520, 380, 420, '#3b2f5c', 70),
  ];
  const desk = [
    rect('desk front', 220, 820, 1480, 300, '#4a2f8a', 0),
    rect('desk panel', 280, 850, 1360, 200, '#3a2370', 24),
    oval('desk logo', 960, 950, 70, 70, '#ffffff'),
    oval('desk logo inner', 960, 950, 44, 44, '#7a3fc4'),
    rect('desk top', 180, 790, 1560, 40, '#d9d3ee', 14),
  ];
  const props = [rect('papers', 520, 760, 120, 30, '#ffffff', 4), rect('pen', 1280, 770, 80, 8, '#1b1b22', 4), rect('mug', 1500, 730, 56, 60, '#f2f3f7', 10)];
  return {
    back: [group('Wall', wall), group('Lights', lights), group('Screen', screen), group('Chairs', chairs)],
    front: [group('Desk', desk), group('Desk props', props)],
  };
}

const SCENES = { 'dressing-room': dressingRoom, 'press-conference': pressConference, 'penalty-shootout': penalty, pundits };

for (const [name, build] of Object.entries(SCENES)) {
  const { back, front } = build();
  write(join(OUT, `${name}-back.svg`), svg(W, H, back));
  write(join(OUT, `${name}-front.svg`), svg(W, H, front));
}
console.log(`wrote ${Object.keys(SCENES).length} sets to ${OUT}`);
