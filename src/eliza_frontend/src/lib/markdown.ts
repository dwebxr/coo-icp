// Minimal Markdown subset for LLM replies. Produces a plain AST that is rendered
// with React elements only — raw HTML is never interpreted.

export type Inline =
  | { type: 'text'; text: string }
  | { type: 'strong'; children: Inline[] }
  | { type: 'em'; children: Inline[] }
  | { type: 'code'; text: string }
  | { type: 'link'; href: string; children: Inline[] }
  | { type: 'br' };

export interface ListItem {
  children: Inline[];
  sub?: ListBlock;
}

export interface ListBlock {
  type: 'list';
  ordered: boolean;
  start: number;
  items: ListItem[];
}

export type Block =
  | { type: 'paragraph'; children: Inline[] }
  | { type: 'heading'; level: 1 | 2 | 3; children: Inline[] }
  | ListBlock
  | { type: 'quote'; children: Block[] }
  | { type: 'code'; lang: string; text: string }
  | { type: 'hr' };

const ESCAPABLE = '\\`*_[]()#+-.!>~|';

// Nesting limits keep recursion shallow for adversarial input (e.g. thousands of nested quotes).
const MAX_INLINE_DEPTH = 6;
const MAX_QUOTE_DEPTH = 4;

export function safeHref(url: string): string | null {
  if (!/^https?:\/\/[^\s]+$/i.test(url)) return null;
  try {
    const { protocol } = new URL(url);
    return protocol === 'http:' || protocol === 'https:' ? url : null;
  } catch {
    return null;
  }
}

const isWordChar = (ch: string | undefined) => !!ch && /[\p{L}\p{N}_]/u.test(ch);

// Span patterns are length-bounded so a reply full of unmatched markers cannot cause quadratic scans.
const MAX_CODE_SPAN = 1000;
const IMAGE = /^!\[([^\]\n]{0,300})\]\(([^)\s]{1,2000})\)/;
const LINK = /^\[([^\]\n]{1,300})\]\(([^)\s]{1,2000})\)/;
const STRONG_STAR = /^\*\*(?=\S)([\s\S]{0,400}?\S)\*\*/;
const STRONG_UNDER = /^__(?=\S)([\s\S]{0,400}?\S)__(?![\p{L}\p{N}_])/u;
const EM_STAR = /^\*(?=[^\s*])([\s\S]{0,400}?[^\s*])\*(?!\*)/;
const SPECIAL = '\\\n`[!h*_';
const EM_UNDER = /^_(?=[^\s_])([\s\S]{0,400}?[^\s_])_(?![\p{L}\p{N}_])/u;

