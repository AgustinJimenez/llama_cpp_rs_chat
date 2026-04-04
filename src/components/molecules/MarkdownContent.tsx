import React, { useEffect, useRef, useState, useCallback } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import mermaid from 'mermaid';
import { SyntaxHighlighter, dracula } from '../../utils/syntaxHighlighterSetup';
import type { Components } from 'react-markdown';

// Initialize mermaid with dark theme
mermaid.initialize({
  startOnLoad: false,
  theme: 'dark',
  securityLevel: 'loose',
  fontFamily: 'inherit',
});

type CodeBlockProps = {
  inline?: boolean;
  className?: string;
  children?: React.ReactNode;
};

interface MarkdownContentProps {
  content: string;
  testId?: string;
}

/** Renders a mermaid diagram from source code. */
const MermaidBlock: React.FC<{ code: string }> = ({ code }) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const [svg, setSvg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const id = `mermaid-${Math.random().toString(36).slice(2, 9)}`;
    mermaid.render(id, code.trim()).then(({ svg: renderedSvg }) => {
      if (!cancelled) setSvg(renderedSvg);
    }).catch((err) => {
      if (!cancelled) setError(String(err));
    });
    return () => { cancelled = true; };
  }, [code]);

  const handleExport = useCallback(() => {
    if (!containerRef.current) return;
    const svgEl = containerRef.current.querySelector('svg');
    if (!svgEl) return;
    // Export as PNG via canvas
    const svgData = new XMLSerializer().serializeToString(svgEl);
    const canvas = document.createElement('canvas');
    const ctx = canvas.getContext('2d');
    if (!ctx) return;
    const img = new Image();
    img.onload = () => {
      canvas.width = img.width * 2;
      canvas.height = img.height * 2;
      ctx.scale(2, 2);
      ctx.drawImage(img, 0, 0);
      const a = document.createElement('a');
      a.download = 'diagram.png';
      a.href = canvas.toDataURL('image/png');
      a.click();
    };
    img.src = 'data:image/svg+xml;base64,' + btoa(unescape(encodeURIComponent(svgData)));
  }, []);

  if (error) {
    return (
      <div className="my-2 p-3 bg-red-900/30 border border-red-700 rounded text-sm">
        <div className="text-red-400 font-medium mb-1">Mermaid Error</div>
        <pre className="text-xs text-red-300 whitespace-pre-wrap">{error}</pre>
        <pre className="text-xs text-muted-foreground mt-2 whitespace-pre-wrap">{code}</pre>
      </div>
    );
  }

  if (!svg) {
    return <div className="my-2 p-4 bg-muted rounded animate-pulse text-sm text-muted-foreground">Rendering diagram...</div>;
  }

  return (
    <div className="my-2">
      <div
        ref={containerRef}
        className="bg-[#1a1a2e] rounded-lg p-4 overflow-x-auto"
        dangerouslySetInnerHTML={{ __html: svg }}
      />
      <button
        onClick={handleExport}
        className="mt-1 text-xs text-muted-foreground hover:text-foreground transition-colors"
      >
        Export PNG
      </button>
    </div>
  );
};

