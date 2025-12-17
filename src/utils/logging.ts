import { invoke } from '@tauri-apps/api/core';

type LogLevel = 'info' | 'warn' | 'error';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

// A debounced function to send logs in batches.
const logQueue: { level: LogLevel; message: string }[] = [];
let debounceTimer: number | null = null;

function sendLogs() {
  if (logQueue.length > 0) {
    const payload = [...logQueue];
    if (isTauri) {
      invoke('log_to_file', { logs: payload }).catch(() => {
        // Ignore failures; we still clear to prevent growth
      });
    } else {
      fetch('/api/logs/frontend', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ logs: payload }),
        keepalive: true,
      }).catch(() => {
        // Ignore failures to avoid noisy recursion; queue is still cleared to prevent growth.
      });
    }
    logQueue.length = 0; // Clear the queue
  }
  debounceTimer = null;
}

function queueLog(level: LogLevel, args: unknown[]) {
    const message = args.map(arg => {
        try {
            if (arg instanceof Error) {
                return JSON.stringify({ message: arg.message, stack: arg.stack });
            }
            return JSON.stringify(arg);
        } catch (e) {
            return 'Unserializable object';
        }
    }).join(' ');

    logQueue.push({ level, message });

    if (!debounceTimer) {
        debounceTimer = window.setTimeout(sendLogs, 500); // Send logs every 500ms
    }
}

export function setupFrontendLogging() {
  const originalLog = console.log;
  const originalWarn = console.warn;
  const originalError = console.error;

  console.log = (...args: unknown[]) => {
    originalLog.apply(console, args);
    queueLog('info', args);
  };

  console.warn = (...args: unknown[]) => {
    originalWarn.apply(console, args);
    queueLog('warn', args);
  };

  console.error = (...args: unknown[]) => {
    originalError.apply(console, args);
    queueLog('error', args);
  };

  // Capture global errors and unhandled promise rejections
  window.addEventListener('error', (event) => {
    queueLog('error', [`Unhandled error: ${event.message}`, event.error?.stack || '']);
  });

  window.addEventListener('unhandledrejection', (event) => {
    const reason = event.reason instanceof Error
      ? `${event.reason.message}\n${event.reason.stack || ''}`
      : JSON.stringify(event.reason);
    queueLog('error', [`Unhandled rejection: ${reason}`]);
  });
}
