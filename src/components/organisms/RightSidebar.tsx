import { X } from 'lucide-react';
import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';

import { useSystemResources } from '../../contexts/SystemResourcesContext';

import { SystemUsage } from './SystemUsage';

interface RightSidebarProps {
  isOpen: boolean;
  onClose: () => void;
}

export const RightSidebar = ({ isOpen, onClose }: RightSidebarProps) => {
  const { t } = useTranslation();
  const { setMonitorActive } = useSystemResources();

  useEffect(() => {
    setMonitorActive(isOpen);
    return () => setMonitorActive(false);
  }, [isOpen, setMonitorActive]);

  return (
    <>
      {/* Overlay for mobile */}
      {!!isOpen && (
        <div
          className="fixed inset-0 z-40 bg-black/50 md:hidden"
          role="button"
          tabIndex={0}
          aria-label={t('rightSidebar.closeMonitorAria')}
          onClick={onClose}
          onKeyDown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') onClose();
          }}
        />
      )}

      {/* Sidebar */}
      <aside
        aria-label={t('rightSidebar.systemMonitor')}
        className={`fixed right-0 top-0 z-50 h-full border-l border-border bg-card transition-transform duration-300 ${
          isOpen ? 'translate-x-0' : 'translate-x-full'
        }`}
        style={{ width: '320px' }}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-border px-4 py-3">
          <h2 className="text-lg font-semibold">{t('rightSidebar.systemMonitor')}</h2>
          <button
            onClick={onClose}
            className="rounded-lg p-2 transition-colors hover:bg-muted"
            title={t('rightSidebar.closeSidebar')}
          >
            <X className="size-5" />
          </button>
        </div>

        {/* Content */}
        <div className="p-4">
          <SystemUsage expanded />
        </div>
      </aside>
    </>
  );
};
