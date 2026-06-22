// eslint-disable-next-line react-doctor/prefer-dynamic-import
import type { Chart as ChartInstance } from 'chart.js';
import mermaid from 'mermaid';
import React, { useEffect, useRef, useState, useCallback } from 'react';
import { createPortal } from 'react-dom';
import { useTranslation } from 'react-i18next';
import ReactMarkdown from 'react-markdown';
import type { Components } from 'react-markdown';
import remarkGfm from 'remark-gfm';

import { SyntaxHighlighter, dracula } from '../../utils/syntaxHighlighterSetup';

import { ExpandableBlock, ThreeDotMenu } from './ExpandableBlock';

// Initialize mermaid — theme is re-applied per render based on current mode
const isDark = () => document.documentElement.classList.contains('dark');
mermaid.initialize({
  startOnLoad: false,
  theme: 'base',
  securityLevel: 'loose',
  fontFamily: 'inherit',
  themeVariables: {
    // Background adapts to theme (re-initialized on render)
    background: '#1a1a2e',
    primaryColor: '#3182ce',
    primaryTextColor: '#ffffff',
    primaryBorderColor: '#2b6cb0',
    secondaryColor: '#2f855a',
    secondaryTextColor: '#ffffff',
    secondaryBorderColor: '#276749',
    tertiaryColor: '#805ad5',
    tertiaryTextColor: '#ffffff',
    tertiaryBorderColor: '#6b46c1',
    // Lines and labels
    lineColor: '#a0aec0',
    textColor: '#e2e8f0',
    // Flowchart
    nodeBorder: '#4a5568',
    mainBkg: '#3182ce',
    nodeTextColor: '#ffffff',
    // Pie chart
    pie1: '#4dc9f6',
    pie2: '#f67019',
    pie3: '#f53794',
    pie4: '#537bc4',
    pie5: '#acc236',
    pie6: '#166a8f',
    pieTextColor: '#e2e8f0',
    pieSectionTextColor: '#e2e8f0',
    pieTitleTextColor: '#e2e8f0',
    // Notes
    noteBkgColor: '#2d3748',
    noteTextColor: '#e2e8f0',
    noteBorderColor: '#4a5568',
  },
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
  const { t } = useTranslation();
  const containerRef = useRef<HTMLDivElement>(null);
  const [svg, setSvg] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const RADIX_36 = 36;
    const ID_SLICE_END = 9;
    const id = `mermaid-${Math.random().toString(RADIX_36).slice(2, ID_SLICE_END)}`;
    mermaid
      .render(id, code.trim())
      .then(({ svg: renderedSvg }) => {
        if (!cancelled) setSvg(renderedSvg);
      })
      .catch((err) => {
        if (!cancelled) setError(String(err));
      });
    return () => {
      cancelled = true;
    };
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
    img.src = `data:image/svg+xml;base64,${btoa(unescape(encodeURIComponent(svgData)))}`;
  }, []);

  if (error) {
    return (
      <div className="my-2 rounded border border-red-700 bg-red-900/30 p-3 text-sm">
        <div className="mb-1 font-medium text-red-400">{t('markdown.mermaidError')}</div>
        <pre className="whitespace-pre-wrap text-xs text-red-300">{error}</pre>
        <pre className="mt-2 whitespace-pre-wrap text-xs text-muted-foreground">{code}</pre>
      </div>
    );
  }

  if (!svg) {
    return (
      <div className="my-2 animate-pulse rounded bg-muted p-4 text-sm text-muted-foreground">
        {t('markdown.renderingDiagram')}
      </div>
    );
  }

  return (
    <ExpandableBlock actions={[{ label: 'Export PNG', onClick: handleExport }]}>
      <div
        ref={containerRef}
        className="w-full overflow-x-auto rounded-lg bg-muted/50 p-4 dark:bg-[#1a1a2e] [&_.edgeLabel]:!text-gray-700 dark:[&_.edgeLabel]:!text-gray-200 [&_.flowchart-link]:!stroke-gray-400 [&_.label]:!text-gray-900 [&_.nodeLabel]:!text-gray-900 [&_text]:!fill-gray-700 dark:[&_text]:!fill-gray-200"
        style={{ ['--mermaid-node-text' as string]: '#1a202c' }}
        // eslint-disable-next-line react/no-danger
        dangerouslySetInnerHTML={{ __html: svg }}
      />
    </ExpandableBlock>
  );
};

