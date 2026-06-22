import { AlertTriangle, CheckCircle, XCircle } from 'lucide-react';
import React, { useCallback, useState } from 'react';
import { useTranslation } from 'react-i18next';

import { getAuthHeaders } from '../../utils/remoteAuth';

export interface ApprovalRequest {
  id: string;
  tool: string;
  args: Record<string, unknown>;
  reason: string;
}

interface ApprovalModalProps {
  request: ApprovalRequest;
  onSettled: () => void;
}

export const ApprovalModal: React.FC<ApprovalModalProps> = ({ request, onSettled }) => {
  const { t } = useTranslation();
  const [pending, setPending] = useState<'approve' | 'reject' | null>(null);

  const decide = useCallback(
    async (approved: boolean) => {
      const action = approved ? 'approve' : 'reject';
      setPending(action);
      try {
        await fetch(`/api/approval/${encodeURIComponent(request.id)}/${action}`, {
          method: 'POST',
          headers: getAuthHeaders(),
        });
      } finally {
        onSettled();
      }
    },
    [request.id, onSettled],
  );

  const commandStr =
    typeof request.args.command === 'string'
      ? request.args.command
      : JSON.stringify(request.args, null, 2);

  const rejectLabel = pending === 'reject' ? t('approval.rejecting') : t('approval.reject');
  const approveLabel = pending === 'approve' ? t('approval.approving') : t('approval.approve');

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/70 p-4">
      <div className="w-full max-w-lg rounded-xl border border-yellow-600/60 bg-gray-900 shadow-2xl">
        <div className="flex items-start gap-3 border-b border-yellow-600/30 p-4">
          <AlertTriangle size={22} className="mt-0.5 shrink-0 text-yellow-400" />
          <div>
            <h2 className="text-base font-semibold text-yellow-300">{t('approval.title')}</h2>
            <p className="mt-0.5 text-sm text-gray-400">{request.reason}</p>
          </div>
        </div>

        <div className="p-4">
          <p className="mb-1 text-xs font-medium uppercase tracking-wide text-gray-500">
            {t('approval.toolLabel')}
            {': '}
            <span className="text-gray-300">{request.tool}</span>
          </p>
          <pre className="mt-2 max-h-48 overflow-auto rounded-lg bg-gray-800 p-3 text-sm text-gray-200 whitespace-pre-wrap break-all">
            {commandStr}
          </pre>
        </div>

        <div className="flex justify-end gap-3 border-t border-gray-700 p-4">
          <button
            type="button"
            disabled={!!pending}
            onClick={() => decide(false)}
            className="flex items-center gap-2 rounded-lg bg-red-700 px-4 py-2 text-sm font-medium text-white transition hover:bg-red-600 disabled:opacity-50"
          >
            <XCircle size={16} />
            {rejectLabel}
          </button>
          <button
            type="button"
            disabled={!!pending}
            onClick={() => decide(true)}
            className="flex items-center gap-2 rounded-lg bg-green-700 px-4 py-2 text-sm font-medium text-white transition hover:bg-green-600 disabled:opacity-50"
          >
            <CheckCircle size={16} />
            {approveLabel}
          </button>
        </div>
      </div>
    </div>
  );
};
