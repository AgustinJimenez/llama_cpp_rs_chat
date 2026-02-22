import React from 'react';

export interface GpuLayersSectionProps {
  gpuLayers: number;
  onGpuLayersChange: (layers: number) => void;
  maxLayers: number;
}

export const GpuLayersSection: React.FC<GpuLayersSectionProps> = ({
  gpuLayers,
  onGpuLayersChange,
  maxLayers
}) => (
  <div className="space-y-1">
    <div className="flex justify-between items-center">
      <span className="text-sm font-medium">GPU Layers (CUDA)</span>
      <span className="text-sm font-mono text-foreground" data-testid="gpu-layers-display">
        {gpuLayers || 0} / {maxLayers}
      </span>
    </div>

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
  </div>
);
