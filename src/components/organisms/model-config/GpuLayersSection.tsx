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
      <div className="flex justify-between items-center">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium">{gpuLabel}</span>
          {loaded ? (
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
          ) : null}
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
            Number of model layers to offload to GPU. Higher values = faster inference but more VRAM
            usage. 0 = CPU only. Model has ~{maxLayers} layers total.
          </p>
        </>
      ) : (
        <p className="text-xs text-amber-400">
          No GPU backend detected. All layers will run on CPU.
        </p>
      )}

      {showCudaBanner ? (
        <div className="mt-2 p-3 rounded-lg bg-blue-500/10 border border-blue-500/30">
          <p className="text-xs text-blue-300 mb-2">
            NVIDIA GPU detected but CUDA acceleration is not installed.
          </p>
          <a
            href={GPU_SETUP_URL}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex items-center gap-1.5 text-xs font-medium px-3 py-1.5 rounded bg-blue-600 hover:bg-blue-500 text-white transition-colors"
          >
            GPU Setup Guide &amp; Downloads
          </a>
          <p className="text-[10px] text-muted-foreground mt-1.5">
            Download the GPU files, place them next to the app, and restart.
          </p>
        </div>
      ) : null}
    </div>
  );
};
