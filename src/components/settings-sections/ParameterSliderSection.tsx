import React from 'react';
import { Slider } from '@/components/ui/slider';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';

interface ParameterSliderSectionProps {
  title: string;
  value: number;
  displayValue?: string;
  onValueChange: (value: number) => void;
  min: number;
  max: number;
  step: number;
  description: string;
}

export const ParameterSliderSection: React.FC<ParameterSliderSectionProps> = ({
  title,
  value,
  displayValue,
  onValueChange,
  min,
  max,
  step,
  description
}) => {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm flex justify-between">
          {title}
          <span className="font-mono text-slate-400">
            {displayValue !== undefined ? displayValue : value}
          </span>
        </CardTitle>
      </CardHeader>
      <CardContent>
        <Slider
          value={[value]}
          onValueChange={([val]) => onValueChange(val)}
          max={max}
          min={min}
          step={step}
          className="w-full"
        />
        <p className="text-xs text-muted-foreground mt-2">
          {description}
        </p>
      </CardContent>
    </Card>
  );
};
