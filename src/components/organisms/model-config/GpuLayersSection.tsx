import React, { useEffect, useState } from 'react';

import { getAvailableBackends, type BackendInfo } from '../../../utils/tauriCommands';

const GPU_SETUP_URL = 'https://github.com/AgustinJimenez/llama_cpp_rs_chat/releases/tag/backends';

export interface GpuLayersSectionProps {
  gpuLayers: number;
  onGpuLayersChange: (layers: number) => void;
  maxLayers: number;
}

export const GpuLayersSection: React.FC<GpuLayersSectionProps> = ({
  gpuLayers,
  onGpuLayersChange,
  maxLayers,
}) => {
  const [backends, setBackends] = useState<BackendInfo[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [nvidiaDetected, setNvidiaDetected] = useState(false);
  const [cudaLoaded, setCudaLoaded] = useState(false);

  useEffect(() => {
    let cancelled = false;
    getAvailableBackends()
      .then((resp) => {
        if (!cancelled) {
          setBackends(resp.backends);
          setNvidiaDetected(resp.nvidia_gpu_detected ?? false);
          setCudaLoaded(resp.cuda_backend_loaded ?? false);
          setLoaded(true);
        }
      })
      .catch(() => {
        if (!cancelled) setLoaded(true);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const gpuBackend = backends.find((b) => b.available && b.name !== 'CPU' && b.name !== 'BLAS');
  const gpuLabel = gpuBackend ? `GPU Layers (${gpuBackend.name})` : 'GPU Layers';
  const hasGpu = !loaded || !!gpuBackend;
  const showCudaBanner = loaded && nvidiaDetected && !cudaLoaded;

  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium">{gpuLabel}</span>
          {!!loaded && (
            <div className="flex gap-1">
              {backends.map((b) => (
                <span
                  key={b.name}
                  className={`rounded-full px-1.5 py-0.5 text-[10px] font-medium ${
                    b.available
                      ? 'border border-green-500/30 bg-green-500/15 text-green-400'
                      : 'border border-border bg-muted text-muted-foreground'
                  }`}
                >
                  {b.name}
                </span>
              ))}
            </div>
          )}
        </div>
        <span className="font-mono text-sm text-foreground" data-testid="gpu-layers-display">
          {gpuLayers || 0} / {maxLayers}
        </span>
      </div>

      {!!hasGpu && (
        <>
          <input
            type="range"
            data-testid="gpu-layers-slider"
            min={0}
            max={maxLayers}
            step={1}
            value={gpuLayers || 0}
            onChange={(e) => onGpuLayersChange(parseInt(e.target.value))}
            className="w-full cursor-pointer accent-[hsl(var(--primary))]"
          />
          <p className="text-xs text-muted-foreground">
            Number of model layers to offload to GPU. Higher values = faster inference but more VRAM
            usage. 0 = CPU only. Model has ~{maxLayers} layers total.
          </p>
        </>
      )}
      {!hasGpu && (
        <p className="text-xs text-amber-400">
          No GPU backend detected. All layers will run on CPU.
        </p>
      )}

      {!!showCudaBanner && (
        <div className="mt-2 rounded-lg border border-blue-500/30 bg-blue-500/10 p-3">
          <p className="mb-2 text-xs text-blue-300">
            NVIDIA GPU detected but CUDA acceleration is not installed.
          </p>
          <a
            href={GPU_SETUP_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1.5 rounded bg-blue-600 px-3 py-1.5 text-xs font-medium text-white transition-colors hover:bg-blue-500"
          >
            GPU Setup Guide &amp; Downloads
          </a>
          <p className="mt-1.5 text-[10px] text-muted-foreground">
            Download the GPU files, place them next to the app, and restart.
          </p>
        </div>
      )}
    </div>
  );
};
