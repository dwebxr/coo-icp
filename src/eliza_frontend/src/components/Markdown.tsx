import { memo, useMemo, type ReactNode } from 'react';
import { parseBlocks, type Block, type Inline, type ListBlock } from '../lib/markdown';
import { CopyButton } from './CopyButton';

// Very long replies are rendered as plain text past this point to keep parsing cheap.
const MAX_PARSED = 20_000;

function renderInline(nodes: Inline[], keyPrefix = ''): ReactNode[] {
  return nodes.map((node, i) => {
    const key = `${keyPrefix}${i}`;
    switch (node.type) {
      case 'text':
        return node.text;
      case 'br':
        return <br key={key} />;
      case 'code':
        return <code key={key}>{node.text}</code>;
      case 'strong':
        return <strong key={key}>{renderInline(node.children, `${key}.`)}</strong>;
      case 'em':
        return <em key={key}>{renderInline(node.children, `${key}.`)}</em>;
      case 'link':
        return (
          <a key={key} href={node.href} title={node.href} target="_blank" rel="noopener noreferrer nofollow">
            {renderInline(node.children, `${key}.`)}
          </a>
        );
    }
  });
}

function List({ block }: { block: ListBlock }) {
  const items = block.items.map((item, i) => (
    <li key={i}>
      {renderInline(item.children)}
      {item.sub && <List block={item.sub} />}
    </li>
  ));
  return block.ordered ? <ol start={block.start === 1 ? undefined : block.start}>{items}</ol> : <ul>{items}</ul>;
}

function renderBlock(block: Block, i: number): ReactNode {
  switch (block.type) {
    case 'paragraph':
      return <p key={i}>{renderInline(block.children)}</p>;
    case 'heading': {
      const Tag = (['h3', 'h4', 'h5'] as const)[block.level - 1];
      return <Tag key={i}>{renderInline(block.children)}</Tag>;
    }
    case 'list':
      return <List key={i} block={block} />;
    case 'quote':
      return <blockquote key={i}>{block.children.map(renderBlock)}</blockquote>;
    case 'hr':
      return <hr key={i} />;
    case 'code':
      return (
        <div key={i} className="md-code">
          <div className="md-code-bar">
            <span>{block.lang || 'code'}</span>
            <CopyButton text={block.text} label="Copy code" compact />
          </div>
          <pre>
            <code>{block.text}</code>
          </pre>
        </div>
      );
  }
}

export const Markdown = memo(function Markdown({ text }: { text: string }) {
  const content = useMemo(() => {
    const head = text.slice(0, MAX_PARSED);
    const tail = text.slice(MAX_PARSED);
    return (
      <>
        {parseBlocks(head).map(renderBlock)}
        {tail && <p className="md-plain">{tail}</p>}
      </>
    );
  }, [text]);
  return <div className="md">{content}</div>;
});
