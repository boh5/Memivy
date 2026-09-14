import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, copyFileSync, chmodSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

function fixture(t, {npm='0.1.0', lock='0.1.0', speech='0.1.0'}={}) {
  const root=mkdtempSync(path.join(tmpdir(),'memivy-release-metadata-'));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  const put=(p,s)=>{mkdirSync(path.dirname(path.join(root,p)),{recursive:true});writeFileSync(path.join(root,p),s);};
  for(const name of ['check-release.mjs','rust-env.mjs']) {put(`scripts/${name}`,'');copyFileSync(`scripts/${name}`,path.join(root,'scripts',name));}
  put('package.json',JSON.stringify({version:npm}));
  put('package-lock.json',JSON.stringify({version:lock,packages:{'':{version:lock}}}));
  put('src-tauri/tauri.conf.json',JSON.stringify({version:'0.1.0'}));
  put('CHANGELOG.md','## 0.1.0\n');
  put('tools/cargo',`#!${process.execPath}\nconsole.log(${JSON.stringify(JSON.stringify({workspace_members:['app','speech'],packages:[{id:'app',name:'memivy',version:'0.1.0',license:'MIT'},{id:'speech',name:'memivy-speech',version:speech,license:'MIT'}]}))});`);
  chmodSync(path.join(root,'tools/cargo'),0o755);
  return tag=>spawnSync(process.execPath,['scripts/check-release.mjs',...(tag?[tag]:[])],{cwd:root,encoding:'utf8',env:{...process.env,PATH:path.join(root,'tools')+path.delimiter+process.env.PATH}});
}
test('release accepts matching stable metadata and tag',t=>{const r=fixture(t)('v0.1.0');assert.equal(r.status,0,r.stderr);});
test('release rejects a mismatched tag',t=>{const r=fixture(t)('v0.2.0');assert.notEqual(r.status,0);assert.match(r.stderr,/Expected tag/);});
test('release rejects a stale npm lockfile',t=>{const r=fixture(t,{lock:'0.0.9'})();assert.notEqual(r.status,0);assert.match(r.stderr,/versions differ/);});
test('release rejects a mismatched speech sidecar',t=>{const r=fixture(t,{speech:'0.2.0'})();assert.notEqual(r.status,0);assert.match(r.stderr,/memivy-speech/);});
test('stable release refuses prerelease version',t=>{const r=fixture(t,{npm:'0.1.0-beta.1'})();assert.notEqual(r.status,0);assert.match(r.stderr,/stable/);});
