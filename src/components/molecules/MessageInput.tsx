import React, { useState, useCallback, KeyboardEvent, useEffect, useRef } from 'react';
import { ArrowUp, Square, X, FileText, Loader2, Paperclip, Database } from 'lucide-react';
import { useChatContext } from '../../contexts/ChatContext';
import { useModelContext } from '../../contexts/ModelContext';
import { MessageStatistics } from './messages/MessageStatistics';
import type { TimingInfo } from '../../utils/chatTransport';

function LiveStreamingStats({ tokensUsed, maxTokens, streamStatus }: { tokensUsed?: number; maxTokens?: number; streamStatus?: string }) {
  const [polledStatus, setPolledStatus] = useState<string | undefined>(undefined);
  const [elapsed, setElapsed] = useState(0);
  const [tokenCount, setTokenCount] = useState(0);
  const startRef = useRef(Date.now());
  const firstTokensUsedRef = useRef<number | null>(null);
  const fmt = (n: number) => n.toLocaleString('en-US').replace(/,/g, '.');
  const pct = tokensUsed && maxTokens ? Math.round((tokensUsed / maxTokens) * 100) : 0;

  // Timer
  useEffect(() => {
    startRef.current = Date.now();
    setTokenCount(0);
    firstTokensUsedRef.current = null;
    const id = setInterval(() => setElapsed(Date.now() - startRef.current), 1000);
    return () => clearInterval(id);
  }, []);

  // Track token count from tokensUsed changes
  useEffect(() => {
    if (tokensUsed === undefined) return;
    if (firstTokensUsedRef.current === null) {
      firstTokensUsedRef.current = tokensUsed;
    }
    setTokenCount(tokensUsed - firstTokensUsedRef.current);
  }, [tokensUsed]);

  // Poll model status API for compaction progress
  useEffect(() => {
    if (streamStatus) { setPolledStatus(undefined); return; }
    const poll = async () => {
      try {
        const resp = await fetch('/api/model/status');
        if (resp.ok) {
          const data = await resp.json();
          setPolledStatus(data.status_message || undefined);
        }
      } catch { /* ignore */ }
    };
    poll();
    const id = setInterval(poll, 2000);
    return () => clearInterval(id);
  }, [streamStatus]);

  const displayStatus = streamStatus || polledStatus;
  const hasContext = tokensUsed !== undefined && maxTokens !== undefined;
  const secs = Math.floor(elapsed / 1000);
  const tokPerSec = secs > 0 && tokenCount > 0 ? (tokenCount / secs).toFixed(1) : null;

  if (!hasContext && !displayStatus) return null;
  return (
    <div className="flex items-center gap-3 text-xs text-muted-foreground font-mono">
      {displayStatus ? (
        <span className="inline-flex items-center gap-1 text-cyan-400">
          <Loader2 className="h-3 w-3 animate-spin" />
          {displayStatus}
        </span>
      ) : null}
      {tokenCount > 0 ? (
        <span className="inline-flex items-center gap-1" title="Tokens generated this turn">
          # {tokenCount.toLocaleString()}
        </span>
      ) : null}
      {tokPerSec ? (
        <span className="inline-flex items-center gap-1" title="Generation speed">
          {tokPerSec} tok/s
        </span>
      ) : null}
      {hasContext ? (
        <span className={`inline-flex items-center gap-1 ${pct > 90 ? 'text-yellow-400' : ''}`} title={`Context: ${pct}% used`}>
          <Database className="h-3 w-3" />
          {fmt(tokensUsed!)}/{fmt(maxTokens!)}
        </span>
      ) : null}
    </div>
  );
}

interface MessageInputProps {
  disabledReason?: string;
  timings?: TimingInfo;
  tokensUsed?: number;
  maxTokens?: number;
  streamStatus?: string;
}

interface AttachedFile {
  name: string;
  text: string;
}

