import {readFileSync, writeFileSync, copyFileSync, mkdirSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
export function updateManifest({version,notes,signature}) {
  if(!/^\d+\.\d+\.\d+$/.test(version)) throw Error('A stable release version is required.');
  // Tauri emits a base64-encoded minisign signature file; the complete text is required.
  const decoded=Buffer.from(signature.trim(),'base64').toString('utf8');
  if(!decoded.startsWith('untrusted comment:')||!decoded.includes('trusted comment:')) throw Error('Invalid updater signature file.');
  return {version,notes,platforms:{'darwin-aarch64':{
    url:`https://github.com/boh5/memivy/releases/download/v${version}/Memivy.app.tar.gz`,signature:signature.trim(),
  }}};
}
export function prepareUpdate(bundle, destination, version, notes) {
  const name='Memivy.app.tar.gz';
  const signature=readFileSync(path.join(bundle,`${name}.sig`),'utf8');
  const manifest=updateManifest({version,notes,signature});
  mkdirSync(destination,{recursive:true});
  copyFileSync(path.join(bundle,name),path.join(destination,name));
  copyFileSync(path.join(bundle,`${name}.sig`),path.join(destination,`${name}.sig`));
  writeFileSync(path.join(destination,'latest.json'),JSON.stringify(manifest,null,2)+'\n');
}
if(process.argv[1]&&path.resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  const [bundle,destination,version,notesFile]=process.argv.slice(2);
  if(!notesFile) throw Error('Usage: prepare-update.mjs BUNDLE_DIR DESTINATION VERSION NOTES_FILE');
  prepareUpdate(bundle,destination,version,readFileSync(notesFile,'utf8'));
}
