// Minimal Markdown subset for LLM replies. Produces a plain AST that is rendered
// with React elements only — raw HTML is never interpreted.

export type Inline =
  | { type: 'text'; text: string }
  | { type: 'strong'; children: Inline[] }
  | { type: 'em'; children: Inline[] }
  | { type: 'code'; text: string }
  | { type: 'link'; href: string; children: Inline[] }
  /** The last child of a link whose text is not its own address: where it really goes (see `linkTo`). */
  | { type: 'linkHost'; text: string }
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

// Block constructs are only recognised on lines up to this length; a longer line is paragraph text.
// Real markup lines are short, and the cap bounds the work any line-level pattern can do.
const MAX_BLOCK_LINE = 2000;

export function safeHref(url: string): string | null {
  if (!/^https?:\/\/[^\s]+$/i.test(url)) return null;
  try {
    const { protocol, username, password } = new URL(url);
    // https://good.example@evil.example/ would read as "good.example" but go to evil.example.
    if (username || password) return null;
    return protocol === 'http:' || protocol === 'https:' ? url : null;
  } catch {
    return null;
  }
}

/**
 * Where a link really goes, as it is shown next to its text: the host as the browser will use it (a port other than
 * the default included, an international domain name in its punycode form), with `http://` in front when the link is
 * not https.
 */
export function destinationOf(href: string): string {
  const url = new URL(href);
  return url.protocol === 'https:' ? url.host : `http://${url.host}`;
}

/**
 * The authority of a URL as it was typed (between "//" and the next "/", "?", "#" or the end), lower-cased; the
 * URL parser's host is the same unless the parser changed something (an international name, percent-encoding, a
 * default port, an IPv6 form, …).
 */
const typedAuthority = (href: string) => /^https?:\/\/([^/?#]*)/i.exec(href)?.[1].toLowerCase() ?? null;

/**
 * A link to `href` (already checked by safeHref) showing `label`. Its real destination always follows the label, in
 * parentheses and inside the link — with one exception: the visible text IS the address (a bare URL, or a label that
 * is exactly the URL) and names its host just as the browser will use it. There is no guessing at what a label
 * means, so nothing in a label can hide where a link goes.
 */
function linkTo(href: string, label: Inline[]): Inline {
  const shown = destinationOf(href);
  const itself = inlineText(label) === href && typedAuthority(href) === new URL(href).host;
  return { type: 'link', href, children: itself ? label : [...label, { type: 'linkHost', text: shown }] };
}

// Drops trailing sentence punctuation from an autolink with a scan; an unanchored /[.,;:!?]+$/ is quadratic on long runs.
function trimUrlEnd(url: string): string {
  let end = url.length;
  while (end > 0 && '.,;:!?'.includes(url[end - 1])) end -= 1;
  return url.slice(0, end);
}

const isWordChar = (ch: string | undefined) => !!ch && /[\p{L}\p{N}_]/u.test(ch);

// Span patterns are length-bounded (and a code span's closer is looked for in a window of its own), so the work for
// each marker is bounded and a reply full of unmatched markers cannot cause quadratic scans.
const MAX_CODE_SPAN = 1000;
const IMAGE = /^!\[([^\]\n]{0,300})\]\(([^)\s]{1,2000})\)/;
const LINK = /^\[([^\]\n]{1,300})\]\(([^)\s]{1,2000})\)/;
const STRONG_STAR = /^\*\*(?=\S)([\s\S]{0,400}?\S)\*\*/;
const STRONG_UNDER = /^__(?=\S)([\s\S]{0,400}?\S)__(?![\p{L}\p{N}_])/u;
const EM_STAR = /^\*(?=[^\s*])([\s\S]{0,400}?[^\s*])\*(?!\*)/;
const SPECIAL = '\\\n`[!h*_';
const EM_UNDER = /^_(?=[^\s_])([\s\S]{0,400}?[^\s_])_(?![\p{L}\p{N}_])/u;

