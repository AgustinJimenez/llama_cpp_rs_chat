import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App.tsx';
import './index.css';
import { setupFrontendLogging } from './utils/logging.ts';
import { SystemResourcesProvider } from './contexts/SystemResourcesContext.tsx';
import { ModelProvider } from './contexts/ModelContext.tsx';
import { ChatProvider } from './contexts/ChatContext.tsx';
import { UIProvider } from './contexts/UIContext.tsx';

setupFrontendLogging();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <SystemResourcesProvider>
      <ModelProvider>
        <ChatProvider>
          <UIProvider>
            <App />
          </UIProvider>
        </ChatProvider>
      </ModelProvider>
    </SystemResourcesProvider>
  </React.StrictMode>,
);
