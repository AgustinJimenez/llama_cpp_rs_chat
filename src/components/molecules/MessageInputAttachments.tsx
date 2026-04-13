import { useState, useCallback, useRef } from 'react';

const IMAGE_EXTENSIONS = new Set(['.png', '.jpg', '.jpeg', '.gif', '.webp', '.bmp']);
const TEXT_EXTENSIONS = new Set([
  '.txt',
  '.json',
  '.xml',
  '.md',
  '.rs',
  '.py',
  '.js',
  '.ts',
  '.tsx',
  '.jsx',
  '.html',
  '.css',
  '.toml',
  '.yaml',
  '.yml',
  '.sh',
  '.bat',
  '.c',
  '.cpp',
  '.h',
  '.hpp',
  '.cs',
  '.go',
  '.java',
  '.rb',
  '.php',
  '.sql',
  '.log',
  '.cfg',
  '.ini',
  '.nim',
  '.ex',
  '.exs',
  '.kt',
  '.swift',
  '.r',
  '.lua',
  '.pl',
  '.scala',
  '.zig',
  '.v',
  '.dart',
]);
const DOCUMENT_EXTENSIONS = new Set([
  '.pdf',
  '.docx',
  '.pptx',
  '.xlsx',
  '.xls',
  '.xlsm',
  '.epub',
  '.odt',
  '.rtf',
  '.zip',
  '.csv',
  '.eml',
]);

const MAX_TEXT_FILE_SIZE = 100 * 1024; // 100KB

let fileIdCounter = 0;
function nextFileId(): string {
  fileIdCounter++;
  return `af-${fileIdCounter}-${Date.now()}`;
}

export interface AttachedFile {
  id: string;
  name: string;
  text: string;
}

// Map extensions to markdown language identifiers for code blocks
export const EXT_TO_LANG: Record<string, string> = {
  '.py': 'python',
  '.js': 'javascript',
  '.ts': 'typescript',
  '.tsx': 'tsx',
  '.jsx': 'jsx',
  '.rs': 'rust',
  '.go': 'go',
  '.java': 'java',
  '.c': 'c',
  '.cpp': 'cpp',
  '.h': 'c',
  '.hpp': 'cpp',
  '.cs': 'csharp',
  '.rb': 'ruby',
  '.php': 'php',
  '.html': 'html',
  '.css': 'css',
  '.json': 'json',
  '.yaml': 'yaml',
  '.yml': 'yaml',
  '.toml': 'toml',
  '.md': 'markdown',
  '.txt': 'text',
  '.sh': 'bash',
  '.bat': 'batch',
  '.sql': 'sql',
  '.nim': 'nim',
  '.ex': 'elixir',
  '.exs': 'elixir',
  '.kt': 'kotlin',
  '.swift': 'swift',
  '.r': 'r',
  '.lua': 'lua',
  '.pl': 'perl',
  '.scala': 'scala',
  '.zig': 'zig',
  '.v': 'v',
  '.dart': 'dart',
  '.xml': 'xml',
  '.log': 'text',
  '.cfg': 'ini',
  '.ini': 'ini',
};

export function getFileExtension(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot >= 0 ? name.slice(dot).toLowerCase() : '';
}

export function formatCharCount(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return `${n}`;
}

/** File accept string for the hidden file input */
export const FILE_ACCEPT =
  '.pdf,.docx,.pptx,.xlsx,.xls,.xlsm,.epub,.odt,.rtf,.zip,.csv,.eml,.txt,.json,.xml,.md,.rs,.py,.js,.ts,.tsx,.jsx,.html,.css,.toml,.yaml,.yml,.sh,.bat,.c,.cpp,.h,.hpp,.cs,.go,.java,.rb,.php,.sql,.log,.cfg,.ini,.nim,.ex,.exs,.kt,.swift,.r,.lua,.pl,.scala,.zig,.v,.dart,.png,.jpg,.jpeg,.gif,.webp';

function warnFileTooLarge(name: string, sizeKb: string): void {
  // Intentional user-facing warning for oversized files
  alert(`File "${name}" exceeds the 100KB size limit (${sizeKb}KB).`);
}

async function processImageFile(file: File, addImage: (url: string) => void): Promise<void> {
  const reader = new FileReader();
  reader.onload = (ev) => {
    const dataUrl = ev.target?.result as string;
    if (dataUrl) addImage(dataUrl);
  };
  reader.readAsDataURL(file);
}

