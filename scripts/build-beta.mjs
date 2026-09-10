import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { cpSync, mkdirSync, mkdtempSync, rmSync, symlinkSync, writeFileSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { createHash } from 'node:crypto';
const root = fileURLToPath(new URL('..', import.meta.url));
if (process.platform !== 'darwin' || process.arch !== 'arm64') throw new Error('Use an Apple Silicon Mac to build this beta.');
// Ad-hoc development package: no submission to Apple or use of a host identity.
const env = { ...process.env, MACOSX_DEPLOYMENT_TARGET: '26.0', APPLE_SIGNING_IDENTITY: '-' };
for (const key of ['APPLE_ID', 'APPLE_PASSWORD', 'APPLE_TEAM_ID', 'APPLE_API_KEY', 'APPLE_API_KEY_PATH', 'APPLE_API_ISSUER']) delete env[key];
function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: 'inherit', env });
  if (result.status !== 0) throw new Error(`${command} failed (${result.status ?? 'could not start'}).`);
}
// Cargo may put outputs outside the repository. Use the same metadata and an
// explicit architecture as Tauri; a previous default-target bundle is not evidence.
const metadata = spawnSync('cargo', ['metadata', '--format-version', '1', '--no-deps', '--locked', '--offline'], { cwd: root, encoding: 'utf8', env });
if (metadata.status !== 0) throw new Error('Cannot resolve the Cargo build directory.');
const workspace = JSON.parse(metadata.stdout);
const version = JSON.parse(readFileSync(path.join(root, 'src-tauri/tauri.conf.json'), 'utf8')).version;
for (const name of ['memivy-phase1', 'memivy-mcp', 'memivy-embedding']) {
  if (workspace.packages.find(pkg => pkg.name === name)?.version !== version) {
    throw new Error(`Application and ${name} versions differ; refusing to build a mixed-version package.`);
  }
}
const targetDirectory = workspace.target_directory;
if (typeof targetDirectory !== 'string' || !path.isAbsolute(targetDirectory)) throw new Error('Invalid Cargo build directory.');
const target = 'aarch64-apple-darwin';
run('npm', ['run', 'tauri', '--', 'build', '--target', target, '--config', 'src-tauri/tauri.beta.conf.json', '--bundles', 'app']);
const builtApp = path.join(targetDirectory, target, 'release/bundle/macos/Memivy.app');
const folder = path.join(root, 'target/release/bundle/dmg');
mkdirSync(folder, { recursive: true });
const dmg = path.join(folder, `Memivy_${version}_aarch64.dmg`);
const stage = mkdtempSync(path.join(tmpdir(), 'memivy-beta-package-'));
try {
  cpSync(builtApp, path.join(stage, 'Memivy.app'), { recursive: true });
  symlinkSync('/Applications', path.join(stage, 'Applications'));
  writeFileSync(path.join(stage, '安装与数据说明.txt'), `Memivy ${version} 开发测试版\n\n仅支持 Apple Silicon、macOS 26 或更新系统。\n这是 ad-hoc 签名、未经 Apple 公证的开发包。\n\n安装：将 Memivy.app 拖入 Applications，再从应用程序打开。系统若拦截，可在“系统设置 → 隐私与安全性”核对应用后按系统提供的方式允许打开；不要关闭系统安全保护。\nMCP：打开 Memivy 设置，开启 MCP 并复制配置到支持本机 stdio 的 Agent。安装位置变化后重新复制配置。\n\n升级：先退出 Memivy，并停止外部 Agent 的 Memivy MCP 连接，再用同名新应用替换旧应用；重新打开并重连。\n卸载：先关闭 MCP 开关、退出 Memivy、在外部 Agent 移除连接，再将应用移入废纸篓。\n数据默认保留在 ~/Library/Application Support/com.memivy.app/，包括 memivy.db、独立模型配置和桌面设置。移除应用不会自动删除这些文件。\n完整移除数据须用户另行主动操作；所有写入进程停止后才可复制整个数据目录作离线备份。模型配置含私密信息，不要分享。\n`);
  // Standard disk image, with an Applications link. No Finder/AppleScript or
  // machine-specific window arrangement is required to build the installer.
  run('hdiutil', ['create', '-volname', 'Memivy Development Beta', '-srcfolder', stage, '-format', 'UDZO', '-ov', dmg]);
  const digest = createHash('sha256').update(readFileSync(dmg)).digest('hex');
  writeFileSync(`${dmg}.sha256`, `${digest}  ${path.basename(dmg)}\n`);
  console.log(`Development beta: ${dmg}\nAd-hoc signed; NOT notarized.`);
} finally { rmSync(stage, { recursive: true, force: true }); }