/** Chart.js-powered data chart from JSON spec. */
const ChartBlock: React.FC<{ code: string }> = ({ code }) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const chartRef = useRef<import('chart.js').Chart | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!canvasRef.current) return;
    let spec: { type?: string; title?: string; labels?: string[]; datasets?: Array<{ label?: string; data: number[]; backgroundColor?: string | string[]; borderColor?: string }> };
    try {
      spec = JSON.parse(code);
    } catch {
      setError('Invalid JSON chart spec');
      return;
    }

    const chartType = (spec.type || 'bar') as 'bar' | 'line' | 'pie' | 'doughnut' | 'radar' | 'scatter' | 'polarArea';
    const palette = ['#4dc9f6','#f67019','#f53794','#537bc4','#acc236','#166a8f','#00a950','#58595b','#8549ba'];

    // Lazy import Chart.js to avoid bundling if unused
    import('chart.js').then(({ Chart, registerables }) => {
      Chart.register(...registerables);
      if (chartRef.current) chartRef.current.destroy();

      const datasets = (spec.datasets || []).map((ds, i) => ({
        label: ds.label || `Dataset ${i + 1}`,
        data: ds.data,
        backgroundColor: ds.backgroundColor || (chartType === 'pie' || chartType === 'doughnut' || chartType === 'polarArea'
          ? palette.slice(0, ds.data.length)
          : palette[i % palette.length] + '99'),
        borderColor: ds.borderColor || palette[i % palette.length],
        borderWidth: chartType === 'line' ? 2 : 1,
        tension: 0.3,
      }));

      chartRef.current = new Chart(canvasRef.current!, {
        type: chartType,
        data: { labels: spec.labels || [], datasets },
        options: {
          responsive: true,
          maintainAspectRatio: true,
          plugins: {
            title: spec.title ? { display: true, text: spec.title, color: '#e0e0e0', font: { size: 14 } } : undefined,
            legend: { labels: { color: '#c0c0c0' } },
          },
          scales: chartType !== 'pie' && chartType !== 'doughnut' && chartType !== 'radar' && chartType !== 'polarArea' ? {
            x: { ticks: { color: '#a0a0a0' }, grid: { color: '#333' } },
            y: { ticks: { color: '#a0a0a0' }, grid: { color: '#333' } },
          } : undefined,
        },
      });
    });

    return () => { chartRef.current?.destroy(); };
  }, [code]);

  const handleExport = useCallback((format: 'png' | 'csv') => {
    if (format === 'png' && canvasRef.current) {
      const a = document.createElement('a');
      a.download = 'chart.png';
      a.href = canvasRef.current.toDataURL('image/png');
      a.click();
    } else if (format === 'csv') {
      try {
        const spec = JSON.parse(code);
        const labels = spec.labels || [];
        const datasets = spec.datasets || [];
        const header = ['Label', ...datasets.map((d: { label?: string }) => d.label || 'Value')].join(',');
        const rows = labels.map((l: string, i: number) =>
          [l, ...datasets.map((d: { data: number[] }) => d.data[i] ?? '')].join(',')
        );
        const csv = [header, ...rows].join('\n');
        const blob = new Blob([csv], { type: 'text/csv' });
        const a = document.createElement('a');
        a.download = 'chart.csv';
        a.href = URL.createObjectURL(blob);
        a.click();
      } catch { /* ignore */ }
    }
  }, [code]);

  if (error) {
    return (
      <div className="my-2 p-3 bg-red-900/30 border border-red-700 rounded text-sm">
        <div className="text-red-400 font-medium mb-1">Chart Error</div>
        <pre className="text-xs text-red-300 whitespace-pre-wrap">{error}</pre>
      </div>
    );
  }

  return (
    <div className="my-2">
      <div className="bg-[#1a1a2e] rounded-lg p-4">
        <canvas ref={canvasRef} />
      </div>
      <div className="mt-1 flex gap-3">
        <button onClick={() => handleExport('png')} className="text-xs text-muted-foreground hover:text-foreground transition-colors">Export PNG</button>
        <button onClick={() => handleExport('csv')} className="text-xs text-muted-foreground hover:text-foreground transition-colors">Export CSV</button>
      </div>
    </div>
  );
};

/**
 * Reusable markdown renderer with syntax highlighting.
 * Used for both user and assistant messages.
 */
const CodeBlock = ({ inline, className, children }: CodeBlockProps) => {
  const match = /language-(\w+)/.exec(className || '');
  const language = match ? match[1] : '';
  const content = String(children ?? '').replace(/\n$/, '');

  // Render mermaid diagrams
  if (!inline && language === 'mermaid') {
    return <MermaidBlock code={content} />;
  }

  // Render Chart.js charts
  if (!inline && language === 'chart') {
    return <ChartBlock code={content} />;
  }

  return !inline && language ? (
    <SyntaxHighlighter
      style={dracula}
      language={language}
      PreTag="div"
    >
      {content}
    </SyntaxHighlighter>
  ) : (
    <code className={`${className ?? ''} bg-muted px-2 py-1 rounded font-mono text-sm`}>
      {content}
    </code>
  );
};

