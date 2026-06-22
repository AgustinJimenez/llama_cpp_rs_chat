import type { SamplerType } from '@/types';

export const SAMPLER_OPTIONS: SamplerType[] = [
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
  'ChainFull',
];
