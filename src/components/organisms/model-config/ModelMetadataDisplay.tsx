import { Eye } from 'lucide-react';
import React from 'react';

import { ParamGroup } from './ParamGroup';

import type { ModelMetadata } from '@/types';

export interface ModelMetadataDisplayProps {
  modelInfo: ModelMetadata;
}

const MetadataField = ({
  label,
  value,
  mono,
}: {
  label: string;
  value: React.ReactNode;
  mono?: boolean;
}) => (
  <p>
    <strong>{label}:</strong>{' '}
    <span className={`text-muted-foreground${mono ? 'font-mono' : ''}`}>{value}</span>
  </p>
);

const BasicInfoSection = ({ modelInfo }: { modelInfo: ModelMetadata }) => (
  <div className="space-y-1">
    <h4 className="mb-2 text-sm font-semibold">Basic Information</h4>
    <MetadataField label="File Name" value={modelInfo.name} />
    {!!modelInfo.general_name && (
      <MetadataField label="Model Name" value={modelInfo.general_name} />
    )}
    <MetadataField label="File Size" value={modelInfo.file_size} />
    <MetadataField label="Architecture" value={modelInfo.architecture} />
    <MetadataField label="Parameters" value={modelInfo.parameters} />
    <MetadataField label="Quantization" value={modelInfo.quantization} />
    {!!modelInfo.file_type && <MetadataField label="File Type" value={modelInfo.file_type} />}
    {!!modelInfo.quantization_version && (
      <MetadataField label="Quant Version" value={modelInfo.quantization_version} />
    )}
    {!!modelInfo.has_vision && (
      <p className="flex items-center gap-1.5">
        <Eye className="size-3.5 text-violet-400" />
        <strong>Vision:</strong> <span className="font-medium text-violet-400">Supported</span>
        <span className="text-muted-foreground">
          ({modelInfo.mmproj_files?.length} mmproj file
          {(modelInfo.mmproj_files?.length ?? 0) > 1 && 's'} found)
        </span>
      </p>
    )}
  </div>
);

const ModelDetailsSection = ({ modelInfo }: { modelInfo: ModelMetadata }) => {
  const hasDetails =
    modelInfo.description ||
    modelInfo.author ||
    modelInfo.organization ||
    modelInfo.version ||
    modelInfo.license;
  if (!hasDetails) return null;
  return (
    <div className="space-y-1 border-t pt-2">
      <h4 className="mb-2 text-sm font-semibold">Model Details</h4>
      {!!modelInfo.description && (
        <MetadataField label="Description" value={modelInfo.description} />
      )}
      {!!modelInfo.author && <MetadataField label="Author" value={modelInfo.author} />}
      {!!modelInfo.organization && (
        <MetadataField label="Organization" value={modelInfo.organization} />
      )}
      {!!modelInfo.version && <MetadataField label="Version" value={modelInfo.version} />}
      {!!modelInfo.license && <MetadataField label="License" value={modelInfo.license} />}
      {!!modelInfo.url && (
        <p>
          <strong>URL:</strong>{' '}
          <a
            href={modelInfo.url}
            target="_blank"
            rel="noopener noreferrer"
            className="break-all text-primary hover:underline"
          >
            {modelInfo.url}
          </a>
        </p>
      )}
      {!!modelInfo.repo_url && (
        <p>
          <strong>Repository:</strong>{' '}
          <a
            href={modelInfo.repo_url}
            target="_blank"
            rel="noopener noreferrer"
            className="break-all text-primary hover:underline"
          >
            {modelInfo.repo_url}
          </a>
        </p>
      )}
    </div>
  );
};

const ArchitectureSpecsSection = ({ modelInfo }: { modelInfo: ModelMetadata }) => {
  const hasSpecs =
    modelInfo.context_length ||
    modelInfo.block_count ||
    modelInfo.embedding_length ||
    modelInfo.feed_forward_length;
  if (!hasSpecs) return null;
  return (
    <div className="space-y-1 border-t pt-2">
      <h4 className="mb-2 text-sm font-semibold">Architecture Specifications</h4>
      <MetadataField label="Context Length" value={modelInfo.context_length} />
      {!!modelInfo.block_count && (
        <MetadataField label="Block Count (Layers)" value={modelInfo.block_count} />
      )}
      {!!modelInfo.embedding_length && (
        <MetadataField label="Embedding Length" value={modelInfo.embedding_length} />
      )}
      {!!modelInfo.feed_forward_length && (
        <MetadataField label="FFN Length" value={modelInfo.feed_forward_length} />
      )}
      {!!modelInfo.attention_head_count && (
        <MetadataField label="Attention Heads" value={modelInfo.attention_head_count} />
      )}
      {!!modelInfo.attention_head_count_kv && (
        <MetadataField label="KV Heads" value={modelInfo.attention_head_count_kv} />
      )}
      {!!modelInfo.layer_norm_epsilon && (
        <MetadataField label="Layer Norm Epsilon" value={modelInfo.layer_norm_epsilon} mono />
      )}
      {!!modelInfo.rope_dimension_count && (
        <MetadataField label="RoPE Dimensions" value={modelInfo.rope_dimension_count} />
      )}
      {!!modelInfo.rope_freq_base && (
        <MetadataField label="RoPE Freq Base" value={modelInfo.rope_freq_base} />
      )}
    </div>
  );
};

