import React from 'react';
import ReactDOM from 'react-dom/client';

import { App } from './App.tsx';
import './i18n';
import './index.css';
import { AgentProvider } from './contexts/AgentContext.tsx';
import { ChatProvider } from './contexts/ChatContext.tsx';
import { ConnectionProvider } from './contexts/ConnectionContext.tsx';
import { DownloadProvider } from './contexts/DownloadContext.tsx';
import { ModelProvider } from './contexts/ModelContext.tsx';
import { SystemResourcesProvider } from './contexts/SystemResourcesContext.tsx';
import { UIProvider } from './contexts/UIContext.tsx';
import { setupFrontendLogging } from './utils/logging.ts';

setupFrontendLogging();

if (import.meta.env.DEV) {
  const axe = await import('@axe-core/react');
  axe.default(React, ReactDOM, 1000);
}

const rootElement = document.getElementById('root');
if (!rootElement) throw new Error('Root element not found');

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <ConnectionProvider>
      <SystemResourcesProvider>
        <ModelProvider>
          <DownloadProvider>
            <AgentProvider>
              <ChatProvider>
                <UIProvider>
                  <App />
                </UIProvider>
              </ChatProvider>
            </AgentProvider>
          </DownloadProvider>
        </ModelProvider>
      </SystemResourcesProvider>
    </ConnectionProvider>
  </React.StrictMode>,
);
