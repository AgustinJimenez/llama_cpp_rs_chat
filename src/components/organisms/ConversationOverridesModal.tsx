import { Loader2, RotateCcw } from 'lucide-react';
import { useEffect, useMemo, useState } from 'react';
import { toast } from 'react-hot-toast';

import type { SamplerConfig } from '../../types';
import {
  getConversationConfig,
  getConversationOverrides,
  saveConversationOverrides,
  type ConversationOverrides,
} from '../../utils/tauriCommands';
import { Button } from '../atoms/button';
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '../atoms/dialog';

type OverrideValue = string | number | boolean | null;

type OverrideField = {
  key: keyof SamplerConfig;
  label: string;
  kind: 'number' | 'boolean' | 'text' | 'select';
  min?: number;
  max?: number;
  step?: number;
  options?: string[];
};

const OVERRIDE_FIELDS: OverrideField[] = [
  { key: 'system_prompt', label: 'System prompt', kind: 'text' },
  {
    key: 'sampler_type',
    label: 'Sampler',
    kind: 'select',
    options: [
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
    ],
  },
  { key: 'temperature', label: 'Temperature', kind: 'number', min: 0, max: 2, step: 0.05 },
  { key: 'top_p', label: 'Top P', kind: 'number', min: 0, max: 1, step: 0.05 },
  { key: 'top_k', label: 'Top K', kind: 'number', min: 0, max: 500, step: 1 },
  { key: 'repeat_penalty', label: 'Repeat penalty', kind: 'number', min: 1, max: 2, step: 0.05 },
  { key: 'context_size', label: 'Context size', kind: 'number', min: 512, max: 1048576, step: 512 },
  {
    key: 'cache_type_k',
    label: 'K cache',
    kind: 'select',
    options: ['f16', 'q8_0', 'q4_0', 'turbo4', 'turbo3', 'turbo2'],
  },
  {
    key: 'cache_type_v',
    label: 'V cache',
    kind: 'select',
    options: ['f16', 'q8_0', 'q4_0', 'turbo4', 'turbo3', 'turbo2'],
  },
  { key: 'flash_attention', label: 'Flash attention', kind: 'boolean' },
  { key: 'thinking_mode', label: 'Thinking mode', kind: 'boolean' },
];

interface ConversationOverridesModalProps {
  isOpen: boolean;
  onClose: () => void;
  conversationId: string | null;
}

function getInitialValue(config: SamplerConfig | null, field: OverrideField): OverrideValue {
  const value = config?.[field.key];
  if (
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean' ||
    value === null
  ) {
    return value;
  }
  if (field.kind === 'boolean') return false;
  if (field.kind === 'number') return field.min ?? 0;
  return '';
}