const IMAGE_EXTENSIONS = new Set(['.png', '.jpg', '.jpeg', '.gif', '.webp', '.bmp']);
const TEXT_EXTENSIONS = new Set([
  '.txt', '.json', '.xml', '.md', '.rs', '.py', '.js', '.ts', '.tsx',
  '.jsx', '.html', '.css', '.toml', '.yaml', '.yml', '.sh', '.bat', '.c',
  '.cpp', '.h', '.hpp', '.cs', '.go', '.java', '.rb', '.php', '.sql',
  '.log', '.cfg', '.ini', '.nim', '.ex', '.exs', '.kt', '.swift', '.r',
  '.lua', '.pl', '.scala', '.zig', '.v', '.dart',
]);
const DOCUMENT_EXTENSIONS = new Set([
  '.pdf', '.docx', '.pptx', '.xlsx', '.xls', '.xlsm',
  '.epub', '.odt', '.rtf', '.zip', '.csv', '.eml',
]);

const MAX_TEXT_FILE_SIZE = 100 * 1024; // 100KB

// Map extensions to markdown language identifiers for code blocks
const EXT_TO_LANG: Record<string, string> = {
  '.py': 'python', '.js': 'javascript', '.ts': 'typescript', '.tsx': 'tsx',
  '.jsx': 'jsx', '.rs': 'rust', '.go': 'go', '.java': 'java', '.c': 'c',
  '.cpp': 'cpp', '.h': 'c', '.hpp': 'cpp', '.cs': 'csharp', '.rb': 'ruby',
  '.php': 'php', '.html': 'html', '.css': 'css', '.json': 'json',
  '.yaml': 'yaml', '.yml': 'yaml', '.toml': 'toml', '.md': 'markdown',
  '.txt': 'text', '.sh': 'bash', '.bat': 'batch', '.sql': 'sql',
  '.nim': 'nim', '.ex': 'elixir', '.exs': 'elixir', '.kt': 'kotlin',
  '.swift': 'swift', '.r': 'r', '.lua': 'lua', '.pl': 'perl',
  '.scala': 'scala', '.zig': 'zig', '.v': 'v', '.dart': 'dart',
  '.xml': 'xml', '.log': 'text', '.cfg': 'ini', '.ini': 'ini',
};

function getFileExtension(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot >= 0 ? name.slice(dot).toLowerCase() : '';
}

function formatCharCount(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return `${n}`;
}

function ImagePreviews({ images, onRemove }: { images: string[]; onRemove: (i: number) => void }) {
  if (images.length === 0) return null;
  return (
    <div className="px-5 pt-2 pb-1 flex flex-wrap gap-2">
      {images.map((img, i) => (
        <div key={i} className="relative inline-block">
          <img
            src={img}
            alt="Attached"
            className="max-h-24 max-w-48 rounded-lg border border-border object-cover"
          />
          <button
            type="button"
            onClick={() => onRemove(i)}
            className="absolute -top-2 -right-2 w-5 h-5 flex items-center justify-center rounded-full bg-red-500 text-white hover:bg-red-600 transition-colors"
            title="Remove image"
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      ))}
    </div>
  );
}

function FilePreviews({ files, onRemove }: { files: AttachedFile[]; onRemove: (i: number) => void }) {
  if (files.length === 0) return null;
  return (
    <div className="px-5 pt-2 pb-1 flex flex-wrap gap-2">
      {files.map((file, i) => (
        <div key={i} className="flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg bg-muted/60 border border-border text-xs">
          <FileText className="h-3.5 w-3.5 text-muted-foreground flex-shrink-0" />
          <span className="font-medium truncate max-w-[150px]" title={file.name}>{file.name}</span>
          <span className="text-muted-foreground">{formatCharCount(file.text.length)} chars</span>
          <button
            type="button"
            onClick={() => onRemove(i)}
            className="ml-0.5 w-4 h-4 flex items-center justify-center rounded-full hover:bg-red-500/20 text-muted-foreground hover:text-red-500 transition-colors"
            title="Remove file"
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      ))}
    </div>
  );
}

