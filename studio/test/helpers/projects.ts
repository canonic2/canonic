import { mkdir, readFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import type { Studio } from '../../src/application/studio.ts';

const exec = promisify(execFile);
const app = `import http from 'node:http';
import { readFile, writeFile } from 'node:fs/promises';
const sessionId = process.env.STUDIO_SESSION_ID;
let visits = 0;
try { visits = Number(await readFile('.visits', 'utf8')); } catch {}
const server = http.createServer(async (req, res) => {
  if (req.url === '/health') {
    res.setHeader('Content-Type', 'application/json');
    return res.end(JSON.stringify({ sessionId, port: server.address().port }));
  }
  visits++;
  await writeFile('.visits', String(visits));
  const title = (await readFile('title.txt', 'utf8')).trim();
  const escape = value => String(value).replace(/[&<>"']/g, char => ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[char]));
  res.setHeader('Content-Type', 'text/html; charset=utf-8');
  res.end('<!doctype html><title>' + escape(title) + '</title><body style="background:#141619;color:#e8e8ec;font:18px system-ui;padding:70px"><p style="color:#a9b1bd">CANONIC STUDIO · LIVE SESSION</p><h1>' + escape(title) + '</h1><p>Edit title.txt in the IDE and refresh this page.</p><p>Session: ' + escape(sessionId) + '</p><p>Port: ' + server.address().port + ' · Visits in this checkout: ' + visits + '</p></body>');
  console.log(new Date().toISOString(), req.method, req.url, 'visits=' + visits);
});
server.listen(Number(process.env.PORT), '127.0.0.1', () => console.log('READY', sessionId, server.address().port));
process.on('SIGTERM', () => server.close(() => process.exit(0)));
`;

export async function seedDemo(manager: Studio) {
  if (manager.snapshot().projects.length) return;
  for (const [name, slug, task] of [
    ['Acme Store', 'acme-store', 'Cart redesign'],
    ['Marketing Site', 'marketing-site', null],
  ] as const) {
    const directory = path.join(manager.root, 'demo', slug);
    await mkdir(directory, { recursive: true });
    // Rerunning after an interrupted seed keeps an existing repository intact.
    try {
      await readFile(path.join(directory, '.git/HEAD'));
    } catch {
      await exec('git', ['init', '-b', 'main', directory]);
      await writeFile(path.join(directory, 'server.ts'), app);
      await writeFile(path.join(directory, 'title.txt'), `${name}\n`);
      await writeFile(path.join(directory, '.gitignore'), '.visits\nnode_modules/\n');
      await writeFile(
        path.join(directory, 'studio.config.json'),
        JSON.stringify(
          { version: 1, runtime: { command: ['$NODE', 'server.ts'], readyPath: '/health' } },
          null,
          2,
        ),
      );
      await writeFile(
        path.join(directory, 'README.md'),
        `# ${name}\n\nThis is Studio's local demo project.\n\nEdit title.txt, then open the service preview. Each session owns its own files, terminal, IDE settings, and visit counter.\n\nOpen a terminal to run git status or inspect STUDIO_SESSION_ID. Use the Extensions view to install an Open VSX extension, or Preferences: Color Theme to choose a theme.\n`,
      );
      await exec('git', ['-C', directory, 'add', '.']);
      await exec('git', [
        '-C',
        directory,
        '-c',
        'user.name=Acme',
        '-c',
        'user.email=dev@example.com',
        '-c',
        'commit.gpgsign=false',
        'commit',
        '-m',
        'Initialize Studio demo',
      ]);
    }
    const project = await manager.addProject(directory, name);
    if (task) {
      const session = await manager.createSession(project.id, task);
      await writeFile(path.join(session.checkout, 'title.txt'), 'Acme Store — Cart redesign\n');
      await exec('git', ['-C', session.checkout, 'add', 'title.txt']);
      await exec('git', [
        '-C',
        session.checkout,
        '-c',
        'user.name=Acme',
        '-c',
        'user.email=dev@example.com',
        '-c',
        'commit.gpgsign=false',
        'commit',
        '-m',
        'Start cart redesign',
      ]);
    }
  }
  const first = manager.snapshot().projects[0];
  await manager.select(first.id, first.activeSessionId);
}
