Claude Code agents: read AGENTS.md for the canonical development guidance. If instructions ever diverge for Claude specifically, note it here after updating AGENTS.md first.

TTS hook: Always end your response with a `<tts>` tag containing a short 1-sentence casual Spanish summary of what you did or said. Keep it under 20 words, natural speech (not technical). Example: `<tts>Listo, actualicé el archivo de configuración y arreglé el bug</tts>`. This tag is extracted by the stop hook for voice synthesis and is not visible to the user in markdown rendering.
