import React, { useState } from 'react';
import { RotateCcw, Plus, Trash2 } from 'lucide-react';
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
}> = ({ pair, onUpdate, onDelete }) => (
  <div className="flex items-center gap-1.5 group">
    <input
      type="checkbox"
      className="h-3 w-3 rounded border-zinc-600 accent-blue-500"
      checked={pair.enabled}
      onChange={e => onUpdate('enabled', e.target.checked)}
    />
    <span className={`text-[10px] w-24 truncate shrink-0 ${pair.enabled ? 'text-foreground' : 'text-zinc-500'}`}>
      {pair.name}
    </span>
    <input
      type="text"
      className="flex-1 h-5 rounded border border-input bg-background px-1 text-[10px] font-mono focus:outline-none focus:ring-1 focus:ring-ring"
      value={pair.open_tag}
      onChange={e => onUpdate('open_tag', e.target.value)}
    />
    <input
      type="text"
      className="flex-1 h-5 rounded border border-input bg-background px-1 text-[10px] font-mono focus:outline-none focus:ring-1 focus:ring-ring"
      placeholder="(none)"
      value={pair.close_tag}
      onChange={e => onUpdate('close_tag', e.target.value)}
    />
    <button
      type="button"
      className="text-zinc-600 hover:text-red-400 transition-colors opacity-0 group-hover:opacity-100 shrink-0"
      title="Delete tag pair"
      onClick={onDelete}
    >
      <Trash2 className="h-3 w-3" />
    </button>
  </div>
);

const AddPairForm: React.FC<{
  existingNames: Set<string>;
  onAdd: (pair: TagPair) => void;
  onCancel: () => void;
}> = ({ existingNames, onAdd, onCancel }) => {
  const [pair, setPair] = useState<TagPair>({
    category: 'Custom', name: '', open_tag: '', close_tag: '', enabled: true,
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
      <div className="flex items-center gap-1.5 bg-zinc-800/50 rounded p-1.5">
        <input type="text" className={`w-20 h-6 rounded border bg-background px-1.5 text-[10px] focus:outline-none focus:ring-1 focus:ring-ring ${isDuplicate ? 'border-red-500' : 'border-input'}`}
          placeholder="Name *" value={pair.name} onChange={e => setPair(p => ({ ...p, name: e.target.value }))} />
        <input type="text" className="flex-1 h-6 rounded border border-input bg-background px-1.5 text-[10px] font-mono focus:outline-none focus:ring-1 focus:ring-ring"
          placeholder="Open tag *" value={pair.open_tag} onChange={e => setPair(p => ({ ...p, open_tag: e.target.value }))} />
        <input type="text" className="flex-1 h-6 rounded border border-input bg-background px-1.5 text-[10px] font-mono focus:outline-none focus:ring-1 focus:ring-ring"
          placeholder="Close tag" value={pair.close_tag} onChange={e => setPair(p => ({ ...p, close_tag: e.target.value }))} />
        <button type="button" className="h-6 px-2 rounded bg-blue-600 hover:bg-blue-500 text-[10px] text-white transition-colors disabled:opacity-50"
          disabled={!canAdd} onClick={handleAdd}>Add</button>
      </div>
      {isDuplicate && <p className="text-[10px] text-red-400 pl-1">Name &quot;{nameTrimmed}&quot; already exists</p>}
    </div>
  );
};

export const TagPairsSection: React.FC<TagPairsSectionProps> = ({
  tagPairs, detectedTagPairs, onTagPairsChange,
}) => {
  const [showAdd, setShowAdd] = useState(false);

  return (
    <div className="rounded-md border border-zinc-700 px-3 py-2 space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium">Tag Pairs ({tagPairs.length})</span>
        {detectedTagPairs?.length ? (
          <button type="button" className="flex items-center gap-1 text-[10px] text-muted-foreground hover:text-foreground transition-colors"
            title="Reset to auto-detected tag pairs" onClick={() => onTagPairsChange([...detectedTagPairs])}>
            <RotateCcw className="h-3 w-3" /> Reset
          </button>
        ) : null}
      </div>

      <div className="space-y-0.5">
        {tagPairs.map((pair, idx) => (
          <TagPairRow key={`${pair.name}-${idx}`} pair={pair}
            onUpdate={(field, value) => { const u = [...tagPairs]; u[idx] = { ...u[idx], [field]: value }; onTagPairsChange(u); }}
            onDelete={() => onTagPairsChange(tagPairs.filter((_, i) => i !== idx))} />
        ))}
      </div>

      {tagPairs.length === 0 && (
        <p className="text-[10px] text-zinc-500 italic">No tag pairs configured. Select a model to auto-detect, or add manually.</p>
      )}

      {showAdd ? (
        <AddPairForm existingNames={new Set(tagPairs.map(p => p.name.trim().toLowerCase()))}
          onAdd={p => onTagPairsChange([...tagPairs, p])} onCancel={() => setShowAdd(false)} />
      ) : (
        <button type="button" className="flex items-center gap-1 text-[10px] text-blue-400 hover:text-blue-300 transition-colors"
          onClick={() => setShowAdd(true)}>
          <Plus className="h-3 w-3" /> Add
        </button>
      )}
    </div>
  );
};
