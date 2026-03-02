import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App.tsx';
import './index.css';
import { setupFrontendLogging } from './utils/logging.ts';
import { SystemResourcesProvider } from './contexts/SystemResourcesContext.tsx';
import { ModelProvider } from './contexts/ModelContext.tsx';
import { ChatProvider } from './contexts/ChatContext.tsx';
import { UIProvider } from './contexts/UIContext.tsx';
import { ConnectionProvider } from './contexts/ConnectionContext.tsx';
import { DownloadProvider } from './contexts/DownloadContext.tsx';

setupFrontendLogging();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ConnectionProvider>
      <SystemResourcesProvider>
        <ModelProvider>
          <DownloadProvider>
            <ChatProvider>
              <UIProvider>
                <App />
              </UIProvider>
            </ChatProvider>
          </DownloadProvider>
        </ModelProvider>
      </SystemResourcesProvider>
    </ConnectionProvider>
  </React.StrictMode>,
);
