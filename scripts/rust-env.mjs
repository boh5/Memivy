import { homedir } from 'node:os';
import path from 'node:path';

// GUI-launched terminals may not load rustup's shell profile. Preserve explicit
// PATH choices and add the standard (or user-selected) Cargo installation.
export function withRustPath(env = process.env, home = homedir()) {
  const cargoBin = path.join(env.CARGO_HOME || path.join(home, '.cargo'), 'bin');
  const entries = (env.PATH || '').split(path.delimiter).filter(Boolean);
  if (!entries.includes(cargoBin)) entries.push(cargoBin);
  return { ...env, PATH: entries.join(path.delimiter) };
}