async function processTextFile(file: File, addFile: (f: AttachedFile) => void): Promise<boolean> {
  if (file.size > MAX_TEXT_FILE_SIZE) {
    warnFileTooLarge(file.name, (file.size / 1024).toFixed(1));
    return false;
  }
  const text = await file.text();
  addFile({ id: nextFileId(), name: file.name, text });
  return true;
}

async function processDocumentFile(
  file: File,
  addFile: (f: AttachedFile) => void,
  setExtracting: (fn: (prev: number) => number) => void,
): Promise<void> {
  setExtracting((prev) => prev + 1);
  try {
    const buf = await file.arrayBuffer();
    const res = await fetch(`/api/file/extract-text?filename=${encodeURIComponent(file.name)}`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/octet-stream' },
      body: buf,
    });
    const data = await res.json();
    if (data.success && data.text) {
      addFile({ id: nextFileId(), name: file.name, text: data.text });
    }
  } finally {
    setExtracting((prev) => prev - 1);
  }
}

export function useFileAttachments(hasVision: boolean) {
  const [attachedImages, setAttachedImages] = useState<string[]>([]);
  const [attachedFiles, setAttachedFiles] = useState<AttachedFile[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const [isExtracting, setIsExtracting] = useState(0);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const dragCounterRef = useRef(0);

  const addImage = useCallback((url: string) => {
    setAttachedImages((prev) => [...prev, url]);
  }, []);

  const addFile = useCallback((f: AttachedFile) => {
    setAttachedFiles((prev) => [...prev, f]);
  }, []);

  const processFiles = useCallback(
    async (files: File[]) => {
      for (const file of files) {
        const ext = getFileExtension(file.name);

        if (IMAGE_EXTENSIONS.has(ext) && hasVision) {
          await processImageFile(file, addImage);
          continue;
        }
        if (TEXT_EXTENSIONS.has(ext)) {
          await processTextFile(file, addFile);
          continue;
        }
        if (DOCUMENT_EXTENSIONS.has(ext)) {
          await processDocumentFile(file, addFile, setIsExtracting);
          continue;
        }
        // Unknown extension -- try as text
        try {
          await processTextFile(file, addFile);
        } catch {
          // Could not read file
        }
      }
    },
    [hasVision, addImage, addFile],
  );

  const handlePaste = useCallback(
    (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
      const items = e.clipboardData?.items;
      if (!items) return;

      const pastedFiles: File[] = [];
      for (const item of items) {
        if (item.kind === 'file') {
          const file = item.getAsFile();
          if (file) pastedFiles.push(file);
        }
      }
      if (pastedFiles.length === 0) return;

      const relevantFiles = pastedFiles.filter((f) => {
        const ext = getFileExtension(f.name);
        if (IMAGE_EXTENSIONS.has(ext)) return hasVision;
        return true;
      });
      if (relevantFiles.length === 0) return;

      e.preventDefault();
      processFiles(relevantFiles);
    },
    [hasVision, processFiles],
  );

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

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      e.stopPropagation();
      dragCounterRef.current = 0;
      setIsDragging(false);

      const droppedFiles = Array.from(e.dataTransfer.files);
      if (droppedFiles.length > 0) {
        processFiles(droppedFiles);
      }
    },
    [processFiles],
  );

  const handleFileButtonClick = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleFileInputChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const selectedFiles = Array.from(e.target.files || []);
      if (selectedFiles.length > 0) {
        processFiles(selectedFiles);
      }
      e.target.value = '';
    },
    [processFiles],
  );

  const removeImage = useCallback((index: number) => {
    setAttachedImages((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const removeFile = useCallback((id: string) => {
    setAttachedFiles((prev) => prev.filter((f) => f.id !== id));
  }, []);

  const clearAll = useCallback(() => {
    setAttachedImages([]);
    setAttachedFiles([]);
  }, []);

  return {
    attachedImages,
    attachedFiles,
    isDragging,
    isExtracting,
    fileInputRef,
    handlePaste,
    handleDragEnter,
    handleDragLeave,
    handleDragOver,
    handleDrop,
    handleFileButtonClick,
    handleFileInputChange,
    removeImage,
    removeFile,
    clearAll,
  };
}
