// Cut a Fanimate release: check, bump the version, commit, tag and push.
//
// CI does not run on every push. Pushing a `v*` tag is what starts the
// release workflow (.github/workflows/release.yml), which builds and tests on
// Windows, macOS and Linux and publishes a GitHub release. Because nothing
// checks ordinary pushes, this script runs the same checks locally first.

import { execSync, spawnSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';

const ROOT = path.resolve(import.meta.dirname, '..');
const CARGO_TOML = path.join(ROOT, 'Cargo.toml');

const USAGE = `
Usage: pnpm release [patch|minor|major|<x.y.z>] [options]

  patch, -p        Bump the patch version (x.y.Z) [default]
  minor, -n        Bump the minor version (x.Y.0)
  major, -m        Bump the major version (X.0.0)
  <x.y.z>          Release exactly this version

Options:
  --dry-run, -d    Run the checks and show what would happen; change nothing
  --skip-checks    Skip fmt, clippy, tests and the brand check (not recommended)
  --help, -h       Show this help
`;

function parseArgs() {
  let bump = 'patch';
  let exact = null;
  let dryRun = false;
  let skipChecks = false;
  for (const arg of process.argv.slice(2)) {
    const a = arg.toLowerCase();
    if (a === 'major' || a === '--major' || a === '-m') bump = 'major';
    else if (a === 'minor' || a === '--minor' || a === '-n') bump = 'minor';
    else if (a === 'patch' || a === '--patch' || a === '-p') bump = 'patch';
    else if (/^v?\d+\.\d+\.\d+$/.test(a)) exact = a.replace(/^v/, '');
    else if (a === '--dry-run' || a === '-d') dryRun = true;
    else if (a === '--skip-checks') skipChecks = true;
    else if (a === '--help' || a === '-h' || a === 'help') {
      console.log(USAGE);
      process.exit(0);
    } else {
      fail(`unknown argument "${arg}"\n${USAGE}`);
    }
  }
  return { bump, exact, dryRun, skipChecks };
}

function fail(message) {
  console.error(`\nrelease: ${message}`);
  process.exit(1);
}

function git(args) {
  return execSync(`git ${args}`, { cwd: ROOT, encoding: 'utf8' }).trim();
}

/** Run a command with live output; stop the release if it fails. */
function step(label, command) {
  console.log(`\n> ${label}\n  ${command}`);
  const result = spawnSync(command, { cwd: ROOT, stdio: 'inherit', shell: true });
  if (result.status !== 0) fail(`"${label}" failed; nothing was tagged or pushed.`);
}

function parseVersion(v) {
  const m = v.match(/^v?(\d+)\.(\d+)\.(\d+)$/);
  return m ? m.slice(1, 4).map(Number) : null;
}

function latestTag() {
  const tags = git('tag --sort=-v:refname').split(/\r?\n/).filter(Boolean);
  return tags.find((t) => parseVersion(t)) ?? null;
}

function nextVersion(current, bump) {
  let [major, minor, patch] = current;
  if (bump === 'major') return [major + 1, 0, 0];
  if (bump === 'minor') return [major, minor + 1, 0];
  return [major, minor, patch + 1];
}

/** The `version` under `[workspace.package]` in Cargo.toml. */
const WORKSPACE_VERSION = /(\[workspace\.package\][^[]*?\nversion\s*=\s*")([^"]+)(")/;

function readCargoVersion() {
  const m = readFileSync(CARGO_TOML, 'utf8').match(WORKSPACE_VERSION);
  if (!m) fail('could not find version under [workspace.package] in Cargo.toml');
  return m[2];
}

function writeCargoVersion(version) {
  const text = readFileSync(CARGO_TOML, 'utf8');
  writeFileSync(CARGO_TOML, text.replace(WORKSPACE_VERSION, `$1${version}$3`));
}

function pythonCommand() {
  for (const cmd of ['python', 'python3']) {
    const r = spawnSync(`${cmd} --version`, { stdio: 'ignore', shell: true });
    if (r.status === 0) return cmd;
  }
  return null;
}

function preflight() {
  const branch = git('branch --show-current');
  if (branch !== 'main') fail(`releases are cut from main; you are on "${branch}".`);

  // not git(): trimming would eat the status column of the first line
  const dirty = execSync('git status --porcelain', { cwd: ROOT, encoding: 'utf8' }).trimEnd();
  if (dirty) fail(`the working tree has uncommitted changes:\n${dirty}\nCommit or stash them first.`);

  let behind = 0;
  try {
    git('fetch --quiet origin main --tags');
    behind = Number(git('rev-list --count HEAD..origin/main'));
  } catch {
    console.warn('Warning: could not reach origin to check for newer commits.');
  }
  if (behind > 0) fail(`main is ${behind} commit(s) behind origin/main; pull first.`);
}

function checks() {
  step('Format', 'cargo fmt --all --check');
  step('Clippy', 'cargo clippy --workspace --all-targets -- -D warnings');
  step('Tests', 'cargo test --workspace');
  const py = pythonCommand();
  if (py) {
    step('Brand tokens', `${py} brand/scripts/build_brand.py --check`);
    step('Licences', `${py} scripts/licences.py --check`);
    step('Models', `${py} scripts/models.py --check`);
  } else {
    console.warn('\nWarning: Python not found; skipping the brand token and licence checks (CI runs them).');
  }
}

function run() {
  const { bump, exact, dryRun, skipChecks } = parseArgs();
  preflight();

  const tag = latestTag();
  const base = tag ? parseVersion(tag) : [0, 0, 0];
  console.log(tag ? `Latest release: ${tag}` : 'No release tags yet; starting from 0.0.0.');

  const version = exact ?? nextVersion(base, bump).join('.');
  const newTag = `v${version}`;
  if (git(`tag --list ${newTag}`)) fail(`tag ${newTag} already exists.`);
  if (tag && exact) {
    const [a, b] = [parseVersion(version), base];
    const newer = a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
    if (newer <= 0) fail(`${newTag} is not newer than ${tag}.`);
  }
  console.log(`Releasing ${newTag}${dryRun ? ' (dry run)' : ''}`);

  if (skipChecks) console.warn('\nWarning: skipping checks.');
  else checks();

  const cargoVersion = readCargoVersion();
  const needsBump = cargoVersion !== version;
  const plan = [
    needsBump && `Set version ${cargoVersion} -> ${version} in Cargo.toml and Cargo.lock`,
    needsBump && `git commit -m "release ${newTag}"`,
    `git tag -a ${newTag} -m "Fanimate ${newTag}"`,
    `git push --atomic origin main ${newTag}`,
  ].filter(Boolean);

  if (dryRun) {
    console.log('\n[Dry run] Checks passed. Would now:');
    plan.forEach((p, i) => console.log(`  ${i + 1}. ${p}`));
    return;
  }

  if (needsBump) {
    console.log(`\nSetting version to ${version}...`);
    writeCargoVersion(version);
    // refresh the workspace crates' entries in Cargo.lock only
    step('Update Cargo.lock', 'cargo update --workspace --offline');
    step('Commit', `git add Cargo.toml Cargo.lock && git commit -m "release ${newTag}"`);
  }
  step('Tag', `git tag -a ${newTag} -m "Fanimate ${newTag}"`);
  step('Push', `git push --atomic origin main ${newTag}`);

  console.log(`\nPushed ${newTag}. The release workflow is building it now:`);
  console.log('  https://github.com/toonittmedia/fanimate/actions/workflows/release.yml');
  console.log('When it finishes, the release is published on GitHub with the notes in docs/releases/ (if any).');
}

run();
