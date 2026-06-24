import { useEffect, useRef, useState } from 'react';

// GitHub noreply patterns: {id}+{username}@users.noreply.github.com (new) or {username}@users.noreply.github.com (old)
function githubNoreplyUsername(email: string): string | null {
  const m = email.match(/^(?:\d+\+)?([^@]+)@users\.noreply\.github\.com$/i);
  return m ? m[1] : null;
}

async function sha256Hex(str: string): Promise<string> {
  const buf = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(str));
  return [...new Uint8Array(buf)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

async function resolveAvatarUrl(email: string): Promise<string | null> {
  // 1. GitHub noreply email → direct avatar (no API call needed)
  const ghUser = githubNoreplyUsername(email);
  if (ghUser) return `https://github.com/${ghUser}.png?size=20`;

  // 2. GitHub search by email (unauthenticated: 10 req/min, fine for unique contributors)
  try {
    const res = await fetch(
      `https://api.github.com/search/users?q=${encodeURIComponent(email)}+in:email`,
      { headers: { Accept: 'application/vnd.github+json' } },
    );
    if (res.ok) {
      const data = await res.json() as { total_count: number; items: { avatar_url: string }[] };
      if (data.total_count > 0 && data.items[0]?.avatar_url) return data.items[0].avatar_url;
    }
  } catch { /* offline or rate-limited */ }

  // 3. Libravatar (federates with Gravatar); d=404 so onerror fires when no photo
  const hex = await sha256Hex(email.trim().toLowerCase());
  return `https://seccdn.libravatar.org/avatar/${hex}?s=20&d=404`;
}

// Returns a map of email → loaded image URL. Absent entries fall back to the initial letter.
export function useAvatars(emails: string[]): Map<string, string> {
  const [avatars, setAvatars] = useState<Map<string, string>>(new Map());
  const attempted = useRef<Set<string>>(new Set());

  useEffect(() => {
    const unique = [...new Set(emails)].filter((e) => e && !attempted.current.has(e));
    if (unique.length === 0) return;
    for (const email of unique) {
      attempted.current.add(email);
      resolveAvatarUrl(email).then((url) => {
        if (!url) return;
        const img = new globalThis.Image();
        img.onload = () => setAvatars((prev) => new Map(prev).set(email, url));
        // onerror → leave absent; node shows initial letter
        img.src = url;
      });
    }
  }, [emails.join('\0')]); // eslint-disable-line react-hooks/exhaustive-deps

  return avatars;
}
