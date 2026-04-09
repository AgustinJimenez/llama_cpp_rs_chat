import React, { useEffect, useState } from 'react';
import { getAvailableBackends, type BackendInfo } from '../../../utils/tauriCommands';

export interface GpuLayersSectionProps {
  gpuLayers: number;
  onGpuLayersChange: (layers: number) => void;
  maxLayers: number;
}

export const GpuLayersSection: React.FC<GpuLayersSectionProps> = ({
  gpuLayers,
  onGpuLayersChange,
  maxLayers
}) => {
  const [backends, setBackends] = useState<BackendInfo[]>([]);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    getAvailableBackends()
      .then(({ backends: b }) => {
        if (!cancelled) {
          setBackends(b);
          setLoaded(true);
        }
      })
      .catch(() => {
        if (!cancelled) setLoaded(true);
      });
    return () => { cancelled = true; };
  }, []);

  const gpuBackend = backends.find(
    (b) => b.available && b.name !== 'CPU' && b.name !== 'BLAS'
  );
  const gpuLabel = gpuBackend ? `GPU Layers (${gpuBackend.name})` : 'GPU Layers';
  const hasGpu = !loaded || !!gpuBackend;

  return (
    <div className="space-y-1">
      <div className="flex justify-between items-center">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium">{gpuLabel}</span>
          {loaded && (
            <div className="flex gap-1">
              {backends.map((b) => (
                <span
                  key={b.name}
                  className={`text-[10px] px-1.5 py-0.5 rounded-full font-medium ${
                    b.available
                      ? 'bg-green-500/15 text-green-400 border border-green-500/30'
                      : 'bg-muted text-muted-foreground border border-border'
                  }`}
                >
                  {b.name}
                </span>
              ))}
            </div>
          )}
        </div>
        <span className="text-sm font-mono text-foreground" data-testid="gpu-layers-display">
          {gpuLayers || 0} / {maxLayers}
        </span>
      </div>

      {hasGpu ? (
        <>
          <input
            type="range"
            data-testid="gpu-layers-slider"
            min={0}
            max={maxLayers}
            step={1}
            value={gpuLayers || 0}
            onChange={(e) => onGpuLayersChange(parseInt(e.target.value))}
            className="w-full accent-[hsl(var(--primary))] cursor-pointer"
          />
          <p className="text-xs text-muted-foreground">
            Number of model layers to offload to GPU. Higher values = faster inference but more VRAM usage. 0 = CPU only. Model has ~{maxLayers} layers total.
          </p>
        </>
      ) : (
        <p className="text-xs text-amber-400">
          No GPU backend detected. All layers will run on CPU.
        </p>
      )}
    </div>
  );
};
