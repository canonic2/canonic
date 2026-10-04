import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { mkdir, realpath } from 'node:fs/promises';
import path from 'node:path';

const exec = promisify(execFile);
export class GitWorkspaces {
  async run(cwd: string, args: string[]) {
    return (await exec('git', ['-C', cwd, ...args])).stdout.trim();
  }
  async repository(folder: string) {
    return realpath(await this.run(folder, ['rev-parse', '--show-toplevel']));
  }
  async branch(folder: string) {
    return (await this.run(folder, ['branch', '--show-current'])) || 'detached';
  }
  async create(repository: string, checkout: string, branch: string, base: string) {
    const commit = await this.run(repository, [
      'rev-parse',
      '--verify',
      '--end-of-options',
      `${base}^{commit}`,
    ]);
    await mkdir(path.dirname(checkout), { recursive: true });
    await this.run(repository, ['worktree', 'add', '-b', branch, checkout, commit]);
  }
  async rollback(repository: string, checkout: string, branch: string) {
    await this.run(repository, ['worktree', 'remove', checkout]);
    await this.run(repository, ['branch', '-D', branch]);
  }
}
