import React from 'react';
import { Button } from '@/components/ui/button';
import { Card, CardContent, CardHeader, CardTitle } from '@/components/ui/card';

interface PresetSectionProps {
  onApplyIBMPreset: () => void;
}

export const PresetSection: React.FC<PresetSectionProps> = ({
  onApplyIBMPreset
}) => {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm">IBM Recommended Preset</CardTitle>
      </CardHeader>
      <CardContent>
        <button
          onClick={onApplyIBMPreset}
          className="flat-button bg-flat-red text-white w-full"
        >
          Apply IBM Settings (ChainFull, temp: 0.7, top_p: 0.95, top_k: 20)
        </button>
      </CardContent>
    </Card>
  );
};
