import React, { useState, useEffect } from 'react';

// Characters kept in the animated "tail" during streaming
const FADE_TAIL_CHARS = 80;
// Re-key the tail span every N chars so the fade re-triggers on new content
const FADE_REKEY_INTERVAL = 15;
// Milliseconds of silence before the idle cursor appears
const IDLE_CURSOR_DELAY_MS = 150;

interface StreamingTextProps {
  content: string;
  isStreaming?: boolean;
}

/**
 * Renders streaming text with two visual effects:
 * - Trailing fade-in: the last ~80 chars materialise over 320ms (re-triggered every 15 new chars)
 * - Idle cursor: a blinking caret appears after 150ms of stream silence
 */
export const StreamingText: React.FC<StreamingTextProps> = ({ content, isStreaming }) => {
  const [isIdle, setIsIdle] = useState(false);

  // react-doctor-disable-next-line react-doctor/no-cascading-set-state — single state, conditional branches
  useEffect(() => {
    if (!isStreaming) {
      setIsIdle(false);
      return;
    }
    setIsIdle(false);
    const t = setTimeout(() => setIsIdle(true), IDLE_CURSOR_DELAY_MS);
    return () => clearTimeout(t);
  }, [content, isStreaming]);

  if (!isStreaming || content.length <= FADE_TAIL_CHARS) {
    return (
      <p className="whitespace-pre-wrap text-sm leading-relaxed" data-testid="message-content">
        {content}
        {!!isStreaming && <span className="streaming-cursor" aria-hidden="true" />}
      </p>
    );
  }

  const cutoff = content.length - FADE_TAIL_CHARS;
  const settled = content.slice(0, cutoff);
  const tail = content.slice(cutoff);
  const tailKey = Math.floor(content.length / FADE_REKEY_INTERVAL);

  return (
    <p className="whitespace-pre-wrap text-sm leading-relaxed" data-testid="message-content">
      {settled}
      <span key={tailKey} className="token-fade-in">
        {tail}
      </span>
      {!!isIdle && <span className="streaming-cursor" aria-hidden="true" />}
    </p>
  );
};
