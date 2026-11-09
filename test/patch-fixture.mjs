import { access, copyFile, mkdir, mkdtemp, readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { dirname, resolve, join } from 'node:path';
import { videoRoot } from './source.mjs';

export async function patchFixture(patchName, relativeFile) {
  const testRoot = process.env.RELAY_TEST_ROOT || '/Volumes/KH-STUDIO-ATLAS/KH-Worktrees';
  await access(testRoot);
  const parent = join(testRoot, 'relay-studio-kh');
  await mkdir(parent).catch((error) => { if (error.code !== 'EEXIST') throw error; });
  const isolated = await mkdtemp(join(parent, `${patchName.replace(/\.patch$/, '')}-`));
  const target = join(isolated, relativeFile);
  await mkdir(dirname(target), { recursive: true });
  await copyFile(resolve(videoRoot, relativeFile), target);
  const patch = resolve(import.meta.dirname, '../patches', patchName);
  execFileSync('git', ['apply', '--check', patch], { cwd: isolated });
  execFileSync('git', ['apply', patch], { cwd: isolated });
  console.info(`Isolated patched source retained: ${isolated}`);
  return { sourceText: await readFile(target, 'utf8'), isolated };
}