/** Export chart data as CSV file. */
function exportChartCsv(code: string) {
  try {
    const spec = JSON.parse(code);
    const labels = spec.labels || [];
    const datasets = spec.datasets || [];
    const header = ['Label', ...datasets.map((d: { label?: string }) => d.label || 'Value')].join(
      ',',
    );
    const rows = labels.map((l: string, i: number) =>
      [l, ...datasets.map((d: { data: number[] }) => d.data[i] ?? '')].join(','),
    );
    const csv = [header, ...rows].join('\n');
    const blob = new Blob([csv], { type: 'text/csv' });
    const a = document.createElement('a');
    a.download = 'chart.csv';
    a.href = URL.createObjectURL(blob);
    a.click();
  } catch {
    /* ignore */
  }
}

/** Chart.js-powered data chart from JSON spec. */
const ChartBlock: React.FC<{ code: string }> = ({ code }) => {
  const { t } = useTranslation();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const chartRef = useRef<ChartInstance | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!canvasRef.current) return;
    let spec: {
      type?: string;
      title?: string;
      labels?: string[];
      datasets?: Array<{
        label?: string;
        data: number[];
        backgroundColor?: string | string[];
        borderColor?: string;
      }>;
    };
    try {
      spec = JSON.parse(code);
    } catch {
      setError('Invalid JSON chart spec');
      return;
    }

    const chartType = (spec.type || 'bar') as
      | 'bar'
      | 'line'
      | 'pie'
      | 'doughnut'
      | 'radar'
      | 'scatter'
      | 'polarArea';
    const palette = [
      '#4dc9f6',
      '#f67019',
      '#f53794',
      '#537bc4',
      '#acc236',
      '#166a8f',
      '#00a950',
      '#58595b',
      '#8549ba',
    ];

    // Lazy import Chart.js to avoid bundling if unused
    import('chart.js').then(({ Chart, registerables }) => {
      Chart.register(...registerables);
      if (chartRef.current) chartRef.current.destroy();

      const datasets = (spec.datasets || []).map((ds, i) => ({
        label: ds.label || `Dataset ${i + 1}`,
        data: ds.data,
        backgroundColor:
          ds.backgroundColor ||
          (chartType === 'pie' || chartType === 'doughnut' || chartType === 'polarArea'
            ? palette.slice(0, ds.data.length)
            : `${palette[i % palette.length]}99`),
        borderColor: ds.borderColor || palette[i % palette.length],
        borderWidth: chartType === 'line' ? 2 : 1,
        tension: 0.3,
      }));

      const canvas = canvasRef.current;
      if (!canvas) return;
      chartRef.current = new Chart(canvas, {
        type: chartType,
        data: { labels: spec.labels || [], datasets },
        options: {
          responsive: true,
          maintainAspectRatio: true,
          plugins: {
            title: spec.title
              ? {
                  display: true,
                  text: spec.title,
                  color: isDark() ? '#e0e0e0' : '#1a202c',
                  font: { size: 14 },
                }
              : undefined,
            legend: { labels: { color: isDark() ? '#c0c0c0' : '#374151' } },
          },
          scales:
            chartType !== 'pie' &&
            chartType !== 'doughnut' &&
            chartType !== 'radar' &&
            chartType !== 'polarArea'
              ? {
                  x: {
                    ticks: { color: isDark() ? '#a0a0a0' : '#4b5563' },
                    grid: { color: isDark() ? '#333' : '#e5e7eb' },
                  },
                  y: {
                    ticks: { color: isDark() ? '#a0a0a0' : '#4b5563' },
                    grid: { color: isDark() ? '#333' : '#e5e7eb' },
                  },
                }
              : undefined,
        },
      });
    });

    return () => {
      chartRef.current?.destroy();
    };
  }, [code]);

  const handleExport = useCallback(
    (format: 'png' | 'csv') => {
      if (format === 'png' && canvasRef.current) {
        const a = document.createElement('a');
        a.download = 'chart.png';
        a.href = canvasRef.current.toDataURL('image/png');
        a.click();
      } else if (format === 'csv') {
        exportChartCsv(code);
      }
    },
    [code],
  );

  if (error) {
    return (
      <div className="my-2 rounded border border-red-700 bg-red-900/30 p-3 text-sm">
        <div className="mb-1 font-medium text-red-400">{t('markdown.chartError')}</div>
        <pre className="whitespace-pre-wrap text-xs text-red-300">{error}</pre>
      </div>
    );
  }

  return (
    <ExpandableBlock
      actions={[
        { label: 'Export PNG', onClick: () => handleExport('png') },
        { label: 'Export CSV', onClick: () => handleExport('csv') },
      ]}
    >
      <div className="w-full rounded-lg bg-muted/50 p-4 dark:bg-[#1a1a2e]">
        <canvas ref={canvasRef} />
      </div>
    </ExpandableBlock>
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
    <SyntaxHighlighter style={dracula} language={language} PreTag="div">
      {content}
    </SyntaxHighlighter>
  ) : (
    <code className={`${className ?? ''} rounded bg-muted px-2 py-1 font-mono text-sm`}>
      {content}
    </code>
  );
};

