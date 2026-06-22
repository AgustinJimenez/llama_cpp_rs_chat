import { X } from 'lucide-react';
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { useSystemResources } from '../../contexts/SystemResourcesContext';
import { DebugPanelContent } from '../atoms/DebugPanel';

import { SystemUsage } from './SystemUsage';

interface RightSidebarProps {
  isOpen: boolean;
  onClose: () => void;
}

type Tab = 'system' | 'debug';

export const RightSidebar = ({ isOpen, onClose }: RightSidebarProps) => {
  const { t } = useTranslation();
  const { setMonitorActive } = useSystemResources();
  const [activeTab, setActiveTab] = useState<Tab>('system');

  useEffect(() => {
    setMonitorActive(isOpen);
    return () => setMonitorActive(false);
  }, [isOpen, setMonitorActive]);

  const showDebugTab = import.meta.env.DEV;

  const mobileOverlay = isOpen && (
    <div
      className="fixed inset-0 z-40 bg-black/50 md:hidden"
      role="button"
      tabIndex={0}
      aria-label="Close system monitor"
      onClick={onClose}
      onKeyDown={(e) => {
        if (e.key === 'Enter' || e.key === ' ') onClose();
      }}
    />
  );

  const debugTabButton = showDebugTab && (
    <button
      onClick={() => setActiveTab('debug')}
      className={`rounded px-3 py-1 text-sm font-medium transition-colors ${
        activeTab === 'debug'
          ? 'bg-muted text-foreground'
          : 'text-muted-foreground hover:text-foreground'
      }`}
    >
      Debug
    </button>
  );

  const systemContent = activeTab === 'system' && <SystemUsage expanded />;
  const debugContent = activeTab === 'debug' && showDebugTab && <DebugPanelContent />;

  return (
    <>
      {mobileOverlay}

      {/* Sidebar */}
      <aside
        aria-label={t('rightSidebar.systemMonitor')}
        className={`fixed right-0 top-0 z-50 flex h-full flex-col border-l border-border bg-card transition-transform duration-300 ${
          isOpen ? 'translate-x-0' : 'translate-x-full'
        }`}
        style={{ width: '320px' }}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <div className="flex gap-1">
            <button
              onClick={() => setActiveTab('system')}
              className={`rounded px-3 py-1 text-sm font-medium transition-colors ${
                activeTab === 'system'
                  ? 'bg-muted text-foreground'
                  : 'text-muted-foreground hover:text-foreground'
              }`}
            >
              System
            </button>
            {debugTabButton}
          </div>

          <button
            onClick={onClose}
            className="rounded-lg p-2 transition-colors hover:bg-muted"
            title={t('rightSidebar.closeSidebar')}
          >
            <X className="size-5" />
          </button>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-y-auto p-4">
          {systemContent}
          {debugContent}
        </div>
      </aside>
    </>
  );
};
