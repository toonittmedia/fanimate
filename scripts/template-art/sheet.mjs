// A contact sheet of every mouth style, for checking the artwork by eye:
//   node scripts/template-art/sheet.mjs out.html   (then open it, or screenshot it with Chrome)
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '../../crates/fanimate-editor/assets/templates/mouths');
const letters = ['A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'X'];
let html = '<body style="margin:0;background:#f1c89e;font:12px sans-serif">';
for (const style of readdirSync(root, { withFileTypes: true }).filter((e) => e.isDirectory() && e.name !== 'frontal')) {
  html += `<div style="display:flex;align-items:center"><b style="width:60px">${style.name}</b>`;
  for (const l of letters) {
    const svg = readFileSync(join(root, style.name, `${l}.svg`), 'utf8');
    html += `<div style="width:140px;height:105px">${svg.replace('<svg ', '<svg style="width:140px;height:105px" ')}</div>`;
  }
  html += '</div>';
}
writeFileSync(process.argv[2] ?? 'sheet.html', html);
