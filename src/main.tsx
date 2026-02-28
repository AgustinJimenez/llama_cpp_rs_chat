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

setupFrontendLogging();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ConnectionProvider>
      <SystemResourcesProvider>
        <ModelProvider>
          <ChatProvider>
            <UIProvider>
              <App />
            </UIProvider>
          </ChatProvider>
        </ModelProvider>
      </SystemResourcesProvider>
    </ConnectionProvider>
  </React.StrictMode>,
);
