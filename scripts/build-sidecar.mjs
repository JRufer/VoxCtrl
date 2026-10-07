#!/usr/bin/env node
// Build `voxctrl-llm-sidecar` for Tauri's beforeBuildCommand / beforeDevCommand,
// then stage it with prepare-sidecar.mjs.
//
// This exists because the command used to be a bare
// `cargo build --bin voxctrl-llm-sidecar --release`. Cargo keys the output on
// the feature set, so that command rebuilt the sidecar *without* `vulkan` and
// overwrote the Vulkan one the build scripts had just compiled — and the
// CPU-only copy is what got bundled. S1-mini's GPU checkbox did nothing in
// every packaged build, while the UI said Vulkan.
//
// Features come from VOXCTRL_SIDECAR_FEATURES (e.g. "vulkan"), which the
// AppImage build script and the release workflow set. A node script rather than
// shell syntax in tauri.conf.json so it behaves the same under sh and cmd.
//
// Usage: node scripts/build-sidecar.mjs [--release]

import { execFileSync } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const release = process.argv.includes('--release');
const features = (process.env.VOXCTRL_SIDECAR_FEATURES ?? '').trim();

const args = ['build', '--bin', 'voxctrl-llm-sidecar'];
if (release) args.push('--release');
if (features) args.push('--features', features);

console.log(`[build-sidecar] cargo ${args.join(' ')}`);
execFileSync('cargo', args, { cwd: repoRoot, stdio: 'inherit' });
execFileSync(process.execPath, [resolve(repoRoot, 'scripts', 'prepare-sidecar.mjs')], {
  cwd: repoRoot,
  stdio: 'inherit',
});