const VisionSection = ({ modelInfo }: { modelInfo: ModelMetadata }) => {
  if (!modelInfo.has_vision || !modelInfo.mmproj_files) return null;
  return (
    <div className="space-y-1 border-t pt-2">
      <h4 className="mb-2 flex items-center gap-1.5 text-sm font-semibold">
        <Eye className="size-3.5 text-violet-400" /> Vision Support
      </h4>
      <p className="mb-2 text-muted-foreground">
        Multimodal projection (mmproj) companion file detected.
      </p>
      {modelInfo.mmproj_files.map((f) => (
        <div
          key={f.name}
          className="flex items-center gap-2 rounded border border-violet-500/20 bg-violet-500/10 px-2 py-1.5"
        >
          <span className="flex-1 truncate font-mono text-xs">{f.name}</span>
          <span className="whitespace-nowrap text-xs text-muted-foreground">{f.file_size}</span>
        </div>
      ))}
    </div>
  );
};

const TokenizerSection = ({ modelInfo }: { modelInfo: ModelMetadata }) => {
  const hasTokenizer =
    modelInfo.tokenizer_model ||
    modelInfo.bos_token_id ||
    modelInfo.eos_token_id ||
    modelInfo.chat_template;
  if (!hasTokenizer) return null;
  return (
    <div className="space-y-1 border-t pt-2">
      <h4 className="mb-2 text-sm font-semibold">Tokenizer Information</h4>
      {!!modelInfo.tokenizer_model && (
        <MetadataField label="Tokenizer Type" value={modelInfo.tokenizer_model} />
      )}
      {!!modelInfo.bos_token_id && (
        <MetadataField label="BOS Token ID" value={modelInfo.bos_token_id} mono />
      )}
      {!!modelInfo.eos_token_id && (
        <MetadataField label="EOS Token ID" value={modelInfo.eos_token_id} mono />
      )}
      {!!modelInfo.padding_token_id && (
        <MetadataField label="Padding Token ID" value={modelInfo.padding_token_id} mono />
      )}
      {!!modelInfo.chat_template && (
        <div>
          <p>
            <strong>Chat Template:</strong>
          </p>
          <pre className="mt-1 max-h-32 overflow-x-auto overflow-y-auto whitespace-pre-wrap break-words rounded bg-muted p-2 text-xs">
            {modelInfo.chat_template}
          </pre>
        </div>
      )}
    </div>
  );
};

function formatGgufValue(value: unknown): string {
  if (typeof value === 'string') return value;
  if (typeof value === 'object') return JSON.stringify(value);
  return String(value);
}

const GgufMetadataSection = ({ modelInfo }: { modelInfo: ModelMetadata }) => {
  if (!modelInfo.gguf_metadata || Object.keys(modelInfo.gguf_metadata).length === 0) return null;
  return (
    <div className="space-y-1 border-t pt-2">
      <h4 className="mb-2 text-sm font-semibold">All GGUF Metadata</h4>
      <div className="space-y-1">
        {Object.entries(modelInfo.gguf_metadata)
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([key, value]) => (
            <p key={key} className="text-xs">
              <strong className="font-mono text-muted-foreground">{key}:</strong>{' '}
              <span className="font-mono text-muted-foreground">{formatGgufValue(value)}</span>
            </p>
          ))}
      </div>
    </div>
  );
};

export const ModelMetadataDisplay: React.FC<ModelMetadataDisplayProps> = ({ modelInfo }) => (
  <ParamGroup
    title="Model Metadata"
    collapsible
    defaultExpanded={false}
    freeLayout
    className="mt-3"
  >
    <div className="max-h-96 space-y-3 overflow-y-auto text-xs">
      <BasicInfoSection modelInfo={modelInfo} />
      <ModelDetailsSection modelInfo={modelInfo} />
      <ArchitectureSpecsSection modelInfo={modelInfo} />
      <VisionSection modelInfo={modelInfo} />
      <TokenizerSection modelInfo={modelInfo} />
      <GgufMetadataSection modelInfo={modelInfo} />
      <div className="border-t pt-2">
        <p className="text-xs">
          <strong>File Path:</strong>
        </p>
        <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
          {modelInfo.file_path}
        </p>
      </div>
    </div>
  </ParamGroup>
);
