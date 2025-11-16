import React from 'react';
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '../../atoms/select';
import { Card, CardContent, CardHeader, CardTitle } from '../../atoms/card';
import type { SamplerType } from '../../../types';

const SAMPLER_OPTIONS: SamplerType[] = [
  'Greedy',
  'Temperature',
  'Mirostat',
  'TopP',
  'TopK',
  'Typical',
  'MinP',
  'TempExt',
  'ChainTempTopP',
  'ChainTempTopK',
  'ChainFull'
];

const SAMPLER_DESCRIPTIONS: Record<SamplerType, string> = {
  'Greedy': 'Deterministic selection - always picks the most likely token',
  'Temperature': 'Controls randomness in text generation',
  'Mirostat': 'Advanced perplexity-based sampling method',
  'TopP': 'Nucleus sampling - considers top tokens by cumulative probability',
  'TopK': 'Considers only the top K most likely tokens',
  'Typical': 'Selects tokens with typical information content',
  'MinP': 'Minimum probability threshold sampling',
  'TempExt': 'Extended temperature sampling with enhanced control',
  'ChainTempTopP': 'Chains Temperature and Top-P sampling methods',
  'ChainTempTopK': 'Chains Temperature and Top-K sampling methods',
  'ChainFull': 'Full chain sampling (IBM recommended for best results)'
};

interface SamplerTypeSectionProps {
  samplerType: string;
  onSamplerTypeChange: (type: string) => void;
}

export const SamplerTypeSection: React.FC<SamplerTypeSectionProps> = ({
  samplerType,
  onSamplerTypeChange
}) => {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-sm">Sampler Type</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <Select
          value={samplerType}
          onValueChange={onSamplerTypeChange}
        >
          <SelectTrigger>
            <SelectValue placeholder="Select a sampler type" />
          </SelectTrigger>
          <SelectContent>
            {SAMPLER_OPTIONS.map(option => (
              <SelectItem key={option} value={option}>
                {option}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
        <p className="text-xs text-muted-foreground">
          {SAMPLER_DESCRIPTIONS[samplerType as SamplerType] || 'Select a sampler type for more information'}
        </p>
      </CardContent>
    </Card>
  );
};
