// A contact sheet of the four sets (back and front together, without the camera flash), for
// checking the artwork by eye:
//   node scripts/template-art/scenes-sheet.mjs out.html
import { readFileSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '../../crates/fanimate-editor/assets/templates/scenes');
const fit = (svg) => svg.replace('<svg ', '<svg style="position:absolute;left:0;top:0;width:640px;height:360px" ');
let html = '<body style="margin:0;background:#888;display:flex;flex-wrap:wrap">';
for (const name of ['dressing-room', 'press-conference', 'penalty-shootout', 'pundits']) {
  const back = readFileSync(join(root, `${name}-back.svg`), 'utf8');
  const front = readFileSync(join(root, `${name}-front.svg`), 'utf8').replace(/<g id="Flash">[\s\S]*?<\/g>/, '');
  html += `<div style="position:relative;flex:none;overflow:hidden;width:640px;height:360px">${fit(back)}${fit(front)}</div>`;
}
writeFileSync(process.argv[2] ?? 'scenes.html', html);
