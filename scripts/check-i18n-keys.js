#!/usr/bin/env node
// Scan all .ts/.tsx source files for t('key') calls and verify every key
// exists (and is not an empty string) in src/i18n/locales/en.json.
// Exits with code 1 and prints a report if any keys are missing.

import fs from 'node:fs';
import path from 'node:path';
import { globSync } from 'glob';

const ROOT = process.cwd();
const LOCALE_FILE = path.join(ROOT, 'src/i18n/locales/en.json');
const SRC_GLOB = 'src/**/*.{ts,tsx}';

// ── Load locale ──────────────────────────────────────────────────────────────

const locale = JSON.parse(fs.readFileSync(LOCALE_FILE, 'utf8'));

/** Resolve a dot-separated key like "approval.title" in the locale object. */
function resolveKey(key) {
  const parts = key.split('.');
  let node = locale;
  for (const part of parts) {
    if (typeof node !== 'object' || node === null || !(part in node)) return undefined;
    node = node[part];
  }
  return node;
}

// ── Extract t() calls ────────────────────────────────────────────────────────

// Match t('key') / t("key") / t(`key`) — static strings only, no interpolation.
// Also catches useTranslation hook namespace: t('ns:key') → skip ns prefix (not used here).
const T_CALL_RE = /\bt\(\s*(['"`])([^'"`\n${]+)\1/g;

const missing = [];   // { file, key } — key not in locale
const empty   = [];   // { file, key } — key present but value is ''
const dynamic = [];   // files with dynamic t() calls (can't validate statically)

const files = globSync(SRC_GLOB, { cwd: ROOT, absolute: true });

for (const file of files) {
  const src = fs.readFileSync(file, 'utf8');
  let match;
  T_CALL_RE.lastIndex = 0;
  while ((match = T_CALL_RE.exec(src)) !== null) {
    const key = match[2];
    // Skip keys that look dynamic (contain variables or are suspiciously short)
    if (key.includes('${') || key.length === 0) {
      dynamic.push({ file, key });
      continue;
    }
    const value = resolveKey(key);
    const rel = path.relative(ROOT, file).replace(/\\/g, '/');
    if (value === undefined) {
      missing.push({ file: rel, key });
    } else if (value === '') {
      empty.push({ file: rel, key });
    }
  }
}

// ── Report ───────────────────────────────────────────────────────────────────

let failed = false;

if (missing.length > 0) {
  failed = true;
  console.error(`\n❌ Missing i18n keys (${missing.length}):`);
  for (const { file, key } of missing) {
    console.error(`   ${key}  ←  ${file}`);
  }
}

if (empty.length > 0) {
  failed = true;
  console.error(`\n⚠️  Empty i18n values (${empty.length}):`);
  for (const { file, key } of empty) {
    console.error(`   ${key}  ←  ${file}`);
  }
}

if (!failed) {
  console.log(`✅ i18n: all ${files.length} files checked, no missing keys found.`);
}

process.exit(failed ? 1 : 0);