export const MessageInput: React.FC<MessageInputProps> = ({
  disabledReason,
  timings,
  tokensUsed,
  maxTokens,
  streamStatus,
}) => {
  const { sendMessage: onSendMessage, isLoading, stopGeneration } = useChatContext();
  const { status, isLoading: isModelLoading, loadingAction } = useModelContext();
  const hasVision = status.has_vision ?? false;
  const isModelBusy = isModelLoading && loadingAction === 'loading';

  const disabled = isLoading || isModelBusy;
  const [message, setMessage] = useState('');
  const [attachedImages, setAttachedImages] = useState<string[]>([]);
  const [attachedFiles, setAttachedFiles] = useState<AttachedFile[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [isExtracting, setIsExtracting] = useState(0);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const dragCounterRef = useRef(0);

  useEffect(() => {
    if (textareaRef.current) {
      textareaRef.current.focus();
    }
  }, []);

  useEffect(() => {
    if (!disabled && textareaRef.current) {
      textareaRef.current.focus();
    }
  }, [disabled]);

  const processFiles = useCallback(async (files: File[]) => {
    for (const file of files) {
      const ext = getFileExtension(file.name);

      // Images → existing vision pipeline
      if (IMAGE_EXTENSIONS.has(ext) && hasVision) {
        const reader = new FileReader();
        reader.onload = (ev) => {
          const dataUrl = ev.target?.result as string;
          if (dataUrl) setAttachedImages(prev => [...prev, dataUrl]);
        };
        reader.readAsDataURL(file);
        continue;
      }

      // Text/code files → read directly in browser
      if (TEXT_EXTENSIONS.has(ext)) {
        if (file.size > MAX_TEXT_FILE_SIZE) {
          console.warn(`File ${file.name} exceeds 100KB limit (${(file.size / 1024).toFixed(1)}KB)`);
          alert(`File "${file.name}" exceeds the 100KB size limit (${(file.size / 1024).toFixed(1)}KB).`);
          continue;
        }
        const text = await file.text();
        setAttachedFiles(prev => [...prev, { name: file.name, text }]);
        continue;
      }

      // Documents → send to backend for extraction
      if (DOCUMENT_EXTENSIONS.has(ext)) {
        setIsExtracting(prev => prev + 1);
        try {
          const buf = await file.arrayBuffer();
          const res = await fetch(`/api/file/extract-text?filename=${encodeURIComponent(file.name)}`, {
            method: 'POST',
            headers: { 'Content-Type': 'application/octet-stream' },
            body: buf,
          });
          const data = await res.json();
          if (data.success && data.text) {
            setAttachedFiles(prev => [...prev, { name: file.name, text: data.text }]);
          }
        } catch (err) {
          console.error('File extraction failed:', err);
        } finally {
          setIsExtracting(prev => prev - 1);
        }
        continue;
      }

      // Unknown extension — try as text
      try {
        if (file.size > MAX_TEXT_FILE_SIZE) {
          console.warn(`File ${file.name} exceeds 100KB limit (${(file.size / 1024).toFixed(1)}KB)`);
          alert(`File "${file.name}" exceeds the 100KB size limit (${(file.size / 1024).toFixed(1)}KB).`);
          continue;
        }
        const text = await file.text();
        setAttachedFiles(prev => [...prev, { name: file.name, text }]);
      } catch {
        console.warn('Could not read file:', file.name);
      }
    }
  }, [hasVision]);

  const handleSubmit = useCallback((e: React.FormEvent) => {
    e.preventDefault();
    const hasContent = message.trim() || attachedImages.length > 0 || attachedFiles.length > 0;
    if (!hasContent || disabled || isExtracting > 0) return;

    // Build message with file context prepended as code blocks
    let finalMessage = '';
    if (attachedFiles.length > 0) {
      for (const f of attachedFiles) {
        const ext = getFileExtension(f.name);
        const lang = EXT_TO_LANG[ext] || 'text';
        finalMessage += `File: ${f.name}\n\`\`\`${lang}\n${f.text}\n\`\`\`\n\n`;
      }
    }
    finalMessage += message.trim() || (attachedImages.length > 0 ? 'What is in this image?' : '');

    onSendMessage(
      finalMessage,
      attachedImages.length > 0 ? attachedImages : undefined,
    );
    setMessage('');
    setAttachedImages([]);
    setAttachedFiles([]);
    if (textareaRef.current) {
      textareaRef.current.style.height = 'auto';
    }
  }, [message, attachedImages, attachedFiles, disabled, isExtracting, onSendMessage]);

  const handleKeyDown = useCallback((e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      handleSubmit(e);
    }
  }, [handleSubmit]);

  const autoResize = useCallback((el: HTMLTextAreaElement) => {
    el.style.height = 'auto';
    const maxHeight = 20 * 7;
    el.style.height = `${Math.min(el.scrollHeight, maxHeight)}px`;
  }, []);

  const handleTextareaChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    setMessage(e.target.value);
    autoResize(e.target);
  }, [autoResize]);

  const handlePaste = useCallback((e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const items = e.clipboardData?.items;
    if (!items) return;

    // Check for files (images or documents)
    const pastedFiles: File[] = [];
    for (const item of items) {
      if (item.kind === 'file') {
        const file = item.getAsFile();
        if (file) pastedFiles.push(file);
      }
    }
    if (pastedFiles.length === 0) return;

    // Only handle images if vision is available, or handle document files
    const relevantFiles = pastedFiles.filter(f => {
      const ext = getFileExtension(f.name);
      if (IMAGE_EXTENSIONS.has(ext)) return hasVision;
      return true; // documents and text always accepted
    });
    if (relevantFiles.length === 0) return;

    e.preventDefault();
    processFiles(relevantFiles);
  }, [hasVision, processFiles]);

  const handleDragEnter = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounterRef.current++;
    if (e.dataTransfer.types.includes('Files')) {
      setIsDragging(true);
    }
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounterRef.current--;
    if (dragCounterRef.current === 0) {
      setIsDragging(false);
    }
  }, []);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
  }, []);

  const handleDrop = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    e.stopPropagation();
    dragCounterRef.current = 0;
    setIsDragging(false);

    const droppedFiles = Array.from(e.dataTransfer.files);
    if (droppedFiles.length > 0) {
      processFiles(droppedFiles);
    }
  }, [processFiles]);

  const handleFileButtonClick = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleFileInputChange = useCallback((e: React.ChangeEvent<HTMLInputElement>) => {
    const selectedFiles = Array.from(e.target.files || []);
    if (selectedFiles.length > 0) {
      processFiles(selectedFiles);
    }
    // Reset input so the same file can be selected again
    e.target.value = '';
  }, [processFiles]);

  const removeImage = useCallback((index: number) => {
    setAttachedImages(prev => prev.filter((_, i) => i !== index));
  }, []);

  const removeFile = useCallback((index: number) => {
    setAttachedFiles(prev => prev.filter((_, i) => i !== index));
  }, []);

  const isMultiline = message.includes('\n') || (textareaRef.current?.scrollHeight ?? 0) > 40;
  const hasContent = message.trim() || attachedImages.length > 0 || attachedFiles.length > 0;
  const placeholder = isModelBusy ? "Loading model..." : disabled && disabledReason ? disabledReason : "Ask anything";

  return (
    <form
      onSubmit={handleSubmit}
      onDragEnter={handleDragEnter}
      onDragLeave={handleDragLeave}
      onDragOver={handleDragOver}
      onDrop={handleDrop}
      data-testid="message-form"
      className="relative"
    >
      {isDragging ? (
        <div className="absolute inset-0 z-10 flex items-center justify-center rounded-2xl border-2 border-dashed border-primary/50 bg-primary/5 backdrop-blur-sm pointer-events-none">
          <div className="flex items-center gap-2 text-sm font-medium text-primary">
            <FileText className="h-5 w-5" />
            Drop files here
          </div>
        </div>
      ) : null}
      {(timings?.genTokPerSec || disabled || (tokensUsed !== undefined && maxTokens !== undefined)) ? (
        <div className="flex items-center justify-between mb-1">
          <div className="flex-1">
            {timings?.genTokPerSec ? (
              <MessageStatistics timings={timings} tokensUsed={tokensUsed} maxTokens={maxTokens} />
            ) : (tokensUsed !== undefined || isLoading || streamStatus) ? (
              <LiveStreamingStats tokensUsed={tokensUsed} maxTokens={maxTokens} streamStatus={streamStatus} />
            ) : null}
          </div>
          {disabled ? (
            <button
              type="button"
              onClick={stopGeneration ?? undefined}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-full text-xs font-medium bg-muted hover:bg-accent text-foreground transition-colors"
              data-testid="stop-button"
              title="Stop generation"
            >
              <Square className="h-3 w-3 fill-current" />
              Stop
            </button>
          ) : null}
        </div>
      ) : null}
      <ImagePreviews images={attachedImages} onRemove={removeImage} />
      <FilePreviews files={attachedFiles} onRemove={removeFile} />
      {isExtracting > 0 ? (
        <div className="px-5 pt-1 pb-1 flex items-center gap-2 text-xs text-muted-foreground">
          <Loader2 className="h-3 w-3 animate-spin" />
          Extracting text from {isExtracting} file{isExtracting > 1 ? 's' : ''}...
        </div>
      ) : null}
      <input
        ref={fileInputRef}
        type="file"
        multiple
        className="hidden"
        onChange={handleFileInputChange}
        accept=".pdf,.docx,.pptx,.xlsx,.xls,.xlsm,.epub,.odt,.rtf,.zip,.csv,.eml,.txt,.json,.xml,.md,.rs,.py,.js,.ts,.tsx,.jsx,.html,.css,.toml,.yaml,.yml,.sh,.bat,.c,.cpp,.h,.hpp,.cs,.go,.java,.rb,.php,.sql,.log,.cfg,.ini,.nim,.ex,.exs,.kt,.swift,.r,.lua,.pl,.scala,.zig,.v,.dart,.png,.jpg,.jpeg,.gif,.webp"
      />
      <div className={`flat-input-container flat-card flex items-end gap-2 px-5 py-2.5 ${isMultiline ? '!rounded-2xl' : ''}`}>
        <button
          type="button"
          onClick={handleFileButtonClick}
          className="flex-shrink-0 flex items-center py-1 opacity-40 hover:opacity-70 transition-opacity"
          title="Attach files"
          aria-label="Attach files"
        >
          <Paperclip className="h-4 w-4" />
        </button>
        <textarea
          ref={textareaRef}
          value={message}
          onChange={handleTextareaChange}
          onKeyDown={handleKeyDown}
          onPaste={handlePaste}
          placeholder={placeholder}
          disabled={disabled}
          className="flex-1 bg-transparent border-none outline-none resize-none text-sm text-foreground placeholder:text-muted-foreground min-h-[28px] py-1 overflow-y-auto"
          rows={1}
          data-testid="message-input"
          aria-disabled={disabled}
          aria-label={disabled && disabledReason ? disabledReason : 'Message input'}
        />
        <button
          type="submit"
          disabled={disabled || !hasContent || isExtracting > 0}
          className="flex-shrink-0 w-8 h-8 flex items-center justify-center rounded-full bg-foreground text-background hover:opacity-80 transition-colors disabled:opacity-30 disabled:cursor-not-allowed"
          data-testid="send-button"
          aria-label="Send message"
        >
          <ArrowUp className="h-4 w-4" />
        </button>
      </div>
    </form>
  );
};
