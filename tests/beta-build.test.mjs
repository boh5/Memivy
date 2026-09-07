import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, copyFileSync, rmSync, chmodSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

function fixture(t) {
  const root=mkdtempSync(path.join(tmpdir(),'memivy-build-review-'));
  t.after(()=>rmSync(root,{recursive:true,force:true}));
  const put=(name,text)=>{const file=path.join(root,name);mkdirSync(path.dirname(file),{recursive:true});writeFileSync(file,text);return file;};
  for(const name of ['prepare-mcp.mjs','build-beta.mjs']) {
    mkdirSync(path.join(root,'scripts'),{recursive:true});copyFileSync(`scripts/${name}`,path.join(root,'scripts',name));
  }
  put('src-tauri/tauri.conf.json',JSON.stringify({version:'0.1.0'}));
  put('target/release/memivy-mcp','STALE MCP');
  put('target/release/bundle/macos/Memivy.app/build-marker','STALE APP');
  const target=path.join(root,'custom-cargo-target');
  const command=(name,body)=>{
    const f=put(`tools/${name}`,`#!${process.execPath}\n${body}`);
    // Executable fixtures stand in for external build tools, not product logic.
    chmodSync(f,0o755);
  };
  const env={...process.env,CARGO_TARGET_DIR:target,PATH:path.join(root,'tools')+path.delimiter+process.env.PATH};
  return {root,put,target,command,run(name){return spawnSync(process.execPath,[path.join(root,'scripts',name)],{cwd:root,env,encoding:'utf8'});}};
}
const supported=process.platform==='darwin'&&process.arch==='arm64';
test('MCP staging uses the artifact emitted by Cargo with a custom target directory', {skip:!supported}, async t=>{
  const f=fixture(t);
  await f.command('cargo',`const fs=require('fs'),p=require('path');const executable=p.join(process.env.CARGO_TARGET_DIR,'release/memivy-mcp');fs.mkdirSync(p.dirname(executable),{recursive:true});fs.writeFileSync(executable,'FRESH MCP');console.log(JSON.stringify({reason:'compiler-artifact',target:{name:'memivy-mcp',kind:['bin']},executable}));`);
  await f.command('lipo','process.exit(0);');
  const result=f.run('prepare-mcp.mjs');assert.equal(result.status,0,result.stderr);
  assert.equal(readFileSync(path.join(f.root,'src-tauri/binaries/memivy-mcp-aarch64-apple-darwin'),'utf8'),'FRESH MCP');
});
test('DMG packaging uses the just-built app rather than a stale default target bundle', {skip:!supported}, async t=>{
  const f=fixture(t);
  await f.command('cargo',`console.log(JSON.stringify({target_directory:process.env.CARGO_TARGET_DIR,packages:[{name:'memivy-phase1',version:'0.1.0'},{name:'memivy-mcp',version:'0.1.0'}]}));`);
  await f.command('npm',`const fs=require('fs'),p=require('path');const args=process.argv.slice(2),i=args.indexOf('--target');const dir=p.join(process.env.CARGO_TARGET_DIR,...(i>=0?[args[i+1]]:[]),'release/bundle/macos/Memivy.app');fs.mkdirSync(dir,{recursive:true});fs.writeFileSync(p.join(dir,'build-marker'),'FRESH APP');`);
  await f.command('hdiutil',`const fs=require('fs'),p=require('path');const a=process.argv.slice(2),stage=a[a.indexOf('-srcfolder')+1];fs.writeFileSync(a.at(-1),fs.readFileSync(p.join(stage,'Memivy.app/build-marker')));`);
  const result=f.run('build-beta.mjs');assert.equal(result.status,0,result.stderr);
  assert.equal(readFileSync(path.join(f.root,'target/release/bundle/dmg/Memivy_0.1.0_aarch64.dmg'),'utf8'),'FRESH APP');
});
test('a mixed app and MCP version is rejected before running the native build', {skip:!supported}, async t=>{
  const f=fixture(t);
  await f.command('cargo',`console.log(JSON.stringify({target_directory:process.env.CARGO_TARGET_DIR,packages:[{name:'memivy-phase1',version:'0.1.0'},{name:'memivy-mcp',version:'0.2.0'}]}));`);
  await f.command('npm',`require('fs').writeFileSync('native-build-started','yes');`);
  const result=f.run('build-beta.mjs');assert.notEqual(result.status,0);
  assert.match(result.stderr,/versions differ/);
  assert(!existsSync(path.join(f.root,'native-build-started')));
});
test('a successful Cargo exit without an executable cannot reuse a stale MCP', {skip:!supported}, async t=>{
  const f=fixture(t);
  await f.command('cargo',`console.log(JSON.stringify({reason:'build-finished',success:true}));`);
  const result=f.run('prepare-mcp.mjs');assert.notEqual(result.status,0);
  assert(!existsSync(path.join(f.root,'src-tauri/binaries/memivy-mcp-aarch64-apple-darwin')));
});
