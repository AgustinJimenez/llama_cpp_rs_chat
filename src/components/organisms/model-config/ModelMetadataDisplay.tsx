import { Eye } from 'lucide-react';
import React from 'react';
import { useTranslation } from 'react-i18next';

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

const BasicInfoSection = ({ modelInfo }: { modelInfo: ModelMetadata }) => {
  const { t } = useTranslation();
  return (
    <div className="space-y-1">
      <h4 className="mb-2 text-sm font-semibold">{t('modelConfig.basicInfo')}</h4>
      <MetadataField label={t('modelConfig.fileName')} value={modelInfo.name} />
      {!!modelInfo.general_name && (
        <MetadataField label={t('modelConfig.modelName')} value={modelInfo.general_name} />
      )}
      <MetadataField label={t('modelConfig.fileSize')} value={modelInfo.file_size} />
      <MetadataField label={t('modelConfig.architecture')} value={modelInfo.architecture} />
      <MetadataField label={t('modelConfig.parameters')} value={modelInfo.parameters} />
      <MetadataField label={t('modelConfig.quantization')} value={modelInfo.quantization} />
      {!!modelInfo.file_type && (
        <MetadataField label={t('modelConfig.fileType')} value={modelInfo.file_type} />
      )}
      {!!modelInfo.quantization_version && (
        <MetadataField
          label={t('modelConfig.quantVersion')}
          value={modelInfo.quantization_version}
        />
      )}
      {!!modelInfo.has_vision && (
        <p className="flex items-center gap-1.5">
          <Eye className="size-3.5 text-violet-400" />
          <strong>{t('modelConfig.vision')}:</strong>{' '}
          <span className="font-medium text-violet-400">{t('modelConfig.supported')}</span>
          <span className="text-muted-foreground">
            (
            {t('modelConfig.mmprojFileCount', {
              count: modelInfo.mmproj_files?.length ?? 0,
              pluralSuffix: (modelInfo.mmproj_files?.length ?? 0) > 1 ? 's' : '',
            })}
            )
          </span>
        </p>
      )}
    </div>
  );
};