// eslint-disable-next-line max-lines-per-function
export const ConversationOverridesModal = ({
  isOpen,
  onClose,
  conversationId,
}: ConversationOverridesModalProps) => {
  const [config, setConfig] = useState<SamplerConfig | null>(null);
  const [values, setValues] = useState<ConversationOverrides>({});
  const [enabled, setEnabled] = useState<Set<string>>(new Set());
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!isOpen || !conversationId) return;
    setLoading(true);
    Promise.all([getConversationConfig(conversationId), getConversationOverrides(conversationId)])
      .then(([effective, overrides]) => {
        setConfig(effective);
        setValues(overrides ?? {});
        setEnabled(new Set(Object.keys(overrides ?? {})));
      })
      .catch((error) => {
        toast.error(error instanceof Error ? error.message : 'Failed to load overrides');
      })
      .finally(() => setLoading(false));
  }, [conversationId, isOpen]);

  const activeCount = enabled.size;
  const canSave = !!conversationId && !loading && !saving;

  const payload = useMemo(() => {
    const next: ConversationOverrides = {};
    for (const key of enabled) {
      const field = OVERRIDE_FIELDS.find((candidate) => candidate.key === key);
      if (!field) continue;
      const value = values[key] ?? getInitialValue(config, field);
      next[key] = value;
    }
    return next;
  }, [config, enabled, values]);

  const toggleField = (key: string, checked: boolean) => {
    setEnabled((prev) => {
      const next = new Set(prev);
      if (checked) next.add(key);
      else next.delete(key);
      return next;
    });
  };

  const setValue = (key: string, value: string | number | boolean | null) => {
    setValues((prev) => ({ ...prev, [key]: value }));
  };

  const handleSave = async () => {
    if (!conversationId) return;
    setSaving(true);
    try {
      await saveConversationOverrides(conversationId, activeCount > 0 ? payload : null);
      toast.success('Conversation overrides saved');
      onClose();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Failed to save overrides');
    } finally {
      setSaving(false);
    }
  };

  const handleClear = async () => {
    if (!conversationId) return;
    setSaving(true);
    try {
      await saveConversationOverrides(conversationId, null);
      setEnabled(new Set());
      setValues({});
      toast.success('Conversation overrides cleared');
      onClose();
    } catch (error) {
      toast.error(error instanceof Error ? error.message : 'Failed to clear overrides');
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="max-w-2xl">
        <DialogHeader>
          <DialogTitle>Conversation Overrides</DialogTitle>
          <DialogDescription>
            Override selected agent settings for this conversation only.
          </DialogDescription>
        </DialogHeader>

        {!!loading && (
          <div className="flex items-center gap-2 py-8 text-sm text-muted-foreground">
            <Loader2 className="h-4 w-4 animate-spin" />
            Loading overrides...
          </div>
        )}

        {!loading && !conversationId && (
          <div className="py-8 text-sm text-muted-foreground">
            Start or select a conversation before editing overrides.
          </div>
        )}

        {!loading && !!conversationId && (
          <div className="max-h-[60vh] overflow-y-auto space-y-2 pr-1">
            {OVERRIDE_FIELDS.map((field) => {
              const key = String(field.key);
              const checked = enabled.has(key);
              const value = values[key] ?? getInitialValue(config, field);
              const fieldBodyClass = checked ? '' : 'opacity-45 pointer-events-none';
              const boolValue = value === true;
              const switchClass = boolValue ? 'bg-primary' : 'bg-muted';
              const knobClass = boolValue ? 'translate-x-6' : 'translate-x-1';
              return (
                <div
                  key={key}
                  className="grid grid-cols-[minmax(120px,160px)_1fr] gap-3 rounded-md border border-border p-3"
                >
                  <label className="flex items-center gap-2 text-sm text-foreground">
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={(event) => toggleField(key, event.target.checked)}
                    />
                    {field.label}
                  </label>
                  <div className={fieldBodyClass}>
                    {field.kind === 'number' && (
                      <input
                        type="number"
                        value={Number(value)}
                        min={field.min}
                        max={field.max}
                        step={field.step}
                        onChange={(event) => setValue(key, Number(event.target.value))}
                        className="h-8 w-full rounded border border-input bg-background px-2 text-sm"
                      />
                    )}
                    {field.kind === 'boolean' && (
                      <button
                        type="button"
                        role="switch"
                        aria-checked={boolValue}
                        onClick={() => setValue(key, !boolValue)}
                        className={`relative inline-flex h-6 w-11 items-center rounded-full transition-colors ${switchClass}`}
                      >
                        <span
                          className={`inline-block h-4 w-4 transform rounded-full bg-background transition-transform ${knobClass}`}
                        />
                      </button>
                    )}
                    {field.kind === 'select' && (
                      <select
                        value={String(value)}
                        onChange={(event) => setValue(key, event.target.value)}
                        className="h-8 w-full rounded border border-input bg-background px-2 text-sm"
                      >
                        {(field.options ?? []).map((option) => (
                          <option key={option} value={option}>
                            {option}
                          </option>
                        ))}
                      </select>
                    )}
                    {field.kind === 'text' && (
                      <textarea
                        value={String(value)}
                        onChange={(event) => setValue(key, event.target.value)}
                        className="min-h-24 w-full rounded border border-input bg-background px-2 py-1.5 text-sm"
                      />
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}

        <DialogFooter className="gap-2">
          <Button variant="outline" onClick={handleClear} disabled={!canSave || activeCount === 0}>
            <RotateCcw className="mr-2 h-4 w-4" />
            Clear
          </Button>
          <Button variant="outline" onClick={onClose}>
            Cancel
          </Button>
          <Button onClick={() => void handleSave()} disabled={!canSave}>
            {!!saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            Save Overrides
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
