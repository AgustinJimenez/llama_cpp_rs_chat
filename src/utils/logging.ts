/* eslint-disable no-console -- logging utility that wraps console methods */
import { invoke } from '@tauri-apps/api/core';

import { recordAppError } from './tauriCommands';

const LOG_FLUSH_INTERVAL_MS = 500;

type LogLevel = 'info' | 'warn' | 'error';

const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

// A debounced function to send logs in batches.
const logQueue: { level: LogLevel; message: string; timestamp: string }[] = [];
let debounceTimer: number | null = null;

function serializeArgs(args: unknown[]) {
  return args
    .map((arg) => {
      try {
        if (arg instanceof Error) {
          return JSON.stringify({ message: arg.message, stack: arg.stack });
        }
        return JSON.stringify(arg);
      } catch {
        return 'Unserializable object';
      }
    })
    .join(' ');
}

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
  const message = serializeArgs(args);

  logQueue.push({ level, message, timestamp: new Date().toISOString() });

  if (!debounceTimer) {
    debounceTimer = window.setTimeout(sendLogs, LOG_FLUSH_INTERVAL_MS); // Send logs every 500ms
  }
}

function persistAppError(source: string, args: unknown[]) {
  void recordAppError({
    level: 'error',
    source,
    message: serializeArgs(args).slice(0, 8000), // eslint-disable-line @typescript-eslint/no-magic-numbers
    timestamp: Date.now(),
  }).catch(() => {
    // Ignore persistence failures to avoid recursive logging.
  });
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
    persistAppError('frontend.console', args);
  };

  // Capture global errors and unhandled promise rejections
  window.addEventListener('error', (event) => {
    const args = [`Unhandled error: ${event.message}`, event.error?.stack || ''];
    queueLog('error', args);
    persistAppError('frontend.window_error', args);
  });

  window.addEventListener('unhandledrejection', (event) => {
    const reason =
      event.reason instanceof Error
        ? `${event.reason.message}\n${event.reason.stack || ''}`
        : JSON.stringify(event.reason);
    const args = [`Unhandled rejection: ${reason}`];
    queueLog('error', args);
    persistAppError('frontend.unhandled_rejection', args);
  });
}
