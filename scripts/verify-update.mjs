import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const [archive, config = 'src-tauri/tauri.release.conf.json'] = process.argv.slice(2);
if (!archive) throw new Error('Usage: verify-update.mjs ARCHIVE [TAURI_CONFIG]');

const publicKey = JSON.parse(readFileSync(config, 'utf8')).plugins.updater.pubkey;
const signature = readFileSync(`${archive}.sig`, 'utf8');
const directory = mkdtempSync(path.join(tmpdir(), 'memivy-update-signature-'));
try {
  // Tauri wraps complete Minisign key/signature files in base64.
  const keyFile = path.join(directory, 'public.key');
  const signatureFile = path.join(directory, 'archive.minisig');
  writeFileSync(keyFile, Buffer.from(publicKey.trim(), 'base64'));
  writeFileSync(signatureFile, Buffer.from(signature.trim(), 'base64'));
  const result = spawnSync('minisign', [
    '-V', '-m', path.resolve(archive), '-p', keyFile, '-x', signatureFile,
  ], { stdio: 'inherit' });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error('Updater archive signature verification failed.');
} finally {
  rmSync(directory, { recursive: true, force: true });
}
