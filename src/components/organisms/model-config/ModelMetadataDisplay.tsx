import React from 'react';
import { ChevronDown, ChevronRight } from 'lucide-react';
import { Card, CardContent, CardHeader, CardTitle } from '../../atoms/card';
import type { ModelMetadata } from '@/types';

export interface ModelMetadataDisplayProps {
  modelInfo: ModelMetadata;
  isExpanded: boolean;
  setIsExpanded: (expanded: boolean) => void;
}

// eslint-disable-next-line complexity
export const ModelMetadataDisplay: React.FC<ModelMetadataDisplayProps> = ({
  modelInfo,
  isExpanded,
  setIsExpanded
}) => (
  <Card className="mt-3">
    <CardHeader className="p-0">
      <button
        className={`flex items-center justify-between w-full text-left bg-primary text-white px-6 py-3 hover:opacity-90 transition-opacity ${
          isExpanded ? 'rounded-t-lg' : 'rounded-lg'
        }`}
        onClick={() => setIsExpanded(!isExpanded)}
        type="button"
      >
        <CardTitle className="text-sm flex items-center gap-2 text-white">
          {isExpanded ? <ChevronDown className="h-5 w-5 text-white stroke-[3]" /> : <ChevronRight className="h-5 w-5 text-white stroke-[3]" />}
          Model Metadata
        </CardTitle>
      </button>
    </CardHeader>
    {isExpanded && (
      <CardContent className="pt-6">
        <div className="space-y-3 text-xs max-h-96 overflow-y-auto">
          {/* Basic Info */}
          <div className="space-y-1">
            <h4 className="font-semibold text-sm mb-2">Basic Information</h4>
            <p><strong>File Name:</strong> <span className="text-muted-foreground">{modelInfo.name}</span></p>
            {modelInfo.general_name && <p><strong>Model Name:</strong> <span className="text-muted-foreground">{modelInfo.general_name}</span></p>}
            <p><strong>File Size:</strong> <span className="text-muted-foreground">{modelInfo.file_size}</span></p>
            <p><strong>Architecture:</strong> <span className="text-muted-foreground">{modelInfo.architecture}</span></p>
            <p><strong>Parameters:</strong> <span className="text-muted-foreground">{modelInfo.parameters}</span></p>
            <p><strong>Quantization:</strong> <span className="text-muted-foreground">{modelInfo.quantization}</span></p>
            {modelInfo.file_type && <p><strong>File Type:</strong> <span className="text-muted-foreground">{modelInfo.file_type}</span></p>}
            {modelInfo.quantization_version && <p><strong>Quant Version:</strong> <span className="text-muted-foreground">{modelInfo.quantization_version}</span></p>}
          </div>

          {/* Model Details */}
          {(modelInfo.description || modelInfo.author || modelInfo.organization || modelInfo.version || modelInfo.license) && (
            <div className="space-y-1 pt-2 border-t">
              <h4 className="font-semibold text-sm mb-2">Model Details</h4>
              {modelInfo.description && <p><strong>Description:</strong> <span className="text-muted-foreground">{modelInfo.description}</span></p>}
              {modelInfo.author && <p><strong>Author:</strong> <span className="text-muted-foreground">{modelInfo.author}</span></p>}
              {modelInfo.organization && <p><strong>Organization:</strong> <span className="text-muted-foreground">{modelInfo.organization}</span></p>}
              {modelInfo.version && <p><strong>Version:</strong> <span className="text-muted-foreground">{modelInfo.version}</span></p>}
              {modelInfo.license && <p><strong>License:</strong> <span className="text-muted-foreground">{modelInfo.license}</span></p>}
              {modelInfo.url && (
                <p><strong>URL:</strong> <a href={modelInfo.url} target="_blank" rel="noopener noreferrer" className="text-primary hover:underline break-all">{modelInfo.url}</a></p>
              )}
              {modelInfo.repo_url && (
                <p><strong>Repository:</strong> <a href={modelInfo.repo_url} target="_blank" rel="noopener noreferrer" className="text-primary hover:underline break-all">{modelInfo.repo_url}</a></p>
              )}
            </div>
          )}

          {/* Architecture Specs */}
          {(modelInfo.context_length || modelInfo.block_count || modelInfo.embedding_length || modelInfo.feed_forward_length) && (
            <div className="space-y-1 pt-2 border-t">
              <h4 className="font-semibold text-sm mb-2">Architecture Specifications</h4>
              <p><strong>Context Length:</strong> <span className="text-muted-foreground">{modelInfo.context_length}</span></p>
              {modelInfo.block_count && <p><strong>Block Count (Layers):</strong> <span className="text-muted-foreground">{modelInfo.block_count}</span></p>}
              {modelInfo.embedding_length && <p><strong>Embedding Length:</strong> <span className="text-muted-foreground">{modelInfo.embedding_length}</span></p>}
              {modelInfo.feed_forward_length && <p><strong>FFN Length:</strong> <span className="text-muted-foreground">{modelInfo.feed_forward_length}</span></p>}
              {modelInfo.attention_head_count && <p><strong>Attention Heads:</strong> <span className="text-muted-foreground">{modelInfo.attention_head_count}</span></p>}
              {modelInfo.attention_head_count_kv && <p><strong>KV Heads:</strong> <span className="text-muted-foreground">{modelInfo.attention_head_count_kv}</span></p>}
              {modelInfo.layer_norm_epsilon && <p><strong>Layer Norm Epsilon:</strong> <span className="text-muted-foreground font-mono">{modelInfo.layer_norm_epsilon}</span></p>}
              {modelInfo.rope_dimension_count && <p><strong>RoPE Dimensions:</strong> <span className="text-muted-foreground">{modelInfo.rope_dimension_count}</span></p>}
              {modelInfo.rope_freq_base && <p><strong>RoPE Freq Base:</strong> <span className="text-muted-foreground">{modelInfo.rope_freq_base}</span></p>}
            </div>
          )}

          {/* Tokenizer Info */}
          {(modelInfo.tokenizer_model || modelInfo.bos_token_id || modelInfo.eos_token_id || modelInfo.chat_template) && (
            <div className="space-y-1 pt-2 border-t">
              <h4 className="font-semibold text-sm mb-2">Tokenizer Information</h4>
              {modelInfo.tokenizer_model && <p><strong>Tokenizer Type:</strong> <span className="text-muted-foreground">{modelInfo.tokenizer_model}</span></p>}
              {modelInfo.bos_token_id && <p><strong>BOS Token ID:</strong> <span className="text-muted-foreground font-mono">{modelInfo.bos_token_id}</span></p>}
              {modelInfo.eos_token_id && <p><strong>EOS Token ID:</strong> <span className="text-muted-foreground font-mono">{modelInfo.eos_token_id}</span></p>}
              {modelInfo.padding_token_id && <p><strong>Padding Token ID:</strong> <span className="text-muted-foreground font-mono">{modelInfo.padding_token_id}</span></p>}
              {modelInfo.chat_template && (
                <div>
                  <p><strong>Chat Template:</strong></p>
                  <pre className="mt-1 p-2 bg-muted rounded text-xs overflow-x-auto whitespace-pre-wrap break-words max-h-32 overflow-y-auto">{modelInfo.chat_template}</pre>
                </div>
              )}
            </div>
          )}

          {/* All GGUF Metadata */}
          {modelInfo.gguf_metadata && Object.keys(modelInfo.gguf_metadata).length > 0 && (
            <div className="space-y-1 pt-2 border-t">
              <h4 className="font-semibold text-sm mb-2">All GGUF Metadata</h4>
              <div className="space-y-1">
                {Object.entries(modelInfo.gguf_metadata)
                  .sort(([a], [b]) => a.localeCompare(b))
                  .map(([key, value]) => (
                    <p key={key} className="text-xs">
                      <strong className="font-mono text-zinc-600 dark:text-zinc-400">{key}:</strong>{' '}
                      <span className="text-muted-foreground font-mono">
                        {typeof value === 'string'
                          ? value
                          : typeof value === 'object'
                            ? JSON.stringify(value)
                            : String(value)}
                      </span>
                    </p>
                  ))}
              </div>
            </div>
          )}

          {/* File Path at the end */}
          <div className="pt-2 border-t">
            <p className="text-xs"><strong>File Path:</strong></p>
            <p className="text-xs text-muted-foreground font-mono break-all mt-1">{modelInfo.file_path}</p>
          </div>
        </div>
      </CardContent>
    )}
  </Card>
);
