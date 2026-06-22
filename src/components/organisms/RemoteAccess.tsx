import { QRCodeSVG } from 'qrcode.react';
import { useState, useEffect, useCallback } from 'react';
import { useTranslation } from 'react-i18next';

import { getAuthHeaders } from '../../utils/remoteAuth';

const TOKEN_COPIED_RESET_MS = 2000;

function apiFetch(url: string, init?: RequestInit): Promise<Response> {
  return fetch(url, { ...init, headers: { ...getAuthHeaders(), ...(init?.headers as Record<string, string> | undefined) } });
}

interface RemoteStatus {
  token: string;
  lan_url: string | null;
  lan_qr: string | null;
}

interface UpnpResult {
  success: boolean;
  public_url?: string;
  public_qr?: string;
  error?: string;
}

export const RemoteAccess = (): JSX.Element => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [upnpLoading, setUpnpLoading] = useState(false);
  const [upnpResult, setUpnpResult] = useState<UpnpResult | null>(null);
  const [tokenCopied, setTokenCopied] = useState(false);

  const fetchStatus = useCallback(() => {
    setLoading(true);
    apiFetch('/api/remote/status')
      .then((r) => r.json() as Promise<RemoteStatus>)
      .then(setStatus)
      .catch(() => {})
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    fetchStatus();
  }, [fetchStatus]);

  const handleRegenerateToken = useCallback(() => {
    apiFetch('/api/remote/token/regenerate', { method: 'POST' })
      .then((r) => r.json() as Promise<{ token: string }>)
      .then((data) => setStatus((s) => s ? { ...s, token: data.token, lan_qr: s.lan_url ? `${s.lan_url}#token=${data.token}` : null } : s))
      .catch(() => {});
  }, []);

  const handleEnableUpnp = useCallback(() => {
    setUpnpLoading(true);
    setUpnpResult(null);
    apiFetch('/api/remote/upnp/enable', { method: 'POST' })
      .then((r) => r.json() as Promise<UpnpResult>)
      .then(setUpnpResult)
      .catch((error: unknown) => setUpnpResult({ success: false, error: String(error) }))
      .finally(() => setUpnpLoading(false));
  }, []);

  const handleDisableUpnp = useCallback(() => {
    setUpnpLoading(true);
    apiFetch('/api/remote/upnp/disable', { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify({ external_port: 18080 }) })
      .then(() => setUpnpResult(null))
      .catch(() => {})
      .finally(() => setUpnpLoading(false));
  }, []);

  const handleCopyToken = useCallback(() => {
    if (!status?.token) return;
    void navigator.clipboard.writeText(status.token).then(() => {
      setTokenCopied(true);
      setTimeout(() => setTokenCopied(false), TOKEN_COPIED_RESET_MS);
    });
  }, [status?.token]);

  if (loading) {
    return <p className="text-sm text-muted-foreground">{t('common.loading')}</p>;
  }

  const qrUrl = upnpResult?.public_qr ?? status?.lan_qr;
  const displayUrl = upnpResult?.public_url ?? status?.lan_url;
  const copyLabel = tokenCopied ? t('common.copied') : t('common.copy');
  const upnpBtnLabel = upnpLoading ? t('common.loading') : t('remoteAccess.upnpEnable');
  const upnpErrorMsg = upnpResult?.error ?? t('remoteAccess.upnpFailed');

  const qrSection = qrUrl ? (
    <div className="flex flex-col items-center gap-3 rounded-lg border border-border bg-white p-4">
      <QRCodeSVG value={qrUrl} size={180} />
      {displayUrl && <p className="break-all text-center font-mono text-xs text-foreground">{displayUrl}</p>}
    </div>
  ) : (
    <div className="rounded-lg border border-border bg-muted/40 p-4 text-center text-sm text-muted-foreground">
      {t('remoteAccess.noLanUrl')}
    </div>
  );

  return (
    <div className="space-y-5">
      <div>
        <p className="mb-3 text-sm text-muted-foreground">
          {t('remoteAccess.description')}
        </p>
      </div>

      {qrSection}

      {/* Token */}
      <div>
        <label className="mb-1 block text-xs font-medium text-muted-foreground">
          {t('remoteAccess.accessToken')}
        </label>
        <div className="flex gap-2">
          <code className="flex-1 truncate rounded border border-border bg-muted px-3 py-1.5 font-mono text-xs">
            {status?.token ?? '—'}
          </code>
          <button
            type="button"
            onClick={handleCopyToken}
            className="rounded border border-border px-3 py-1.5 text-xs transition-colors hover:bg-muted"
          >
            {copyLabel}
          </button>
          <button
            type="button"
            onClick={handleRegenerateToken}
            className="rounded border border-border px-3 py-1.5 text-xs transition-colors hover:bg-muted"
          >
            {t('remoteAccess.regenerate')}
          </button>
        </div>
      </div>

      {/* UPnP */}
      <div>
        <label className="mb-1 block text-xs font-medium text-muted-foreground">
          {t('remoteAccess.upnpTitle')}
        </label>
        <p className="mb-2 text-xs text-muted-foreground">{t('remoteAccess.upnpDescription')}</p>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={handleEnableUpnp}
            disabled={upnpLoading}
            className="rounded border border-border px-3 py-1.5 text-xs transition-colors hover:bg-muted disabled:opacity-50"
          >
            {upnpBtnLabel}
          </button>
          {upnpResult?.success && (
            <button
              type="button"
              onClick={handleDisableUpnp}
              disabled={upnpLoading}
              className="rounded border border-border px-3 py-1.5 text-xs transition-colors hover:bg-muted disabled:opacity-50"
            >
              {t('remoteAccess.upnpDisable')}
            </button>
          )}
        </div>
        {upnpResult && !upnpResult.success && (
          <p className="mt-1 text-xs text-red-400">{upnpErrorMsg}</p>
        )}
        {upnpResult?.public_url && (
          <p className="mt-1 text-xs text-green-400">{t('remoteAccess.upnpActive', { url: upnpResult.public_url })}</p>
        )}
      </div>
    </div>
  );
};
