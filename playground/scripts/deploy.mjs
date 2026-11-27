// Publishes the playground and the guide to a GitHub Pages repository, in a folder of their own
// (other projects live next to it):
//
//   npm run deploy                 build everything, then publish to RB14/RB14.github.io/redsat/
//   npm run deploy -- --no-build   publish what dist/ already holds (plus a fresh guide)
//   npm run deploy -- --dry-run    prepare the commit in the clone, but do not push
//
// Settings (environment): REDSAT_PAGES_REPO (owner/name), REDSAT_PAGES_FOLDER (the folder in it),
// REDSAT_PAGES_CLONE (where the repository is cloned; a cache directory by default).
// The site is served at https://<owner>.github.io/<folder>/ and the guide under /guide/.

import { execFileSync } from 'node:child_process';
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../..', import.meta.url));
const playground = join(root, 'playground');
const repo = process.env.REDSAT_PAGES_REPO ?? 'RB14/RB14.github.io';
const folder = process.env.REDSAT_PAGES_FOLDER ?? 'redsat';
const clone = process.env.REDSAT_PAGES_CLONE ?? join(process.env.XDG_CACHE_HOME ?? join(homedir(), '.cache'), 'redsat-deploy', repo.replace('/', '-'));
const args = new Set(process.argv.slice(2));
const env = { ...process.env, PATH: [join(homedir(), '.cargo', 'bin'), process.env.PATH].join(':') };

function run(command, commandArgs, cwd = root) {
  console.log(`$ ${command} ${commandArgs.join(' ')}`);
  return execFileSync(command, commandArgs, { cwd, env, stdio: ['ignore', 'pipe', 'inherit'] }).toString().trim();
}

// 1. Build the playground (compiler, solvers, page) and the guide into it.
if (!args.has('--no-build')) execFileSync('npm', ['run', 'build'], { cwd: playground, env, stdio: 'inherit' });
const dist = join(playground, 'dist');
if (!existsSync(join(dist, 'index.html'))) throw new Error('dist/ is empty: run without --no-build');
run('mdbook', ['build', 'docs/guide', '--dest-dir', join(dist, 'guide')]);

// 2. Bring the Pages repository up to date.
if (existsSync(join(clone, '.git'))) {
  run('git', ['fetch', '--quiet', 'origin'], clone);
  run('git', ['reset', '--quiet', '--hard', 'origin/HEAD'], clone);
} else {
  mkdirSync(clone, { recursive: true });
  run('gh', ['repo', 'clone', repo, clone, '--', '--quiet']);
}

// 3. Replace our folder (and only it) with the new build.
const target = join(clone, folder);
rmSync(target, { recursive: true, force: true });
cpSync(dist, target, { recursive: true, filter: (path) => !path.endsWith('.map') });
const [owner] = repo.split('/');
const url = `https://${owner}.github.io/${folder}/`;
const readme = join(clone, 'README.md');
if (existsSync(readme)) {
  const text = readFileSync(readme, 'utf8');
  if (!text.includes(url)) {
    const entry = `- [RedSAT Playground](${url}) and [the RedSAT Guide](${url}guide/)`;
    writeFileSync(readme, `${text.trimEnd()}\n${entry}\n`);
  }
}

// 4. Commit and push, naming the RedSAT commit that was built.
const sha = run('git', ['rev-parse', '--short', 'HEAD']);
const dirty = run('git', ['status', '--porcelain']) ? '-dirty' : '';
run('git', ['add', '--all', folder, 'README.md'], clone);
if (!run('git', ['status', '--porcelain'], clone)) {
  console.log('Nothing changed: the site is up to date.');
} else {
  run('git', ['commit', '--quiet', '-m', `chore: deploy redsat ${sha}${dirty}`], clone);
  if (args.has('--dry-run')) console.log(`Dry run: the commit is ready in ${clone}, not pushed.`);
  else run('git', ['push', '--quiet', 'origin', 'HEAD'], clone);
}
console.log(`Playground: ${url}\nGuide: ${url}guide/`);
