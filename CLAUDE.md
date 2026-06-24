Claude Code agents: read AGENTS.md for the canonical development guidance. If instructions ever diverge for Claude specifically, note it here after updating AGENTS.md first.

CI validation: After editing any .ts/.tsx files, always run `npx tsc --noEmit`, `npx eslint "src/**/*.{ts,tsx}" --max-warnings 0`, and `node scripts/check-i18n-keys.js` before finishing. After editing any .rs files, always run `rtk cargo check --workspace --no-default-features`. A Stop hook enforces the TS/lint/i18n checks automatically — if it fires with errors, fix them before completing your response.

TTS hook: Always end your response with a `<tts>` tag containing a short 1-sentence casual Spanish summary of what you did or said. Keep it under 20 words, natural speech (not technical). Example: `<tts>Listo, actualicé el archivo de configuración y arreglé el bug</tts>`. This tag is extracted by the stop hook for voice synthesis and is not visible to the user in markdown rendering.
