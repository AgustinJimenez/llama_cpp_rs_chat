const EXTENSION_TO_LANGUAGE: Record<string, string> = {
  rs: 'rust',
  py: 'python',
  js: 'javascript',
  jsx: 'javascript',
  ts: 'typescript',
  tsx: 'typescript',
  json: 'json',
  css: 'css',
  c: 'c',
  h: 'c',
  cpp: 'cpp',
  hpp: 'cpp',
  java: 'java',
  go: 'go',
  yaml: 'yaml',
  yml: 'yaml',
  md: 'markdown',
  sql: 'sql',
  toml: 'toml',
  sh: 'bash',
  bash: 'bash',
  bat: 'bash',
  ps1: 'bash',
};

export function detectLanguageFromPath(path: string): string | null {
  const ext = path.split('.').pop()?.toLowerCase();
  if (!ext) return null;
  return EXTENSION_TO_LANGUAGE[ext] || null;
}
