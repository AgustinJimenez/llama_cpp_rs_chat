import { X } from 'lucide-react';
import { SystemUsage } from './SystemUsage';

interface RightSidebarProps {
  isOpen: boolean;
  onClose: () => void;
}

export function RightSidebar({ isOpen, onClose }: RightSidebarProps) {
  return (
    <>
      {/* Overlay for mobile */}
      {isOpen ? <div
          className="fixed inset-0 bg-black/50 z-40 md:hidden"
          role="button"
          tabIndex={0}
          onClick={onClose}
          onKeyDown={(e) => { if (e.key === 'Enter' || e.key === ' ') onClose(); }}
        /> : null}

      {/* Sidebar */}
      <div
        className={`fixed top-0 right-0 h-full bg-card border-l border-border z-50 transition-transform duration-300 ${
          isOpen ? 'translate-x-0' : 'translate-x-full'
        }`}
        style={{ width: '320px' }}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-border">
          <h2 className="text-lg font-semibold">System Monitor</h2>
          <button
            onClick={onClose}
            className="p-2 hover:bg-muted rounded-lg transition-colors"
            title="Close sidebar"
          >
            <X className="h-5 w-5" />
          </button>
        </div>

        {/* Content */}
        <div className="p-4">
          <SystemUsage expanded active={isOpen} />
        </div>
      </div>
    </>
  );
}