export function parseInline(src: string, depth = 0): Inline[] {
  if (depth > MAX_INLINE_DEPTH) return src ? [{ type: 'text', text: src }] : [];
  const out: Inline[] = [];
  let buf = '';
  const flush = () => {
    if (buf) {
      out.push({ type: 'text', text: buf });
      buf = '';
    }
  };
  const push = (node: Inline, length: number) => {
    flush();
    out.push(node);
    i += length;
  };

  let i = 0;
  while (i < src.length) {
    const ch = src[i];
    // Fast path: only these characters can start a construct.
    if (!SPECIAL.includes(ch)) {
      buf += ch;
      i += 1;
      continue;
    }
    const prev = src[i - 1];
    const rest = src.slice(i);
    let m: RegExpExecArray | null;

    if (ch === '\\' && i + 1 < src.length && ESCAPABLE.includes(src[i + 1])) {
      buf += src[i + 1];
      i += 2;
      continue;
    }
    if (ch === '\n') {
      push({ type: 'br' }, 1);
      continue;
    }
    if (ch === '`') {
      // Measure the opening run once and look for a closing run of the same length within a bounded window.
      // An unmatched run is emitted as text in one step, so long runs of backticks stay linear.
      let run = 1;
      while (src[i + run] === '`') run += 1;
      const fence = '`'.repeat(run);
      const limit = Math.min(src.length, i + run + MAX_CODE_SPAN + run);
      let close = -1;
      let from = i + run;
      while (from < limit) {
        const at = src.indexOf(fence, from);
        if (at === -1 || at + run > limit) break;
        let end = at + run;
        if (src[end] !== '`') {
          close = at;
          break;
        }
        while (src[end] === '`') end += 1;
        from = end;
      }
      if (close > i + run) {
        const body = src.slice(i + run, close);
        const text = body.length > 2 && body.startsWith(' ') && body.endsWith(' ') ? body.slice(1, -1) : body;
        push({ type: 'code', text }, close + run - i);
      } else {
        buf += fence;
        i += run;
      }
      continue;
    }
    // Images are never loaded (model output could embed tracking URLs); they render as links.
    if (ch === '!' && (m = IMAGE.exec(rest))) {
      const href = safeHref(m[2]);
      const label = m[1] || 'image';
      if (href) push({ type: 'link', href, children: [{ type: 'text', text: label }] }, m[0].length);
      else push({ type: 'text', text: label }, m[0].length);
      continue;
    }
    if (ch === '[' && (m = LINK.exec(rest))) {
      const href = safeHref(m[2]);
      const children = parseInline(m[1], depth + 1);
      if (href) push({ type: 'link', href, children }, m[0].length);
      else {
        flush();
        out.push(...children);
        i += m[0].length;
      }
      continue;
    }
    if (ch === 'h' && !isWordChar(prev) && (m = /^https?:\/\/[^\s<>()[\]{}"']+/i.exec(rest))) {
      const url = m[0].replace(/[.,;:!?]+$/, '');
      push({ type: 'link', href: url, children: [{ type: 'text', text: url }] }, url.length);
      continue;
    }
    if (ch === '*' || ch === '_') {
      const canOpen = ch === '*' || !isWordChar(prev);
      if (canOpen) {
        const strong = (ch === '*' ? STRONG_STAR : STRONG_UNDER).exec(rest);
        if (strong) {
          push({ type: 'strong', children: parseInline(strong[1], depth + 1) }, strong[0].length);
          continue;
        }
        const em = (ch === '*' ? EM_STAR : EM_UNDER).exec(rest);
        if (em) {
          push({ type: 'em', children: parseInline(em[1], depth + 1) }, em[0].length);
          continue;
        }
      }
    }
    buf += ch;
    i += 1;
  }
  flush();
  return out;
}

const indentOf = (line: string) => {
  let n = 0;
  for (const c of line) {
    if (c === ' ') n += 1;
    else if (c === '\t') n += 4;
    else break;
  }
  return n;
};

const FENCE = /^\s*(```+|~~~+)\s*([\w+#.-]*)\s*$/;
const HEADING = /^\s{0,3}(#{1,6})\s+(.*?)\s*#*\s*$/;
const HR = /^\s{0,3}([-*_])(\s*\1){2,}\s*$/;
const QUOTE = /^\s{0,3}>\s?(.*)$/;
const BULLET = /^(\s*)[-*+]\s+(.*)$/;
const ORDERED = /^(\s*)(\d{1,9})[.)]\s+(.*)$/;

interface ListFrame {
  indent: number;
  block: ListBlock;
}

export function parseBlocks(src: string, depth = 0): Block[] {
  const lines = src.replace(/\r\n?/g, '\n').split('\n');
  const blocks: Block[] = [];
  let para: string[] = [];
  let stack: ListFrame[] = [];
  let afterBlank = false;

  const closePara = () => {
    if (para.length) {
      blocks.push({ type: 'paragraph', children: parseInline(para.join('\n').trim()) });
      para = [];
    }
  };
  const closeLists = () => {
    stack = [];
  };
  const appendToItem = (text: string, blank: boolean) => {
    const items = stack[stack.length - 1].block.items;
    const item = items[items.length - 1];
    item.children.push({ type: 'br' }, ...(blank ? [{ type: 'br' } as Inline] : []), ...parseInline(text.trim()));
  };

  for (let li = 0; li < lines.length; li++) {
    const line = lines[li];

    const fence = FENCE.exec(line);
    if (fence) {
      closePara();
      closeLists();
      const closer = fence[1][0];
      const body: string[] = [];
      li++;
      while (li < lines.length && !new RegExp(`^\\s*\\${closer}{${fence[1].length},}\\s*$`).test(lines[li])) {
        body.push(lines[li]);
        li++;
      }
      blocks.push({ type: 'code', lang: fence[2], text: body.join('\n') });
      afterBlank = false;
      continue;
    }

    if (!line.trim()) {
      closePara();
      afterBlank = true;
      continue;
    }

    const bullet = BULLET.exec(line);
    const ordered = bullet ? null : ORDERED.exec(line);
    if ((bullet || ordered) && !HR.test(line)) {
      closePara();
      const indent = indentOf(line);
      const isOrdered = !!ordered;
      const content = bullet ? bullet[2] : ordered![3];
      const item: ListItem = { children: parseInline(content.trim()) };

      while (stack.length > 1 && indent < stack[stack.length - 1].indent) stack.pop();
      const top = stack[stack.length - 1];

      if (!top) {
        const block: ListBlock = { type: 'list', ordered: isOrdered, start: ordered ? Number(ordered[2]) : 1, items: [item] };
        blocks.push(block);
        stack = [{ indent, block }];
      } else if (indent >= top.indent + 2) {
        const parentItem = top.block.items[top.block.items.length - 1];
        if (parentItem.sub && parentItem.sub.ordered === isOrdered) {
          parentItem.sub.items.push(item);
        } else {
          parentItem.sub = { type: 'list', ordered: isOrdered, start: ordered ? Number(ordered[2]) : 1, items: [item] };
        }
        stack.push({ indent, block: parentItem.sub });
      } else if (top.block.ordered !== isOrdered && stack.length === 1) {
        const block: ListBlock = { type: 'list', ordered: isOrdered, start: ordered ? Number(ordered[2]) : 1, items: [item] };
        blocks.push(block);
        stack = [{ indent, block }];
      } else {
        top.block.items.push(item);
      }
      afterBlank = false;
      continue;
    }

    if (stack.length) {
      const indent = indentOf(line);
      if (indent >= 2 || !afterBlank) {
        if (!(HEADING.test(line) || QUOTE.test(line) || HR.test(line))) {
          appendToItem(line, afterBlank);
          afterBlank = false;
          continue;
        }
      }
      closeLists();
    }

    const heading = HEADING.exec(line);
    if (heading) {
      closePara();
      const level = Math.min(heading[1].length, 3) as 1 | 2 | 3;
      blocks.push({ type: 'heading', level, children: parseInline(heading[2]) });
      afterBlank = false;
      continue;
    }

    if (HR.test(line)) {
      closePara();
      blocks.push({ type: 'hr' });
      afterBlank = false;
      continue;
    }

    if (QUOTE.test(line) && depth < MAX_QUOTE_DEPTH) {
      closePara();
      const inner: string[] = [];
      while (li < lines.length && QUOTE.test(lines[li])) {
        inner.push(QUOTE.exec(lines[li])![1]);
        li++;
      }
      li--;
      blocks.push({ type: 'quote', children: parseBlocks(inner.join('\n'), depth + 1) });
      afterBlank = false;
      continue;
    }

    para.push(line.trim());
    afterBlank = false;
  }
  closePara();
  return blocks;
}

/** Plain text fallback used for copy-to-clipboard of code blocks etc. */
export function inlineText(nodes: Inline[]): string {
  return nodes
    .map((n) => {
      switch (n.type) {
        case 'text':
        case 'code':
          return n.text;
        case 'br':
          return '\n';
        default:
          return inlineText(n.children);
      }
    })
    .join('');
}
