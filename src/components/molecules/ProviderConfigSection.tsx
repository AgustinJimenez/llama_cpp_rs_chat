/**
 * Dynamic provider parameter configuration.
 *
 * Renders controls (sliders, toggles, selects) based on the provider's
 * parameter schema from providerParams.ts. Users can add optional params
 * and all values are persisted via ModelContext.
 */
import { Plus, X } from 'lucide-react';
import { useCallback, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';

import {
  getProviderDefaults,
  getProviderParams,
  type ParamSchema,
} from '../../config/providerParams';
import { useModelContext } from '../../contexts/ModelContext';

interface ProviderConfigSectionProps {
  providerId: string;
}

const ParamControl = ({
  schema,
  value,
  onChange,
  onRemove,
}: {
  schema: ParamSchema;
  value: unknown;
  onChange: (key: string, val: unknown) => void;
  onRemove?: () => void;
}) => {
  if (schema.type === 'select') {
    return (
      <div className="flex items-center gap-2">
        <label className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider min-w-[100px] flex-shrink-0">
          {schema.label}
        </label>
        <select
          value={String(value ?? schema.default ?? '')}
          onChange={(e) => onChange(schema.key, e.target.value)}
          className="flex-1 px-2 py-1 text-xs bg-muted border border-border rounded-md text-foreground focus:outline-none focus:border-primary"
        >
          {(schema.options ?? []).map((opt) => (
            <option key={opt.value} value={opt.value}>
              {opt.label}
            </option>
          ))}
        </select>
        {onRemove != null && (
          <button
            onClick={onRemove}
            className="p-0.5 text-muted-foreground hover:text-red-400 transition-colors"
            title="Remove"
          >
            <X className="h-3 w-3" />
          </button>
        )}
      </div>
    );
  }

  if (schema.type === 'boolean') {
    return (
      <div className="flex items-center gap-2">
        <label className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider min-w-[100px] flex-shrink-0">
          {schema.label}
        </label>
        <input
          type="checkbox"
          checked={Boolean(value ?? schema.default)}
          onChange={(e) => onChange(schema.key, e.target.checked)}
          className="rounded border-border"
        />
        {onRemove != null && (
          <button
            onClick={onRemove}
            className="p-0.5 text-muted-foreground hover:text-red-400 transition-colors ml-auto"
            title="Remove"
          >
            <X className="h-3 w-3" />
          </button>
        )}
      </div>
    );
  }

  if (schema.type === 'number') {
    const numVal = typeof value === 'number' ? value : ((schema.default as number) ?? 0);
    return (
      <div className="flex items-center gap-2">
        <label className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider min-w-[100px] flex-shrink-0">
          {schema.label}
        </label>
        <input
          type="range"
          min={schema.min ?? 0}
          max={schema.max ?? 100}
          step={schema.step ?? 1}
          value={numVal}
          onChange={(e) => onChange(schema.key, parseFloat(e.target.value))}
          className="flex-1 accent-primary h-1"
        />
        <span className="text-xs text-muted-foreground tabular-nums w-[50px] text-right">
          {numVal}
        </span>
        {onRemove != null && (
          <button
            onClick={onRemove}
            className="p-0.5 text-muted-foreground hover:text-red-400 transition-colors"
            title="Remove"
          >
            <X className="h-3 w-3" />
          </button>
        )}
      </div>
    );
  }

  // string type
  return (
    <div className="flex items-center gap-2">
      <label className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider min-w-[100px] flex-shrink-0">
        {schema.label}
      </label>
      <input
        type="text"
        value={String(value ?? schema.default ?? '')}
        onChange={(e) => onChange(schema.key, e.target.value)}
        className="flex-1 px-2 py-1 text-xs bg-muted border border-border rounded-md text-foreground focus:outline-none focus:border-primary"
      />
      {onRemove != null && (
        <button
          onClick={onRemove}
          className="p-0.5 text-muted-foreground hover:text-red-400 transition-colors"
          title="Remove"
        >
          <X className="h-3 w-3" />
        </button>
      )}
    </div>
  );
};

export const ProviderConfigSection = ({ providerId }: ProviderConfigSectionProps) => {
  const { t } = useTranslation();
  const { providerParams, setProviderParamsFor } = useModelContext();
  const allSchemas = useMemo(() => getProviderParams(providerId), [providerId]);
  const defaults = useMemo(() => getProviderDefaults(providerId), [providerId]);

  const currentParams = useMemo(
    () => providerParams[providerId] ?? {},
    [providerParams, providerId],
  );
  const mergedParams = useMemo(
    () => ({ ...defaults, ...currentParams }),
    [defaults, currentParams],
  );

  // Track which optional params are enabled
  const [enabledOptional, setEnabledOptional] = useState<Set<string>>(() => {
    const set = new Set<string>();
    for (const s of allSchemas) {
      if (s.optional && currentParams[s.key] !== undefined) set.add(s.key);
    }
    return set;
  });

  const requiredSchemas = allSchemas.filter((s) => !s.optional);
  const optionalSchemas = allSchemas.filter((s) => s.optional);
  const availableOptional = optionalSchemas.filter((s) => !enabledOptional.has(s.key));
  const activeOptional = optionalSchemas.filter((s) => enabledOptional.has(s.key));

  const [addMenuOpen, setAddMenuOpen] = useState(false);

  const handleChange = useCallback(
    (key: string, val: unknown) => {
      setProviderParamsFor(providerId, { ...currentParams, [key]: val });
    },
    [providerId, currentParams, setProviderParamsFor],
  );

  const handleRemoveOptional = useCallback(
    (key: string) => {
      const next = { ...currentParams };
      delete next[key];
      setProviderParamsFor(providerId, next);
      setEnabledOptional((prev) => {
        const s = new Set(prev);
        s.delete(key);
        return s;
      });
    },
    [providerId, currentParams, setProviderParamsFor],
  );

  const handleAddOptional = useCallback(
    (key: string) => {
      const schema = optionalSchemas.find((s) => s.key === key);
      if (schema?.default !== undefined) {
        setProviderParamsFor(providerId, { ...currentParams, [key]: schema.default });
      }
      setEnabledOptional((prev) => new Set(prev).add(key));
      setAddMenuOpen(false);
    },
    [providerId, currentParams, optionalSchemas, setProviderParamsFor],
  );

  if (allSchemas.length === 0) return null;

  return (
    <div className="space-y-2 pt-1 border-t border-border/40">
      <p className="text-[10px] font-medium text-muted-foreground uppercase tracking-wider">
        {t('provider.parameters')}
      </p>

      {requiredSchemas.map((schema) => (
        <ParamControl
          key={schema.key}
          schema={schema}
          value={mergedParams[schema.key]}
          onChange={handleChange}
        />
      ))}

      {activeOptional.map((schema) => (
        <ParamControl
          key={schema.key}
          schema={schema}
          value={mergedParams[schema.key]}
          onChange={handleChange}
          onRemove={() => handleRemoveOptional(schema.key)}
        />
      ))}

      {availableOptional.length > 0 && (
        <div className="relative">
          <button
            onClick={() => setAddMenuOpen((v) => !v)}
            className="flex items-center gap-1 text-[10px] text-muted-foreground hover:text-foreground transition-colors"
          >
            <Plus className="h-3 w-3" />
            {t('provider.addParameter')}
          </button>
          {addMenuOpen ? (
            <div className="absolute left-0 top-6 z-10 bg-card border border-border rounded-md shadow-lg py-1 min-w-[180px]">
              {availableOptional.map((s) => (
                <button
                  key={s.key}
                  onClick={() => handleAddOptional(s.key)}
                  className="w-full text-left px-3 py-1.5 text-xs text-foreground hover:bg-muted transition-colors"
                >
                  {s.label}
                  {s.description ? (
                    <span className="block text-[10px] text-muted-foreground">{s.description}</span>
                  ) : null}
                </button>
              ))}
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
};