/** Image with lightbox modal and download button. */
const ImageWithControls: React.FC<React.ImgHTMLAttributes<HTMLImageElement>> = (props) => {
  const [isOpen, setIsOpen] = useState(false);
  const src = props.src || '';
  const alt = props.alt || 'image';

  return (
    <>
      <div className="my-2 inline-block relative group">
        <img
          {...props}
          className="rounded-lg max-w-full max-h-[400px] cursor-pointer border border-border/50 hover:border-primary/50 transition-colors"
          onClick={() => setIsOpen(true)}
          loading="lazy"
        />
        <div className="absolute bottom-2 right-2 flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
          <a
            href={src}
            download={alt.replace(/[^a-zA-Z0-9]/g, '_') + '.jpg'}
            className="px-2 py-1 bg-black/70 text-white text-xs rounded hover:bg-black/90 transition-colors"
            onClick={(e) => e.stopPropagation()}
          >
            Download
          </a>
          <button
            onClick={(e) => { e.stopPropagation(); setIsOpen(true); }}
            className="px-2 py-1 bg-black/70 text-white text-xs rounded hover:bg-black/90 transition-colors"
          >
            Expand
          </button>
        </div>
      </div>
      {isOpen && (
        <div
          className="fixed inset-0 z-50 bg-black/80 flex items-center justify-center cursor-pointer"
          onClick={() => setIsOpen(false)}
        >
          <div className="relative max-w-[90vw] max-h-[90vh]" onClick={(e) => e.stopPropagation()}>
            <img src={src} alt={alt} className="max-w-full max-h-[90vh] rounded-lg" />
            <div className="absolute top-2 right-2 flex gap-2">
              <a
                href={src}
                download={alt.replace(/[^a-zA-Z0-9]/g, '_') + '.jpg'}
                className="px-3 py-1.5 bg-black/70 text-white text-sm rounded hover:bg-black/90 transition-colors"
              >
                Download
              </a>
              <button
                onClick={() => setIsOpen(false)}
                className="px-3 py-1.5 bg-black/70 text-white text-sm rounded hover:bg-black/90 transition-colors"
              >
                Close
              </button>
            </div>
          </div>
        </div>
      )}
    </>
  );
};

export const MarkdownContent: React.FC<MarkdownContentProps> = ({ content, testId }) => {
  const components: Components = {
    code: CodeBlock,
    img: ImageWithControls,
    pre: ({ children }) => <div className="my-2">{children}</div>,
    p: ({ children }) => <p className="my-2">{children}</p>,
    h1: ({ children }) => (
      <h1 className="font-bold text-2xl my-3 border-b border-border pb-2">{children}</h1>
    ),
    h2: ({ children }) => (
      <h2 className="font-bold text-xl my-3 border-b border-border pb-2">{children}</h2>
    ),
    h3: ({ children }) => (
      <h3 className="font-semibold text-lg my-2">{children}</h3>
    ),
    ul: ({ children }) => <ul className="list-disc ml-4 my-2">{children}</ul>,
    ol: ({ children }) => <ol className="list-decimal ml-4 my-2">{children}</ol>,
    li: ({ children }) => <li>{children}</li>,
    strong: ({ children }) => <strong className="font-bold">{children}</strong>,
    em: ({ children }) => <em className="italic">{children}</em>,
    blockquote: ({ children }) => (
      <blockquote className="border-l-4 border-border pl-4 my-2 italic">{children}</blockquote>
    ),
  };

  return (
    <div className="text-sm prose prose-sm max-w-none prose-invert" data-testid={testId}>
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={components}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
};
