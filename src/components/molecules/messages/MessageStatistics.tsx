import { Gauge } from 'lucide-react';

interface MessageStatisticsProps {
  timings: {
    promptTokPerSec?: number;
    genTokPerSec?: number;
  };
}

export function MessageStatistics({ timings }: MessageStatisticsProps) {
  const { genTokPerSec } = timings;

  if (!genTokPerSec) return null;

  return (
    <div className="flex items-center gap-3 mt-2 text-xs text-muted-foreground font-mono">
      <span className="inline-flex items-center gap-1" title="Generation speed">
        <Gauge className="h-3 w-3" />
        {genTokPerSec.toFixed(1)} tok/s
      </span>
    </div>
  );
}
