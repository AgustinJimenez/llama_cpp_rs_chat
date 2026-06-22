import { RotateCcw, Plus, Trash2 } from 'lucide-react';
import React, { useState } from 'react';
import { useTranslation } from 'react-i18next';

import type { TagPair } from '@/types';

interface TagPairsSectionProps {
  tagPairs: TagPair[];
  detectedTagPairs?: TagPair[];
  onTagPairsChange: (pairs: TagPair[]) => void;
}

const TagPairRow: React.FC<{
  pair: TagPair;
  onUpdate: (field: keyof TagPair, value: string | boolean) => void;
  onDelete: () => void;
}> = ({ pair, onUpdate, onDelete }) => {
  const { t } = useTranslation();
  return (
    <div className="group flex items-center gap-1.5">
      <input
        type="checkbox"
        className="size-3 rounded border-border accent-blue-500"
        checked={pair.enabled}
        onChange={(e) => onUpdate('enabled', e.target.checked)}
      />
      <span
        className={`w-24 shrink-0 truncate text-[10px] ${pair.enabled ? 'text-foreground' : 'text-muted-foreground'}`}
      >
        {pair.name}
      </span>
      <input
        type="text"
        className="h-5 flex-1 rounded border border-input bg-background px-1 font-mono text-[10px] focus:outline-none focus:ring-1 focus:ring-ring"
        value={pair.open_tag}
        onChange={(e) => onUpdate('open_tag', e.target.value)}
      />
      <input
        type="text"
        className="h-5 flex-1 rounded border border-input bg-background px-1 font-mono text-[10px] focus:outline-none focus:ring-1 focus:ring-ring"
        placeholder={t('modelConfig.closeTagPlaceholder')}
        value={pair.close_tag}
        onChange={(e) => onUpdate('close_tag', e.target.value)}
      />
      <button
        type="button"
        className="shrink-0 text-muted-foreground opacity-0 transition-colors hover:text-red-400 group-hover:opacity-100"
        title={t('modelConfig.deleteTagPair')}
        onClick={onDelete}
      >
        <Trash2 className="size-3" />
      </button>
    </div>
  );
};

const AddPairForm: React.FC<{
  existingNames: Set<string>;
  onAdd: (pair: TagPair) => void;
  onCancel: () => void;
}> = ({ existingNames, onAdd, onCancel }) => {
  const { t } = useTranslation();
  const [pair, setPair] = useState<TagPair>({
    category: 'Custom',
    name: '',
    open_tag: '',
    close_tag: '',
    enabled: true,
  });

  const nameTrimmed = pair.name.trim();
  const isDuplicate = nameTrimmed !== '' && existingNames.has(nameTrimmed.toLowerCase());
  const canAdd = nameTrimmed !== '' && pair.open_tag.trim() !== '' && !isDuplicate;

  const handleAdd = () => {
    if (!canAdd) return;
    onAdd(pair);
    onCancel();
  };

  return (
    <div className="space-y-1">
      <div className="flex items-center gap-1.5 rounded bg-muted/50 p-1.5">
        <input
          type="text"
          className={`h-6 w-20 rounded border bg-background px-1.5 text-[10px] focus:outline-none focus:ring-1 focus:ring-ring ${isDuplicate ? 'border-red-500' : 'border-input'}`}
          placeholder={t('modelConfig.namePlaceholder')}
          value={pair.name}
          onChange={(e) => setPair((p) => ({ ...p, name: e.target.value }))}
        />
        <input
          type="text"
          className="h-6 flex-1 rounded border border-input bg-background px-1.5 font-mono text-[10px] focus:outline-none focus:ring-1 focus:ring-ring"
          placeholder={t('modelConfig.openTagPlaceholder')}
          value={pair.open_tag}
          onChange={(e) => setPair((p) => ({ ...p, open_tag: e.target.value }))}
        />
        <input
          type="text"
          className="h-6 flex-1 rounded border border-input bg-background px-1.5 font-mono text-[10px] focus:outline-none focus:ring-1 focus:ring-ring"
          placeholder={t('modelConfig.closeTagPlaceholder')}
          value={pair.close_tag}
          onChange={(e) => setPair((p) => ({ ...p, close_tag: e.target.value }))}
        />
        <button
          type="button"
          className="h-6 rounded bg-blue-600 px-2 text-[10px] text-white transition-colors hover:bg-blue-500 disabled:opacity-50"
          disabled={!canAdd}
          onClick={handleAdd}
        >
          {t('common.add')}
        </button>
      </div>
      {!!isDuplicate && (
        <p className="pl-1 text-[10px] text-red-400">
          {t('modelConfig.duplicateName', { name: nameTrimmed })}
        </p>
      )}
    </div>
  );
};

export const TagPairsSection: React.FC<TagPairsSectionProps> = ({
  tagPairs,
  detectedTagPairs,
  onTagPairsChange,
}) => {
  const { t } = useTranslation();
  const [showAdd, setShowAdd] = useState(false);

  return (
    <div className="space-y-2 rounded-md border border-border px-3 py-2">
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium">
          {t('modelConfig.tagPairs', { count: tagPairs.length })}
        </span>
        {!!detectedTagPairs?.length && (
          <button
            type="button"
            className="flex items-center gap-1 text-[10px] text-muted-foreground transition-colors hover:text-foreground"
            title={t('modelConfig.resetTagPairs')}
            onClick={() => onTagPairsChange([...detectedTagPairs])}
          >
            <RotateCcw className="size-3" /> {t('modelConfig.reset')}
          </button>
        )}
      </div>

      <div className="space-y-0.5">
        {tagPairs.map((pair, idx) => (
          <TagPairRow
            key={`${pair.category}-${pair.name}-${pair.open_tag}`}
            pair={pair}
            onUpdate={(field, value) => {
              const u = [...tagPairs];
              u[idx] = { ...u[idx], [field]: value };
              onTagPairsChange(u);
            }}
            onDelete={() => onTagPairsChange(tagPairs.filter((_, i) => i !== idx))}
          />
        ))}
      </div>

      {tagPairs.length === 0 && (
        <p className="text-[10px] italic text-muted-foreground">{t('modelConfig.noTagPairs')}</p>
      )}

      {!!showAdd && (
        <AddPairForm
          existingNames={new Set(tagPairs.map((p) => p.name.trim().toLowerCase()))}
          onAdd={(p) => onTagPairsChange([...tagPairs, p])}
          onCancel={() => setShowAdd(false)}
        />
      )}
      {!showAdd && (
        <button
          type="button"
          className="flex items-center gap-1 text-[10px] text-blue-400 transition-colors hover:text-blue-300"
          onClick={() => setShowAdd(true)}
        >
          <Plus className="size-3" /> {t('common.add')}
        </button>
      )}
    </div>
  );
};
