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
  for(const name of ['prepare-mcp.mjs','build-release.mjs','rust-env.mjs']) {
    mkdirSync(path.join(root,'scripts'),{recursive:true});copyFileSync(`scripts/${name}`,path.join(root,'scripts',name));
  }
  put('scripts/check-release.mjs', '');
  put('LICENSE', 'test license');
  put('README.zh-CN.md', 'test instructions');
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
  return {root,put,target,command,run(name,args=[]){return spawnSync(process.execPath,[path.join(root,'scripts',name),...args],{cwd:root,env,encoding:'utf8'});}};
}
const supported=process.platform==='darwin'&&process.arch==='arm64';
function releaseBuildFixture(t,{entitlement='<true/>',tamper=false}={}) {
  const f=fixture(t);
  f.command('cargo',`console.log(JSON.stringify({target_directory:process.env.CARGO_TARGET_DIR,packages:[{name:'memivy',version:'0.1.0'},{name:'memivy-core',version:'0.1.0'},{name:'memivy-speech',version:'0.1.0'},{name:'memivy-mcp',version:'0.1.0'},{name:'memivy-embedding',version:'0.1.0'}]}));`);
  f.command('npm',`
    const fs=require('fs'),p=require('path'),{spawnSync}=require('child_process');
    const args=process.argv.slice(2),i=args.indexOf('--target');
    const app=p.join(process.env.CARGO_TARGET_DIR,...(i>=0?[args[i+1]]:[]),'release/bundle/macos/Memivy.app');
    const executable=p.join(app,'Contents/MacOS/Memivy');fs.mkdirSync(p.dirname(executable),{recursive:true});
    fs.mkdirSync(p.join(app,'Contents/Resources'),{recursive:true});
    fs.writeFileSync(p.join(app,'Contents/Resources/build-marker'),'FRESH APP');
    fs.writeFileSync(p.join(app,'Contents/Info.plist'),'<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.memivy.packaging.test</string><key>CFBundleExecutable</key><string>Memivy</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>');
    const compile=spawnSync('/usr/bin/clang',['-x','c','-o',executable,'-'],{input:'int main(void) { return 0; }',encoding:'utf8'});
    if(compile.status!==0)throw Error(compile.stderr);
    const signedValue=${JSON.stringify(entitlement)};
    const entitlements=p.join(process.cwd(),'test-entitlements.plist');
    if(signedValue!==null)fs.writeFileSync(entitlements,'<plist version="1.0"><dict><key>com.apple.security.device.audio-input</key>'+signedValue+'</dict></plist>');
    const signed=spawnSync('/usr/bin/codesign',['--force','--sign','-','--options','runtime',...(signedValue!==null?['--entitlements',entitlements]:[]),app],{encoding:'utf8'});
    if(signed.status!==0)throw Error(signed.stderr);
    if(${tamper})fs.appendFileSync(executable,'changed after signing');
  `);
  f.command('hdiutil',`const fs=require('fs'),p=require('path');fs.writeFileSync('dmg-packaging-started','yes');const a=process.argv.slice(2),stage=a[a.indexOf('-srcfolder')+1];fs.writeFileSync(a.at(-1),fs.readFileSync(p.join(stage,'Memivy.app/Contents/Resources/build-marker')));`);
  return f;
}
function sidecarCargo(f, {missing, wrongArch} = {}) {
  f.command('cargo', `
    const fs=require('fs'),p=require('path'),args=process.argv.slice(2);
    fs.appendFileSync('cargo-calls.jsonl',JSON.stringify(args)+'\\n');
    const names=args.flatMap((arg,i)=>arg==='-p'?[args[i+1]]:[]),target=args.indexOf('--target');
    for(const name of names) {
      if(name===${JSON.stringify(missing)})continue;
      const executable=p.join(process.env.CARGO_TARGET_DIR,...(target>=0?[args[target+1]]:[]),args.includes('--release')?'release':'debug',name);
      fs.mkdirSync(p.dirname(executable),{recursive:true});fs.writeFileSync(executable,'FRESH '+name);
      console.log(JSON.stringify({reason:'compiler-artifact',target:{name,kind:['bin']},executable}));
    }
  `);
  f.command('lipo', `process.exit(process.argv[2].endsWith(${JSON.stringify(wrongArch || 'no-invalid-binary')})?1:0);`);
}
test('release sidecars share one Cargo build and the app target with a custom directory', {skip:!supported}, t=>{
  const f=fixture(t);sidecarCargo(f);
  const result=f.run('prepare-mcp.mjs');assert.equal(result.status,0,result.stderr);
  const calls=readFileSync(path.join(f.root,'cargo-calls.jsonl'),'utf8').trim().split('\n').map(JSON.parse);
  assert.equal(calls.length,1,'sidecars must share a single dependency graph');
  assert.equal(calls[0][calls[0].indexOf('--target')+1],'aarch64-apple-darwin');
  assert(calls[0].includes('--release'));
  for(const name of ['memivy-mcp','memivy-embedding','memivy-speech']) {
    assert.equal(readFileSync(path.join(f.root,`src-tauri/binaries/${name}-aarch64-apple-darwin`),'utf8'),'FRESH '+name);
  }
});
test('development helpers are built beside the default Cargo debug app', {skip:!supported}, t=>{
  const f=fixture(t);sidecarCargo(f);
  const result=f.run('prepare-mcp.mjs',['--debug']);assert.equal(result.status,0,result.stderr);
  const args=JSON.parse(readFileSync(path.join(f.root,'cargo-calls.jsonl'),'utf8').trim());
  assert(!args.includes('--target'));assert(!args.includes('--release'));
  for(const name of ['memivy-mcp','memivy-embedding','memivy-speech']) {
    assert.equal(readFileSync(path.join(f.target,'debug',name),'utf8'),'FRESH '+name);
  }
});
test('DMG packaging uses the just-built app rather than a stale default target bundle', {skip:!supported}, async t=>{
  const f=releaseBuildFixture(t);
  const result=f.run('build-release.mjs');assert.equal(result.status,0,result.stderr);
  assert.equal(readFileSync(path.join(f.root,'target/release/bundle/dmg/Memivy_0.1.0_aarch64.dmg'),'utf8'),'FRESH APP');
});
for(const entitlement of [null,'<false/>','<string>true</string>']) {
  test(`DMG packaging rejects a real signed bundle with audio-input ${entitlement??'absent'}`,{skip:!supported},t=>{
    const f=releaseBuildFixture(t,{entitlement}),result=f.run('build-release.mjs');
    assert.notEqual(result.status,0);assert.match(result.stderr,/audio-input entitlement set to true/);
    assert(!existsSync(path.join(f.root,'dmg-packaging-started')));
    assert(!existsSync(path.join(f.root,'target/release/bundle/dmg/Memivy_0.1.0_aarch64.dmg')));
  });
}
test('DMG packaging rejects a bundle modified after signing even with audio-input enabled',{skip:!supported},t=>{
  const f=releaseBuildFixture(t,{tamper:true}),result=f.run('build-release.mjs');
  assert.notEqual(result.status,0);assert.match(result.stderr,/signature is invalid/);
  assert(!existsSync(path.join(f.root,'dmg-packaging-started')));
});
test('failed release validation stops the native build', {skip:!supported}, async t=>{
  const f=fixture(t);
  await f.command('cargo',`console.log(JSON.stringify({target_directory:process.env.CARGO_TARGET_DIR,packages:[{name:'memivy',version:'0.1.0'},{name:'memivy-core',version:'0.1.0'},{name:'memivy-speech',version:'0.1.0'},{name:'memivy-mcp',version:'0.2.0'}]}));`);
  f.put('scripts/check-release.mjs', "throw new Error('release validation failed');");
  await f.command('npm',`require('fs').writeFileSync('native-build-started','yes');`);
  const result=f.run('build-release.mjs');assert.notEqual(result.status,0);
  assert.match(result.stderr,/release validation failed/);
  assert(!existsSync(path.join(f.root,'native-build-started')));
});
test('a successful Cargo exit without an executable cannot reuse a stale MCP', {skip:!supported}, async t=>{
  const f=fixture(t);
  await f.command('cargo',`console.log(JSON.stringify({reason:'build-finished',success:true}));`);
  const result=f.run('prepare-mcp.mjs');assert.notEqual(result.status,0);
  assert(!existsSync(path.join(f.root,'src-tauri/binaries/memivy-mcp-aarch64-apple-darwin')));
});
for (const fault of [{missing:'memivy-embedding'}, {wrongArch:'memivy-speech'}]) {
  test(`invalid batch preserves staged sidecars: ${JSON.stringify(fault)}`, {skip:!supported}, t=>{
    const f=fixture(t);sidecarCargo(f,fault);
    for(const name of ['memivy-mcp','memivy-embedding','memivy-speech']) {
      f.put(`src-tauri/binaries/${name}-aarch64-apple-darwin`,'PREVIOUS '+name);
    }
    const result=f.run('prepare-mcp.mjs');assert.notEqual(result.status,0);
    for(const name of ['memivy-mcp','memivy-embedding','memivy-speech']) {
      assert.equal(readFileSync(path.join(f.root,`src-tauri/binaries/${name}-aarch64-apple-darwin`),'utf8'),'PREVIOUS '+name);
    }
  });
}
