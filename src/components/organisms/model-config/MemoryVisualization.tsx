import React from 'react';
import { AlertTriangle, Info } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '../../atoms/card';

export interface MemoryBreakdown {
  // VRAM breakdown (GPU memory)
  vram: {
    total: number;           // Total VRAM available (GB)
    modelGpu: number;        // Model layers on GPU (GB)
    kvCache: number;         // KV cache for context (GB)
    overhead: number;        // System overhead/buffers (GB)
    available: number;       // Remaining free VRAM (GB)
    overcommitted: boolean;  // true if total usage > available
  };
  // RAM breakdown (System memory)
  ram: {
    total: number;           // Total RAM available (GB)
    modelCpu: number;        // Model layers on CPU (GB)
    available: number;       // Remaining free RAM (GB)
    overcommitted: boolean;  // true if total usage > available
  };
}

interface MemoryVisualizationProps {
  memory: MemoryBreakdown;
}

export const MemoryVisualization: React.FC<MemoryVisualizationProps> = ({ memory }) => {
  // Calculate percentages for VRAM
  const vramUsed = memory.vram.modelGpu + memory.vram.kvCache + memory.vram.overhead;
  const vramModelPct = (memory.vram.modelGpu / memory.vram.total) * 100;
  const vramCachePct = (memory.vram.kvCache / memory.vram.total) * 100;
  const vramOverheadPct = (memory.vram.overhead / memory.vram.total) * 100;
  const vramAvailablePct = (memory.vram.available / memory.vram.total) * 100;
  const vramUtilization = (vramUsed / memory.vram.total) * 100;

  // Calculate percentages for RAM
  const ramUsed = memory.ram.modelCpu;
  const ramModelPct = (memory.ram.modelCpu / memory.ram.total) * 100;
  const ramAvailablePct = (memory.ram.available / memory.ram.total) * 100;
  const ramUtilization = (ramUsed / memory.ram.total) * 100;

  // Determine status colors
  const getStatusColor = (utilization: number, overcommitted: boolean) => {
    if (overcommitted) return 'text-red-600 dark:text-red-400';
    if (utilization > 90) return 'text-orange-600 dark:text-orange-400';
    if (utilization > 75) return 'text-yellow-600 dark:text-yellow-400';
    return 'text-green-600 dark:text-green-400';
  };

  const vramStatusColor = getStatusColor(vramUtilization, memory.vram.overcommitted);
  const ramStatusColor = getStatusColor(ramUtilization, memory.ram.overcommitted);

  return (
    <Card className="border-zinc-800 bg-zinc-900">
      <CardHeader className="pb-3">
        <CardTitle className="text-base font-medium flex items-center gap-2">
          <Info className="h-4 w-4" />
          Memory Usage Estimate
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* VRAM Section */}
        <div className="space-y-2">
          <div className="flex justify-between items-baseline">
            <span className="text-sm font-medium text-zinc-300">GPU Memory (VRAM)</span>
            <span className={`text-sm font-mono ${vramStatusColor}`}>
              {vramUsed.toFixed(2)} / {memory.vram.total.toFixed(2)} GB ({vramUtilization.toFixed(1)}%)
            </span>
          </div>

          {/* VRAM Progress Bar */}
          <div className="h-8 bg-zinc-800 rounded-md overflow-hidden flex relative">
            {/* Model GPU segment */}
            {memory.vram.modelGpu > 0 && (
              <div
                className="bg-primary flex items-center justify-center text-xs text-white font-medium transition-all duration-300"
                style={{ width: `${Math.min(vramModelPct, 100)}%` }}
                title={`Model (GPU): ${memory.vram.modelGpu.toFixed(2)} GB`}
              >
                {vramModelPct > 8 && `${memory.vram.modelGpu.toFixed(1)}G`}
              </div>
            )}

            {/* KV Cache segment */}
            {memory.vram.kvCache > 0 && (
              <div
                className="bg-purple-600 flex items-center justify-center text-xs text-white font-medium transition-all duration-300"
                style={{ width: `${Math.min(vramCachePct, 100 - vramModelPct)}%` }}
                title={`KV Cache: ${memory.vram.kvCache.toFixed(2)} GB`}
              >
                {vramCachePct > 8 && `${memory.vram.kvCache.toFixed(1)}G`}
              </div>
            )}

            {/* Overhead segment */}
            {memory.vram.overhead > 0 && (
              <div
                className="bg-zinc-600 flex items-center justify-center text-xs text-white font-medium transition-all duration-300"
                style={{ width: `${Math.min(vramOverheadPct, 100 - vramModelPct - vramCachePct)}%` }}
                title={`Overhead: ${memory.vram.overhead.toFixed(2)} GB`}
              >
                {vramOverheadPct > 6 && `${memory.vram.overhead.toFixed(1)}G`}
              </div>
            )}

            {/* Available segment */}
            {memory.vram.available > 0 && !memory.vram.overcommitted && (
              <div
                className="bg-zinc-700/50 flex items-center justify-center text-xs text-zinc-400 font-medium transition-all duration-300"
                style={{ width: `${vramAvailablePct}%` }}
                title={`Available: ${memory.vram.available.toFixed(2)} GB`}
              >
                {vramAvailablePct > 10 && `${memory.vram.available.toFixed(1)}G`}
              </div>
            )}

            {/* Overcommitted indicator */}
            {memory.vram.overcommitted && (
              <div className="absolute inset-0 bg-red-600/20 border-2 border-red-600 flex items-center justify-center">
                <span className="text-xs text-black font-bold">OVERCOMMITTED</span>
              </div>
            )}
          </div>

          {/* VRAM Legend */}
          <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-xs text-zinc-400">
            <div className="flex items-center gap-2">
              <div className="w-3 h-3 bg-primary rounded-sm"></div>
              <span>Model (GPU): {memory.vram.modelGpu.toFixed(2)} GB</span>
            </div>
            <div className="flex items-center gap-2">
              <div className="w-3 h-3 bg-purple-600 rounded-sm"></div>
              <span>KV Cache: {memory.vram.kvCache.toFixed(2)} GB</span>
            </div>
            <div className="flex items-center gap-2">
              <div className="w-3 h-3 bg-zinc-600 rounded-sm"></div>
              <span>Overhead: {memory.vram.overhead.toFixed(2)} GB</span>
            </div>
            <div className="flex items-center gap-2">
              <div className="w-3 h-3 bg-zinc-700/50 border border-zinc-600 rounded-sm"></div>
              <span>Available: {memory.vram.available.toFixed(2)} GB</span>
            </div>
          </div>
        </div>

        {/* RAM Section */}
        <div className="space-y-2">
          <div className="flex justify-between items-baseline">
            <span className="text-sm font-medium text-zinc-300">System Memory (RAM)</span>
            <span className={`text-sm font-mono ${ramStatusColor}`}>
              {ramUsed.toFixed(2)} / {memory.ram.total.toFixed(2)} GB ({ramUtilization.toFixed(1)}%)
            </span>
          </div>

          {/* RAM Progress Bar */}
          <div className="h-8 bg-zinc-800 rounded-md overflow-hidden flex relative">
            {/* Model CPU segment */}
            {memory.ram.modelCpu > 0 && (
              <div
                className="bg-cyan-600 flex items-center justify-center text-xs text-white font-medium transition-all duration-300"
                style={{ width: `${Math.min(ramModelPct, 100)}%` }}
                title={`Model (CPU): ${memory.ram.modelCpu.toFixed(2)} GB`}
              >
                {ramModelPct > 8 && `${memory.ram.modelCpu.toFixed(1)}G`}
              </div>
            )}

            {/* Available segment */}
            {memory.ram.available > 0 && !memory.ram.overcommitted && (
              <div
                className="bg-zinc-700/50 flex items-center justify-center text-xs text-zinc-400 font-medium transition-all duration-300"
                style={{ width: `${ramAvailablePct}%` }}
                title={`Available: ${memory.ram.available.toFixed(2)} GB`}
              >
                {ramAvailablePct > 10 && `${memory.ram.available.toFixed(1)}G`}
              </div>
            )}

            {/* Overcommitted indicator */}
            {memory.ram.overcommitted && (
              <div className="absolute inset-0 bg-red-600/20 border-2 border-red-600 flex items-center justify-center">
                <span className="text-xs text-black font-bold">OVERCOMMITTED</span>
              </div>
            )}
          </div>

          {/* RAM Legend */}
          <div className="grid grid-cols-2 gap-x-4 gap-y-1 text-xs text-zinc-400">
            <div className="flex items-center gap-2">
              <div className="w-3 h-3 bg-cyan-600 rounded-sm"></div>
              <span>Model (CPU): {memory.ram.modelCpu.toFixed(2)} GB</span>
            </div>
            <div className="flex items-center gap-2">
              <div className="w-3 h-3 bg-zinc-700/50 border border-zinc-600 rounded-sm"></div>
              <span>Available: {memory.ram.available.toFixed(2)} GB</span>
            </div>
          </div>
        </div>

        {/* Warnings */}
        {(memory.vram.overcommitted || memory.ram.overcommitted) && (
          <div className="bg-red-900/20 border border-red-600 rounded-md p-3 flex items-start gap-2">
            <AlertTriangle className="h-5 w-5 text-red-600 flex-shrink-0 mt-0.5" />
            <div className="text-sm text-red-600 dark:text-red-400">
              <p className="font-semibold mb-1">Memory Overcommitted!</p>
              {memory.vram.overcommitted && (
                <p className="text-xs">
                  VRAM usage exceeds available GPU memory. Reduce GPU layers or context size.
                </p>
              )}
              {memory.ram.overcommitted && (
                <p className="text-xs">
                  RAM usage exceeds available system memory. Increase GPU layers or reduce model size.
                </p>
              )}
            </div>
          </div>
        )}

        {/* Helpful info */}
        {!memory.vram.overcommitted && !memory.ram.overcommitted && vramUtilization > 85 && (
          <div className="bg-yellow-900/20 border border-yellow-600 rounded-md p-3 flex items-start gap-2">
            <Info className="h-5 w-5 text-yellow-600 flex-shrink-0 mt-0.5" />
            <div className="text-sm text-yellow-600 dark:text-yellow-400">
              <p className="text-xs">
                VRAM usage is high ({vramUtilization.toFixed(1)}%). Consider reducing context size if you experience crashes.
              </p>
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
};
