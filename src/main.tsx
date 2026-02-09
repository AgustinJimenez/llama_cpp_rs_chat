import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App.tsx';
import './index.css';
import { setupFrontendLogging } from './utils/logging.ts';
import { SystemResourcesProvider } from './contexts/SystemResourcesContext.tsx';

setupFrontendLogging();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <SystemResourcesProvider>
      <App />
    </SystemResourcesProvider>
  </React.StrictMode>,
);