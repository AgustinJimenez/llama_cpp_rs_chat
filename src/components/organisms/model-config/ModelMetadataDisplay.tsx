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
    <span className={`text-muted-foreground${mono ? ' font-mono' : ''}`}>{value}</span>
  </p>
);

const BasicInfoSection = ({ modelInfo }: { modelInfo: ModelMetadata }) => (
  <div className="space-y-1">
    <h4 className="font-semibold text-sm mb-2">Basic Information</h4>
    <MetadataField label="File Name" value={modelInfo.name} />
    {modelInfo.general_name ? (
      <MetadataField label="Model Name" value={modelInfo.general_name} />
    ) : null}
    <MetadataField label="File Size" value={modelInfo.file_size} />
    <MetadataField label="Architecture" value={modelInfo.architecture} />
    <MetadataField label="Parameters" value={modelInfo.parameters} />
    <MetadataField label="Quantization" value={modelInfo.quantization} />
    {modelInfo.file_type ? <MetadataField label="File Type" value={modelInfo.file_type} /> : null}
    {modelInfo.quantization_version ? (
      <MetadataField label="Quant Version" value={modelInfo.quantization_version} />
    ) : null}
    {modelInfo.has_vision ? (
      <p className="flex items-center gap-1.5">
        <Eye className="h-3.5 w-3.5 text-violet-400" />
        <strong>Vision:</strong> <span className="text-violet-400 font-medium">Supported</span>
        <span className="text-muted-foreground">
          ({modelInfo.mmproj_files?.length} mmproj file
          {(modelInfo.mmproj_files?.length ?? 0) > 1 ? 's' : ''} found)
        </span>
      </p>
    ) : null}
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
    <div className="space-y-1 pt-2 border-t">
      <h4 className="font-semibold text-sm mb-2">Model Details</h4>
      {modelInfo.description ? (
        <MetadataField label="Description" value={modelInfo.description} />
      ) : null}
      {modelInfo.author ? <MetadataField label="Author" value={modelInfo.author} /> : null}
      {modelInfo.organization ? (
        <MetadataField label="Organization" value={modelInfo.organization} />
      ) : null}
      {modelInfo.version ? <MetadataField label="Version" value={modelInfo.version} /> : null}
      {modelInfo.license ? <MetadataField label="License" value={modelInfo.license} /> : null}
      {modelInfo.url ? (
        <p>
          <strong>URL:</strong>{' '}
          <a
            href={modelInfo.url}
            target="_blank"
            rel="noopener noreferrer"
            className="text-primary hover:underline break-all"
          >
            {modelInfo.url}
          </a>
        </p>
      ) : null}
      {modelInfo.repo_url ? (
        <p>
          <strong>Repository:</strong>{' '}
          <a
            href={modelInfo.repo_url}
            target="_blank"
            rel="noopener noreferrer"
            className="text-primary hover:underline break-all"
          >
            {modelInfo.repo_url}
          </a>
        </p>
      ) : null}
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
    <div className="space-y-1 pt-2 border-t">
      <h4 className="font-semibold text-sm mb-2">Architecture Specifications</h4>
      <MetadataField label="Context Length" value={modelInfo.context_length} />
      {modelInfo.block_count ? (
        <MetadataField label="Block Count (Layers)" value={modelInfo.block_count} />
      ) : null}
      {modelInfo.embedding_length ? (
        <MetadataField label="Embedding Length" value={modelInfo.embedding_length} />
      ) : null}
      {modelInfo.feed_forward_length ? (
        <MetadataField label="FFN Length" value={modelInfo.feed_forward_length} />
      ) : null}
      {modelInfo.attention_head_count ? (
        <MetadataField label="Attention Heads" value={modelInfo.attention_head_count} />
      ) : null}
      {modelInfo.attention_head_count_kv ? (
        <MetadataField label="KV Heads" value={modelInfo.attention_head_count_kv} />
      ) : null}
      {modelInfo.layer_norm_epsilon ? (
        <MetadataField label="Layer Norm Epsilon" value={modelInfo.layer_norm_epsilon} mono />
      ) : null}
      {modelInfo.rope_dimension_count ? (
        <MetadataField label="RoPE Dimensions" value={modelInfo.rope_dimension_count} />
      ) : null}
      {modelInfo.rope_freq_base ? (
        <MetadataField label="RoPE Freq Base" value={modelInfo.rope_freq_base} />
      ) : null}
    </div>
  );
};

const VisionSection = ({ modelInfo }: { modelInfo: ModelMetadata }) => {
  if (!modelInfo.has_vision || !modelInfo.mmproj_files) return null;
  return (
    <div className="space-y-1 pt-2 border-t">
      <h4 className="font-semibold text-sm mb-2 flex items-center gap-1.5">
        <Eye className="h-3.5 w-3.5 text-violet-400" /> Vision Support
      </h4>
      <p className="text-muted-foreground mb-2">
        Multimodal projection (mmproj) companion file detected.
      </p>
      {modelInfo.mmproj_files.map((f) => (
        <div
          key={f.name}
          className="flex items-center gap-2 px-2 py-1.5 bg-violet-500/10 rounded border border-violet-500/20"
        >
          <span className="font-mono text-xs truncate flex-1">{f.name}</span>
          <span className="text-muted-foreground text-xs whitespace-nowrap">{f.file_size}</span>
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
    <div className="space-y-1 pt-2 border-t">
      <h4 className="font-semibold text-sm mb-2">Tokenizer Information</h4>
      {modelInfo.tokenizer_model ? (
        <MetadataField label="Tokenizer Type" value={modelInfo.tokenizer_model} />
      ) : null}
      {modelInfo.bos_token_id ? (
        <MetadataField label="BOS Token ID" value={modelInfo.bos_token_id} mono />
      ) : null}
      {modelInfo.eos_token_id ? (
        <MetadataField label="EOS Token ID" value={modelInfo.eos_token_id} mono />
      ) : null}
      {modelInfo.padding_token_id ? (
        <MetadataField label="Padding Token ID" value={modelInfo.padding_token_id} mono />
      ) : null}
      {modelInfo.chat_template ? (
        <div>
          <p>
            <strong>Chat Template:</strong>
          </p>
          <pre className="mt-1 p-2 bg-muted rounded text-xs overflow-x-auto whitespace-pre-wrap break-words max-h-32 overflow-y-auto">
            {modelInfo.chat_template}
          </pre>
        </div>
      ) : null}
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
    <div className="space-y-1 pt-2 border-t">
      <h4 className="font-semibold text-sm mb-2">All GGUF Metadata</h4>
      <div className="space-y-1">
        {Object.entries(modelInfo.gguf_metadata)
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([key, value]) => (
            <p key={key} className="text-xs">
              <strong className="font-mono text-muted-foreground">{key}:</strong>{' '}
              <span className="text-muted-foreground font-mono">{formatGgufValue(value)}</span>
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
    <div className="space-y-3 text-xs max-h-96 overflow-y-auto">
      <BasicInfoSection modelInfo={modelInfo} />
      <ModelDetailsSection modelInfo={modelInfo} />
      <ArchitectureSpecsSection modelInfo={modelInfo} />
      <VisionSection modelInfo={modelInfo} />
      <TokenizerSection modelInfo={modelInfo} />
      <GgufMetadataSection modelInfo={modelInfo} />
      <div className="pt-2 border-t">
        <p className="text-xs">
          <strong>File Path:</strong>
        </p>
        <p className="text-xs text-muted-foreground font-mono break-all mt-1">
          {modelInfo.file_path}
        </p>
      </div>
    </div>
  </ParamGroup>
);
