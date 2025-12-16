import { invoke } from '@tauri-apps/api/core';

type LogLevel = 'info' | 'warn' | 'error';

// A debounced function to send logs in batches.
const logQueue: { level: LogLevel; message: string }[] = [];
let debounceTimer: number | null = null;

function sendLogs() {
  if (logQueue.length > 0) {
    invoke('log_to_file', { logs: [...logQueue] });
    logQueue.length = 0; // Clear the queue
  }
  debounceTimer = null;
}

function queueLog(level: LogLevel, args: any[]) {
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

  console.log = (...args: any[]) => {
    originalLog.apply(console, args);
    queueLog('info', args);
  };

  console.warn = (...args: any[]) => {
    originalWarn.apply(console, args);
    queueLog('warn', args);
  };

  console.error = (...args: any[]) => {
    originalError.apply(console, args);
    queueLog('error', args);
  };
}
