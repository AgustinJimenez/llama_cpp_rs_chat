#!/usr/bin/env bash
# Stop hook: runs fast CI checks after every Claude response.
# Exit 0 = silent pass. Exit 2 = errors fed back to Claude for auto-fix.

cd "$(dirname "$0")/.."

ERRORS=""

# TypeScript
TSC_OUT=$(npx tsc --noEmit 2>&1)
if [ $? -ne 0 ]; then
  ERRORS="${ERRORS}\n## TypeScript errors\n\`\`\`\n${TSC_OUT}\n\`\`\`\n"
fi

# ESLint
ESLINT_OUT=$(npx eslint "src/**/*.{ts,tsx}" --max-warnings 0 2>&1)
if [ $? -ne 0 ]; then
  ERRORS="${ERRORS}\n## ESLint errors\n\`\`\`\n${ESLINT_OUT}\n\`\`\`\n"
fi

# i18n keys
I18N_OUT=$(node scripts/check-i18n-keys.js 2>&1)
if [ $? -ne 0 ]; then
  ERRORS="${ERRORS}\n## i18n key errors\n\`\`\`\n${I18N_OUT}\n\`\`\`\n"
fi

if [ -n "$ERRORS" ]; then
  printf "CI checks failed — please fix before finishing:\n${ERRORS}"
  exit 2
fi
