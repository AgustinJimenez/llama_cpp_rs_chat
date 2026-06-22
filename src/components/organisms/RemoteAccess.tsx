import { QRCodeSVG } from 'qrcode.react';
import React, { useCallback, useEffect, useState } from 'react';
import toast from 'react-hot-toast';
import { useTranslation } from 'react-i18next';

import { Button } from '../atoms/button';

interface RemoteStatus {
  token: string;
  lan_url: string | null;
  lan_qr: string | null;
}

interface UpnpResult {
  success: boolean;
  public_url?: string;
  public_qr?: string;
  external_ip?: string;
  external_port?: number;
}

// eslint-disable-next-line react-doctor/prefer-useReducer -- genuinely distinct remote access UI states
const RemoteAccess: React.FC = () => {
  const { t } = useTranslation();
  const [status, setStatus] = useState<RemoteStatus | null>(null);
  const [upnpResult, setUpnpResult] = useState<UpnpResult | null>(null);
  const [upnpLoading, setUpnpLoading] = useState(false);
  const [upnpError, setUpnpError] = useState<string | null>(null);
  const [showToken, setShowToken] = useState(false);

  const fetchStatus = useCallback(async () => {
    try {
      const res = await fetch('/api/remote/status');
      if (res.ok) setStatus((await res.json()) as RemoteStatus);
    } catch {
      /* ignore */
    }
  }, []);

  useEffect(() => {
    void fetchStatus();
  }, [fetchStatus]);

  const handleEnableUpnp = async () => {
    setUpnpLoading(true);
    setUpnpError(null);
    try {
      const res = await fetch('/api/remote/upnp/enable', { method: 'POST' });
      const data = (await res.json()) as UpnpResult & { error?: string };
      if (data.success) {
        setUpnpResult(data);
        toast.success(t('remoteAccess.upnpEnabled'));
      } else {
        setUpnpError((data as { error?: string }).error ?? t('remoteAccess.upnpFailed'));
      }
    } catch (e) {
      setUpnpError(String(e));
    } finally {
      setUpnpLoading(false);
    }
  };

  const handleRegenerateToken = async () => {
    try {
      const res = await fetch('/api/remote/token/regenerate', { method: 'POST' });
      if (res.ok) {
        await fetchStatus();
        toast.success(t('remoteAccess.tokenRegenerated'));
      }
    } catch {
      toast.error(t('remoteAccess.tokenRegenFailed'));
    }
  };

  const copyToClipboard = (text: string) => {
    void navigator.clipboard.writeText(text).then(() => toast.success(t('remoteAccess.copied')));
  };

  if (!status) return <p className="text-sm text-muted-foreground">{t('common.loading')}</p>;

  const activeQr = upnpResult?.public_qr ?? status.lan_qr;
  const activeUrl = upnpResult?.public_url ?? status.lan_url;
  const hasUpnpSuccess = upnpResult?.success;

  const statusContent = activeQr ? (
    <div className="flex flex-col items-center gap-3">
      <div className="rounded-lg border bg-white p-3">
        <QRCodeSVG value={activeQr} size={180} />
      </div>
      <p className="max-w-xs break-all text-center text-xs text-muted-foreground">{activeUrl}</p>
      <Button variant="outline" size="sm" onClick={() => activeUrl && copyToClipboard(activeUrl)}>
        {t('remoteAccess.copyUrl')}
      </Button>
    </div>
  ) : (
    <p className="text-sm text-muted-foreground">{t('remoteAccess.noIp')}</p>
  );

  const publicUrl = upnpResult?.public_url ?? '';
  const upnpButtonLabel = upnpLoading ? t('remoteAccess.tryingUpnp') : t('remoteAccess.enableUpnp');
  const upnpContent = hasUpnpSuccess ? (
    <div className="text-xs text-green-600 dark:text-green-400">
      {t('remoteAccess.publicUrl', { url: publicUrl })}
    </div>
  ) : (
    <Button
      variant="outline"
      size="sm"
      onClick={() => void handleEnableUpnp()}
      disabled={upnpLoading}
    >
      {upnpButtonLabel}
    </Button>
  );
  const tokenDisplay = showToken ? status.token : '••••••••••••••••';

  const showHideLabel = showToken ? t('remoteAccess.hide') : t('remoteAccess.show');

  return (
    <div className="space-y-6">
      <div>
        <h3 className="mb-1 text-sm font-semibold">{t('remoteAccess.title')}</h3>
        <p className="text-xs text-muted-foreground">{t('remoteAccess.description')}</p>
      </div>

      {/* QR code */}
      {statusContent}

      {/* UPnP section */}
      <div className="space-y-2">
        <h4 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          {t('remoteAccess.upnpTitle')}
        </h4>
        <p className="text-xs text-muted-foreground">{t('remoteAccess.upnpDescription')}</p>
        {!!upnpError && <p className="text-xs text-destructive">{upnpError}</p>}
        {upnpContent}
      </div>

      {/* Token management */}
      <div className="space-y-2 border-t pt-4">
        <h4 className="text-xs font-medium uppercase tracking-wider text-muted-foreground">
          {t('remoteAccess.accessToken')}
        </h4>
        <div className="flex items-center gap-2">
          <code className="rounded bg-muted px-2 py-1 font-mono text-xs">{tokenDisplay}</code>
          <Button variant="ghost" size="sm" onClick={() => setShowToken((v) => !v)}>
            {showHideLabel}
          </Button>
          <Button variant="ghost" size="sm" onClick={() => copyToClipboard(status.token)}>
            {t('remoteAccess.copyToken')}
          </Button>
        </div>
        <Button variant="outline" size="sm" onClick={() => void handleRegenerateToken()}>
          {t('remoteAccess.regenerateToken')}
        </Button>
        <p className="text-xs text-muted-foreground">{t('remoteAccess.tokenDescription')}</p>
      </div>
    </div>
  );
};