// inLabel: inside a link label, where bare URLs and nested links stay plain text (no link in a link).
export function parseInline(src: string, depth = 0, inLabel = false): Inline[] {
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
      // Measure the opening run once and look for a closing run of the same length in the window that follows it
      // (MAX_CODE_SPAN characters, plus room for the closing run) — only there: the scan stops at the window's end,
      // so each opening run costs at most its own length plus the window. An unmatched run is emitted as text in
      // one step.
      let run = 1;
      while (src[i + run] === '`') run += 1;
      const fence = '`'.repeat(run);
      const limit = Math.min(src.length, i + run + MAX_CODE_SPAN + run);
      let close = -1;
      for (let k = i + run; k < limit; ) {
        if (src[k] !== '`') {
          k += 1;
          continue;
        }
        let end = k + 1;
        while (end < limit && src[end] === '`') end += 1;
        // A run that reaches the window's end and goes on past it is longer than it looks here: not a match.
        if (end - k === run && src[end] !== '`') {
          close = k;
          break;
        }
        k = end;
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
      if (href && !inLabel) push(linkTo(href, [{ type: 'text', text: label }]), m[0].length);
      else push({ type: 'text', text: label }, m[0].length);
      continue;
    }
    if (ch === '[' && !inLabel && (m = LINK.exec(rest))) {
      const href = safeHref(m[2]);
      const children = parseInline(m[1], depth + 1, true);
      if (href) push(linkTo(href, children), m[0].length);
      else {
        flush();
        out.push(...children);
        i += m[0].length;
      }
      continue;
    }
    if (ch === 'h' && !inLabel && !isWordChar(prev) && (m = /^https?:\/\/[^\s<>()[\]{}"']+/i.exec(rest))) {
      const url = trimUrlEnd(m[0]);
      if (safeHref(url)) push(linkTo(url, [{ type: 'text', text: url }]), url.length);
      // A refused candidate (userinfo, say) is text as a whole. Going on from its next character would match the
      // rest of it again from every "http" inside, which is quadratic ('http://a@' repeated).
      else {
        buf += m[0];
        i += m[0].length;
      }
      continue;
    }
    if (ch === '*' || ch === '_') {
      const canOpen = ch === '*' || !isWordChar(prev);
      if (canOpen) {
        const strong = (ch === '*' ? STRONG_STAR : STRONG_UNDER).exec(rest);
        if (strong) {
          push({ type: 'strong', children: parseInline(strong[1], depth + 1, inLabel) }, strong[0].length);
          continue;
        }
        const em = (ch === '*' ? EM_STAR : EM_UNDER).exec(rest);
        if (em) {
          push({ type: 'em', children: parseInline(em[1], depth + 1, inLabel) }, em[0].length);
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

// Fence, heading and rule are recognised with a linear prefix test plus string scans. The obvious single
// regexes (nested \s* around optional groups, a lazy body before a closing run) are cubic on long runs of
// spaces or '#', and a stored reply would freeze the tab on every load.
const HEADING_OPEN = /^\s{0,3}(#{1,6})\s+([\s\S]*)$/;
const INFO = /^[\w+#.-]*$/;
const isSpace = (c: string) => /\s/.test(c);

function parseFenceOpen(line: string): { char: string; length: number; lang: string } | null {
  const t = line.trim();
  const char = t[0];
  if (char !== '`' && char !== '~') return null;
  let n = 1;
  while (t[n] === char) n += 1;
  if (n < 3) return null;
  const lang = t.slice(n).trim();
  return INFO.test(lang) ? { char, length: n, lang } : null;
}

// A closing fence is only the fence character, at least as many as the opener.
function isFenceClose(line: string, char: string, length: number): boolean {
  const t = line.trim();
  if (t.length < length) return false;
  for (let k = 0; k < t.length; k++) if (t[k] !== char) return false;
  return true;
}

function parseHeading(line: string): { level: number; text: string } | null {
  const m = HEADING_OPEN.exec(line);
  if (!m) return null;
  const body = m[2];
  let end = body.length;
  while (end > 0 && isSpace(body[end - 1])) end -= 1;
  let start = end;
  while (start > 0 && body[start - 1] === '#') start -= 1;
  // A closing run only counts after whitespace (so "C#" keeps its #) or when it is the whole content.
  if (start < end && (start === 0 || isSpace(body[start - 1]))) {
    end = start;
    while (end > 0 && isSpace(body[end - 1])) end -= 1;
  }
  return { level: m[1].length, text: body.slice(0, end) };
}

function isHr(line: string): boolean {
  let k = 0;
  while (k < 3 && k < line.length && isSpace(line[k])) k += 1;
  const mark = line[k];
  if (mark !== '-' && mark !== '*' && mark !== '_') return false;
  let count = 0;
  for (; k < line.length; k++) {
    if (line[k] === mark) count += 1;
    else if (!isSpace(line[k])) return false;
  }
  return count >= 3;
}

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
    const long = line.length > MAX_BLOCK_LINE;

    const fence = long ? null : parseFenceOpen(line);
    if (fence) {
      closePara();
      closeLists();
      const body: string[] = [];
      li++;
      while (li < lines.length && !isFenceClose(lines[li], fence.char, fence.length)) {
        body.push(lines[li]);
        li++;
      }
      blocks.push({ type: 'code', lang: fence.lang, text: body.join('\n') });
      afterBlank = false;
      continue;
    }

    if (!line.trim()) {
      closePara();
      afterBlank = true;
      continue;
    }

    const bullet = long ? null : BULLET.exec(line);
    const ordered = long || bullet ? null : ORDERED.exec(line);
    if ((bullet || ordered) && !isHr(line)) {
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
        if (long || !(parseHeading(line) || QUOTE.test(line) || isHr(line))) {
          appendToItem(line, afterBlank);
          afterBlank = false;
          continue;
        }
      }
      closeLists();
    }

    const heading = long ? null : parseHeading(line);
    if (heading) {
      closePara();
      const level = Math.min(heading.level, 3) as 1 | 2 | 3;
      blocks.push({ type: 'heading', level, children: parseInline(heading.text) });
      afterBlank = false;
      continue;
    }

    if (!long && isHr(line)) {
      closePara();
      blocks.push({ type: 'hr' });
      afterBlank = false;
      continue;
    }

    if (!long && QUOTE.test(line) && depth < MAX_QUOTE_DEPTH) {
      closePara();
      const inner: string[] = [];
      while (li < lines.length && lines[li].length <= MAX_BLOCK_LINE && QUOTE.test(lines[li])) {
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
        case 'linkHost':
          return ` (${n.text})`;
        default:
          return inlineText(n.children);
      }
    })
    .join('');
}
