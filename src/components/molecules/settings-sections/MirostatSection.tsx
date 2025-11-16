import React from 'react';
import { Slider } from '../../atoms/slider';
import { Card, CardContent, CardHeader, CardTitle } from '../../atoms/card';

interface MirostatSectionProps {
  tauValue: number;
  etaValue: number;
  onTauChange: (value: number) => void;
  onEtaChange: (value: number) => void;
}

export const MirostatSection: React.FC<MirostatSectionProps> = ({
  tauValue,
  etaValue,
  onTauChange,
  onEtaChange
}) => {
  return (
    <div className="grid grid-cols-2 gap-4">
      <Card>
        <CardHeader>
          <CardTitle className="text-sm flex justify-between">
            Mirostat Tau
            <span className="font-mono text-slate-400">{tauValue.toFixed(1)}</span>
          </CardTitle>
        </CardHeader>
        <CardContent>
          <Slider
            value={[tauValue]}
            onValueChange={([value]) => onTauChange(value)}
            max={10}
            min={0}
            step={0.1}
            className="w-full"
          />
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm flex justify-between">
            Mirostat Eta
            <span className="font-mono text-slate-400">{etaValue.toFixed(2)}</span>
          </CardTitle>
        </CardHeader>
        <CardContent>
          <Slider
            value={[etaValue]}
            onValueChange={([value]) => onEtaChange(value)}
            max={1}
            min={0}
            step={0.01}
            className="w-full"
          />
        </CardContent>
      </Card>
    </div>
  );
};