const ModelDetailsSection = ({ modelInfo }: { modelInfo: ModelMetadata }) => {
  const { t } = useTranslation();
  const hasDetails =
    modelInfo.description ||
    modelInfo.author ||
    modelInfo.organization ||
    modelInfo.version ||
    modelInfo.license;
  if (!hasDetails) return null;
  return (
    <div className="space-y-1 border-t pt-2">
      <h4 className="mb-2 text-sm font-semibold">{t('modelConfig.modelDetails')}</h4>
      {!!modelInfo.description && (
        <MetadataField label={t('modelConfig.description')} value={modelInfo.description} />
      )}
      {!!modelInfo.author && (
        <MetadataField label={t('modelConfig.author')} value={modelInfo.author} />
      )}
      {!!modelInfo.organization && (
        <MetadataField label={t('modelConfig.organization')} value={modelInfo.organization} />
      )}
      {!!modelInfo.version && (
        <MetadataField label={t('modelConfig.version')} value={modelInfo.version} />
      )}
      {!!modelInfo.license && (
        <MetadataField label={t('modelConfig.license')} value={modelInfo.license} />
      )}
      {!!modelInfo.url && (
        <p>
          <strong>{t('modelConfig.url')}:</strong>{' '}
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
          <strong>{t('modelConfig.repository')}:</strong>{' '}
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
  const { t } = useTranslation();
  const hasSpecs =
    modelInfo.context_length ||
    modelInfo.block_count ||
    modelInfo.embedding_length ||
    modelInfo.feed_forward_length;
  if (!hasSpecs) return null;
  return (
    <div className="space-y-1 border-t pt-2">
      <h4 className="mb-2 text-sm font-semibold">{t('modelConfig.architectureSpecs')}</h4>
      <MetadataField label={t('modelConfig.contextLength')} value={modelInfo.context_length} />
      {!!modelInfo.block_count && (
        <MetadataField label={t('modelConfig.blockCount')} value={modelInfo.block_count} />
      )}
      {!!modelInfo.embedding_length && (
        <MetadataField
          label={t('modelConfig.embeddingLength')}
          value={modelInfo.embedding_length}
        />
      )}
      {!!modelInfo.feed_forward_length && (
        <MetadataField label={t('modelConfig.ffnLength')} value={modelInfo.feed_forward_length} />
      )}
      {!!modelInfo.attention_head_count && (
        <MetadataField
          label={t('modelConfig.attentionHeads')}
          value={modelInfo.attention_head_count}
        />
      )}
      {!!modelInfo.attention_head_count_kv && (
        <MetadataField label={t('modelConfig.kvHeads')} value={modelInfo.attention_head_count_kv} />
      )}
      {!!modelInfo.layer_norm_epsilon && (
        <MetadataField
          label={t('modelConfig.layerNormEpsilon')}
          value={modelInfo.layer_norm_epsilon}
          mono
        />
      )}
      {!!modelInfo.rope_dimension_count && (
        <MetadataField
          label={t('modelConfig.ropeDimensions')}
          value={modelInfo.rope_dimension_count}
        />
      )}
      {!!modelInfo.rope_freq_base && (
        <MetadataField label={t('modelConfig.ropeFreqBase')} value={modelInfo.rope_freq_base} />
      )}
    </div>
  );
};

const VisionSection = ({ modelInfo }: { modelInfo: ModelMetadata }) => {
  const { t } = useTranslation();
  if (!modelInfo.has_vision || !modelInfo.mmproj_files) return null;
  return (
    <div className="space-y-1 border-t pt-2">
      <h4 className="mb-2 flex items-center gap-1.5 text-sm font-semibold">
        <Eye className="size-3.5 text-violet-400" /> {t('modelConfig.visionSupport')}
      </h4>
      <p className="mb-2 text-muted-foreground">{t('modelConfig.multimodalDetected')}</p>
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
  const { t } = useTranslation();
  const hasTokenizer =
    modelInfo.tokenizer_model ||
    modelInfo.bos_token_id ||
    modelInfo.eos_token_id ||
    modelInfo.chat_template;
  if (!hasTokenizer) return null;
  return (
    <div className="space-y-1 border-t pt-2">
      <h4 className="mb-2 text-sm font-semibold">{t('modelConfig.tokenizerInfo')}</h4>
      {!!modelInfo.tokenizer_model && (
        <MetadataField label={t('modelConfig.tokenizerType')} value={modelInfo.tokenizer_model} />
      )}
      {!!modelInfo.bos_token_id && (
        <MetadataField label={t('modelConfig.bosTokenId')} value={modelInfo.bos_token_id} mono />
      )}
      {!!modelInfo.eos_token_id && (
        <MetadataField label={t('modelConfig.eosTokenId')} value={modelInfo.eos_token_id} mono />
      )}
      {!!modelInfo.padding_token_id && (
        <MetadataField
          label={t('modelConfig.paddingTokenId')}
          value={modelInfo.padding_token_id}
          mono
        />
      )}
      {!!modelInfo.chat_template && (
        <div>
          <p>
            <strong>{t('modelConfig.chatTemplate')}:</strong>
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
  const { t } = useTranslation();
  if (!modelInfo.gguf_metadata || Object.keys(modelInfo.gguf_metadata).length === 0) return null;
  return (
    <div className="space-y-1 border-t pt-2">
      <h4 className="mb-2 text-sm font-semibold">{t('modelConfig.allGgufMetadata')}</h4>
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

export const ModelMetadataDisplay: React.FC<ModelMetadataDisplayProps> = ({ modelInfo }) => {
  const { t } = useTranslation();
  return (
    <ParamGroup title={undefined} collapsible defaultExpanded={false} freeLayout className="mt-3">
      <div className="max-h-96 space-y-3 overflow-y-auto text-xs">
        <BasicInfoSection modelInfo={modelInfo} />
        <ModelDetailsSection modelInfo={modelInfo} />
        <ArchitectureSpecsSection modelInfo={modelInfo} />
        <VisionSection modelInfo={modelInfo} />
        <TokenizerSection modelInfo={modelInfo} />
        <GgufMetadataSection modelInfo={modelInfo} />
        <div className="border-t pt-2">
          <p className="text-xs">
            <strong>{t('modelConfig.filePath')}:</strong>
          </p>
          <p className="mt-1 break-all font-mono text-xs text-muted-foreground">
            {modelInfo.file_path}
          </p>
        </div>
      </div>
    </ParamGroup>
  );
};
