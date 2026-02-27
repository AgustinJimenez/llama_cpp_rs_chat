import { Gauge, BookOpenText } from 'lucide-react';

interface MessageStatisticsProps {
  timings: {
    promptTokPerSec?: number;
    genTokPerSec?: number;
  };
}

export function MessageStatistics({ timings }: MessageStatisticsProps) {
  const { promptTokPerSec, genTokPerSec } = timings;

  if (!genTokPerSec && !promptTokPerSec) return null;

  return (
    <div className="flex items-center gap-3 mt-2 text-xs text-muted-foreground font-mono">
      {promptTokPerSec ? (
        <span className="inline-flex items-center gap-1" title="Prompt eval speed">
          <BookOpenText className="h-3 w-3" />
          {promptTokPerSec.toFixed(1)} tok/s
        </span>
      ) : null}
      {genTokPerSec ? (
        <span className="inline-flex items-center gap-1" title="Generation speed">
          <Gauge className="h-3 w-3" />
          {genTokPerSec.toFixed(1)} tok/s
        </span>
      ) : null}
    </div>
  );
}
