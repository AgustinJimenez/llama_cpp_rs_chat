#!/usr/bin/env node
// Simple size gate: warn and fail if tracked files exceed limits.
// - 5MB limit for source-like paths
// - 100MB hard cap for any checked file

import fs from 'node:fs';
import path from 'node:path';

const ROOT = process.cwd();
const SOFT_LIMIT = 5 * 1024 * 1024; // 5MB
const HARD_LIMIT = 100 * 1024 * 1024; // 100MB

const SKIP_DIRS = new Set([
  'node_modules',
  'dist',
  'target',
  'target2',
  'playwright-report',
  'logs',
  'assets',
  'public',
  '.git',
  'gen',
  'capabilities',
]);

const WATCH_DIRS = [
  'src',
  'tests',
  'config',
  'scripts',
];

function shouldSkipDir(dirName) {
  return SKIP_DIRS.has(dirName);
}

function walk(dir, results = []) {
  if (shouldSkipDir(path.basename(dir))) {
    return results;
  }
  let entries = [];
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return results;
  }

  for (const entry of entries) {
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      walk(fullPath, results);
    } else if (entry.isFile()) {
      results.push(fullPath);
    }
  }
  return results;
}

function main() {
  const filesToCheck = [];
  for (const dir of WATCH_DIRS) {
    const abs = path.join(ROOT, dir);
    if (fs.existsSync(abs)) {
      walk(abs, filesToCheck);
    }
  }

  const offenders = [];
  for (const filePath of filesToCheck) {
    let size = 0;
    try {
      size = fs.statSync(filePath).size;
    } catch {
      continue;
    }
    if (size > SOFT_LIMIT) {
      offenders.push({ filePath, size });
    }
  }

  const hardOffenders = offenders.filter(o => o.size > HARD_LIMIT);
  const formatSize = (bytes) => `${(bytes / (1024 * 1024)).toFixed(2)} MB`;

  if (hardOffenders.length > 0) {
    console.error('❌ Found files above hard cap (100MB):');
    hardOffenders.forEach(o => console.error(` - ${o.filePath} (${formatSize(o.size)})`));
    process.exit(1);
  }

  if (offenders.length > 0) {
    console.error('❌ Found large files above 5MB (consider LFS or relocation):');
    offenders.forEach(o => console.error(` - ${o.filePath} (${formatSize(o.size)})`));
    process.exit(1);
  }

  console.log('✅ File size check passed');
}

main();