/** Image with lightbox modal and 3-dot menu. */
const ImageWithControls: React.FC<React.ImgHTMLAttributes<HTMLImageElement>> = (props) => {
  const [isOpen, setIsOpen] = useState(false);
  const src = props.src || '';
  const alt = props.alt || 'image';

  const handleDownload = useCallback(() => {
    const a = document.createElement('a');
    a.href = src;
    a.download = `${alt.replace(/[^a-zA-Z0-9]/g, '_')}.jpg`;
    a.click();
  }, [src, alt]);

  return (
    <>
      <div className="group relative my-2 inline-block">
        <div
          className="cursor-pointer"
          onClick={() => setIsOpen(true)}
          role="button"
          tabIndex={0}
          onKeyDown={(e) => {
            if (e.key === 'Enter') setIsOpen(true);
          }}
        >
          <img
            src={src}
            alt={alt}
            className="max-h-[400px] max-w-full rounded-lg border border-border/50 transition-colors hover:border-primary/50"
            loading="lazy"
          />
        </div>
        <ThreeDotMenu actions={[{ label: 'Download', onClick: handleDownload }]} />
      </div>
      {!!isOpen &&
        createPortal(
          <div
            className="fixed inset-0 z-[9999] flex cursor-pointer items-center justify-center bg-black/95"
            role="button"
            tabIndex={0}
            onClick={() => setIsOpen(false)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === 'Escape') setIsOpen(false);
            }}
          >
            <img
              src={src}
              alt={alt}
              className="h-full w-full object-contain p-2"
              role="presentation"
              onClick={(e) => e.stopPropagation()}
            />
            <button
              onClick={() => setIsOpen(false)}
              className="absolute right-4 top-4 rounded-full bg-white/20 p-2 text-white backdrop-blur transition-colors hover:bg-white/30"
              title="Close"
            >
              <svg
                width="20"
                height="20"
                viewBox="0 0 20 20"
                fill="none"
                stroke="currentColor"
                strokeWidth="2"
              >
                <path d="M5 5l10 10M15 5L5 15" />
              </svg>
            </button>
          </div>,
          document.body,
        )}
    </>
  );
};

/* Module-level component definitions for ReactMarkdown (avoids unstable nested components). */
const MarkdownPre = ({ children }: { children?: React.ReactNode }) => (
  <div className="my-2">{children}</div>
);
const MarkdownP = ({ children }: { children?: React.ReactNode }) => (
  <p className="my-2 [overflow-wrap:anywhere]">{children}</p>
);
const MarkdownH1 = ({ children }: { children?: React.ReactNode }) => (
  <h1 className="my-3 border-b border-border pb-2 text-2xl font-semibold">{children}</h1>
);
const MarkdownH2 = ({ children }: { children?: React.ReactNode }) => (
  <h2 className="my-3 border-b border-border pb-2 text-xl font-semibold">{children}</h2>
);
const MarkdownH3 = ({ children }: { children?: React.ReactNode }) => (
  <h3 className="my-2 text-lg font-semibold">{children}</h3>
);
const MarkdownUl = ({ children }: { children?: React.ReactNode }) => (
  <ul className="my-2 ml-4 list-disc">{children}</ul>
);
const MarkdownOl = ({ children }: { children?: React.ReactNode }) => (
  <ol className="my-2 ml-4 list-decimal">{children}</ol>
);
const MarkdownLi = ({ children }: { children?: React.ReactNode }) => <li>{children}</li>;
const MarkdownStrong = ({ children }: { children?: React.ReactNode }) => (
  <strong className="font-bold">{children}</strong>
);
const MarkdownEm = ({ children }: { children?: React.ReactNode }) => (
  <em className="italic">{children}</em>
);
const MarkdownBlockquote = ({ children }: { children?: React.ReactNode }) => (
  <blockquote className="my-2 border-l-2 border-muted-foreground/30 bg-muted/20 pl-4 italic">
    {children}
  </blockquote>
);

const markdownComponents: Components = {
  code: CodeBlock,
  img: ImageWithControls,
  pre: MarkdownPre,
  p: MarkdownP,
  h1: MarkdownH1,
  h2: MarkdownH2,
  h3: MarkdownH3,
  ul: MarkdownUl,
  ol: MarkdownOl,
  li: MarkdownLi,
  strong: MarkdownStrong,
  em: MarkdownEm,
  blockquote: MarkdownBlockquote,
};

export const MarkdownContent: React.FC<MarkdownContentProps> = ({ content, testId }) => {
  return (
    <div className="prose prose-sm max-w-none text-sm dark:prose-invert" data-testid={testId}>
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
        {content}
      </ReactMarkdown>
    </div>
  );
};
