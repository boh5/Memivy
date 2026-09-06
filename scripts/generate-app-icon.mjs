import { readFileSync, writeFileSync, mkdirSync, copyFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { resolve } from 'node:path';

// Preserve the selected vector logo; the macOS 26 application background must
// be opaque and full bleed so the OS can apply its own corner mask.
// https://developer.apple.com/design/human-interface-guidelines/app-icons
const root=resolve(import.meta.dirname,'..');
const work=resolve(root,'research/app-icon');
mkdirSync(work,{recursive:true});
const source=readFileSync(resolve(root,'design-demo/brand/memivy-icon.svg'),'utf8');
if(!source.includes('rx="15"')) throw new Error('Selected logo shape changed; review the app icon export.');
writeFileSync(resolve(work,'app-icon.svg'),source.replace('width="48" height="48" viewBox','width="1024" height="1024" viewBox').replace('rx="15"','rx="0"'));
execFileSync(resolve(root,'node_modules/.bin/tauri'),['icon',resolve(work,'app-icon.svg'),'-o',work],{stdio:'ignore'});
mkdirSync(resolve(root,'src-tauri/icons'),{recursive:true});
copyFileSync(resolve(work,'icon.icns'),resolve(root,'src-tauri/icons/memivy.icns'));
copyFileSync(resolve(work,'icon.png'),resolve(root,'src-tauri/icons/app-icon.png'));
console.log('Generated macOS app icon from the selected Memivy vector logo.');
