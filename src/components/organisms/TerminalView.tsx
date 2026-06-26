import '@xterm/xterm/css/xterm.css';

import { FitAddon } from '@xterm/addon-fit';
import { Terminal } from '@xterm/xterm';
import { Plus, X } from 'lucide-react';
import React, { useCallback, useEffect, useRef, useState } from 'react';

import { getWsAuthParam } from '../../utils/remoteAuth';

const TERMINAL_THEME = {
  background: '#111111',
  foreground: '#cccccc',
  cursor: '#cccccc',
  cursorAccent: '#111111',
  selectionBackground: '#264f78',
  black: '#000000',
  red: '#cc5555',
  green: '#55aa55',
  yellow: '#aaaa55',
  blue: '#5555cc',
  magenta: '#aa55aa',
  cyan: '#55aaaa',
  white: '#aaaaaa',
  brightBlack: '#555555',
  brightRed: '#ff5555',
  brightGreen: '#55ff55',
  brightYellow: '#ffff55',
  brightBlue: '#5555ff',
  brightMagenta: '#ff55ff',
  brightCyan: '#55ffff',
  brightWhite: '#ffffff',
};

interface Tab {
  id: string;
  title: string;
}

function makeTabId(): string {
  return crypto.randomUUID();
}

// eslint-disable-next-line max-lines-per-function
const TerminalInstance = React.memo(
  ({
    tabId,
    isActive,
    onTitleChange,
  }: {
    tabId: string;
    isActive: boolean;
    onTitleChange: (id: string, title: string) => void;
  }) => {
    const containerRef = useRef<HTMLDivElement>(null);
    const termRef = useRef<Terminal | null>(null);
    const wsRef = useRef<WebSocket | null>(null);
    const fitRef = useRef<FitAddon | null>(null);

    // Initialize terminal + WS on mount (tab is always active on first render)
    useEffect(() => {
      if (!containerRef.current) return;

      const term = new Terminal({
        theme: TERMINAL_THEME,
        fontFamily: '"JetBrains Mono", "Cascadia Code", Menlo, Monaco, Consolas, monospace',
        fontSize: 13,
        lineHeight: 1.2,
        cursorBlink: true,
        allowTransparency: false,
        scrollback: 5000,
      });

      const fitAddon = new FitAddon();
      term.loadAddon(fitAddon);

      // Open must happen while container is visible (tab starts active)
      requestAnimationFrame(() => {
        if (!containerRef.current) return;
        term.open(containerRef.current);
        fitAddon.fit();

        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/ws/terminal${getWsAuthParam()}`;
        const ws = new WebSocket(wsUrl);
        ws.binaryType = 'arraybuffer';
        wsRef.current = ws;

        ws.onopen = () => {
          const dims = fitAddon.proposeDimensions();
          if (dims) {
            ws.send(JSON.stringify({ type: 'resize', cols: dims.cols, rows: dims.rows }));
          }
        };

        ws.onmessage = (e) => {
          if (e.data instanceof ArrayBuffer) {
            term.write(new Uint8Array(e.data));
          } else if (typeof e.data === 'string') {
            try {
              const msg = JSON.parse(e.data) as { type: string };
              if (msg.type === 'exit') {
                term.write('\r\n\x1b[90m[Process exited]\x1b[0m\r\n');
              }
            } catch {
              // ignore malformed control messages
            }
          }
        };

        ws.onerror = () => {
          term.write('\r\n\x1b[31m[Terminal connection error]\x1b[0m\r\n');
        };

        term.onData((data) => {
          if (ws.readyState === WebSocket.OPEN) {
            ws.send(new TextEncoder().encode(data));
          }
        });

        term.onTitleChange((title) => {
          if (title) onTitleChange(tabId, title);
        });
      });

      termRef.current = term;
      fitRef.current = fitAddon;

      return () => {
        wsRef.current?.close();
        term.dispose();
        termRef.current = null;
        fitRef.current = null;
        wsRef.current = null;
      };
      // eslint-disable-next-line react-hooks/exhaustive-deps
    }, []);

    // Re-fit and notify backend of new size whenever tab becomes visible
    useEffect(() => {
      if (!isActive) return;
      requestAnimationFrame(() => {
        fitRef.current?.fit();
        const dims = fitRef.current?.proposeDimensions();
        const ws = wsRef.current;
        if (dims && ws && ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ type: 'resize', cols: dims.cols, rows: dims.rows }));
        }
      });
    }, [isActive]);

    // Observe container resize to keep PTY cols/rows in sync
    useEffect(() => {
      if (!containerRef.current) return;
      const observer = new ResizeObserver(() => {
        if (!isActive) return;
        fitRef.current?.fit();
        const dims = fitRef.current?.proposeDimensions();
        const ws = wsRef.current;
        if (dims && ws && ws.readyState === WebSocket.OPEN) {
          ws.send(JSON.stringify({ type: 'resize', cols: dims.cols, rows: dims.rows }));
        }
      });
      observer.observe(containerRef.current);
      return () => observer.disconnect();
    }, [isActive]);

    return (
      <div
        ref={containerRef}
        className="h-full w-full overflow-hidden"
        style={{ display: isActive ? 'block' : 'none' }}
      />
    );
  },
);

TerminalInstance.displayName = 'TerminalInstance';

// eslint-disable-next-line max-lines-per-function
export const TerminalView = React.memo(() => {
  const initialId = makeTabId();
  const [tabs, setTabs] = useState<Tab[]>([{ id: initialId, title: 'Terminal 1' }]);
  const [activeId, setActiveId] = useState(initialId);

  const addTab = useCallback(() => {
    const id = makeTabId();
    const num = tabs.length + 1;
    setTabs((prev) => [...prev, { id, title: `Terminal ${num}` }]);
    setActiveId(id);
  }, [tabs.length]);

  const closeTab = useCallback(
    (id: string) => {
      if (tabs.length === 1) return;
      const idx = tabs.findIndex((t) => t.id === id);
      setTabs((prev) => prev.filter((t) => t.id !== id));
      if (activeId === id) {
        const next = tabs[idx === 0 ? 1 : idx - 1];
        setActiveId(next.id);
      }
    },
    [tabs, activeId],
  );

  const handleTitleChange = useCallback((id: string, title: string) => {
    setTabs((prev) => prev.map((t) => (t.id === id ? { ...t, title } : t)));
  }, []);

  return (
    <div className="flex h-full flex-col bg-[#111111]">
      {/* Tab bar */}
      <div className="flex items-center border-b border-border/50 bg-card/60 px-1">
        {tabs.map((tab) => (
          <div
            key={tab.id}
            role="tab"
            tabIndex={0}
            aria-selected={tab.id === activeId}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') setActiveId(tab.id);
            }}
            onClick={() => setActiveId(tab.id)}
            className={`flex cursor-pointer select-none items-center gap-1 border-b-2 px-3 py-1.5 text-xs transition-colors ${
              tab.id === activeId
                ? 'border-primary text-foreground'
                : 'border-transparent text-muted-foreground hover:text-foreground'
            }`}
          >
            <span className="max-w-[120px] truncate">{tab.title}</span>
            {tabs.length > 1 && (
              <button
                onClick={(e) => {
                  e.stopPropagation();
                  closeTab(tab.id);
                }}
                className="ml-0.5 rounded p-0.5 opacity-60 hover:opacity-100 hover:text-destructive"
                title="Close tab"
                aria-label="Close terminal tab"
              >
                <X className="size-2.5" />
              </button>
            )}
          </div>
        ))}
        <button
          onClick={addTab}
          className="ml-1 rounded p-1 text-muted-foreground hover:bg-muted hover:text-foreground"
          title="New terminal tab"
          aria-label="New terminal tab"
        >
          <Plus className="size-3.5" />
        </button>
      </div>

      {/* Terminal instances — kept mounted, toggled via display:none */}
      <div className="min-h-0 flex-1 p-1">
        {tabs.map((tab) => (
          <TerminalInstance
            key={tab.id}
            tabId={tab.id}
            isActive={tab.id === activeId}
            onTitleChange={handleTitleChange}
          />
        ))}
      </div>
    </div>
  );
});

TerminalView.displayName = 'TerminalView';
