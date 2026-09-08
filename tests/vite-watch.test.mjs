import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, mkdir, writeFile, rm, realpath } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';
import { setTimeout as delay } from 'node:timers/promises';
import { createServer } from 'vite';

test('Vite watches frontend edits without polling or watching generated trees', { timeout: 10000 }, async () => {
  const root = await realpath(await mkdtemp(path.join(tmpdir(), 'memivy-vite-watch-')));
  let server;
  try {
    await writeFile(path.join(root, 'index.html'), '<script type="module" src="/main.js"></script>');
    await writeFile(path.join(root, 'main.js'), 'export const value = "before";');
    for (const folder of ['target', 'src-tauri', 'research']) {
      await mkdir(path.join(root, folder));
      await writeFile(path.join(root, folder, 'generated.txt'), 'generated');
    }
    server = await createServer({
      root,
      configFile: fileURLToPath(new URL('../vite.config.ts', import.meta.url)),
      // Exercise the macOS fallback even on installations that have fsevents.
      server: { middlewareMode: true, watch: { useFsEvents: false } },
      logLevel: 'silent',
    });
    await once(server.watcher, 'ready');
    assert.equal(server.watcher.options.usePolling, false);
    const watched = server.watcher.getWatched();
    for (const folder of ['target', 'src-tauri', 'research']) {
      assert.equal(watched[path.join(root, folder)], undefined);
    }
    assert.match((await server.transformRequest('/main.js')).code, /before/);
    const changed = once(server.watcher, 'change', { signal: AbortSignal.timeout(3000) });
    await writeFile(path.join(root, 'main.js'), 'export const value = "after";');
    assert.equal((await changed)[0], path.join(root, 'main.js'));
    // Vite's own change handler must invalidate the previously transformed module.
    let code;
    for (let attempt = 0; attempt < 30; attempt++) {
      code = (await server.transformRequest('/main.js')).code;
      if (code.includes('after')) break;
      await delay(50);
    }
    assert.match(code, /after/);
  } finally {
    await server?.close();
    await rm(root, { recursive: true, force: true });
  }
});
