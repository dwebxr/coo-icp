import { describe, expect, it } from 'vitest';
import { looksLikeUrl, parseBlocks, parseInline, safeHref, type Inline } from './markdown';

describe('parseInline', () => {
  it('parses bold, italic and code', () => {
    expect(parseInline('a **b** *c* `d`')).toEqual([
      { type: 'text', text: 'a ' },
      { type: 'strong', children: [{ type: 'text', text: 'b' }] },
      { type: 'text', text: ' ' },
      { type: 'em', children: [{ type: 'text', text: 'c' }] },
      { type: 'text', text: ' ' },
      { type: 'code', text: 'd' },
    ]);
  });

  it('does not treat arithmetic or snake_case as emphasis', () => {
    expect(parseInline('2 * 3 * 4')).toEqual([{ type: 'text', text: '2 * 3 * 4' }]);
    expect(parseInline('get_conversation_history')).toEqual([{ type: 'text', text: 'get_conversation_history' }]);
  });

  it('keeps markdown inside code spans literal', () => {
    expect(parseInline('`**x**`')).toEqual([{ type: 'code', text: '**x**' }]);
  });

  it('matches code spans by run length', () => {
    expect(parseInline('``a ` b``')).toEqual([{ type: 'code', text: 'a ` b' }]);
    expect(parseInline('`a``')).toEqual([{ type: 'text', text: '`a``' }]);
    expect(parseInline('a ``` b')).toEqual([{ type: 'text', text: 'a ``` b' }]);
  });

  it('only links http(s) URLs', () => {
    expect(parseInline('[ok](https://internetcomputer.org)')).toEqual([
      { type: 'link', href: 'https://internetcomputer.org', children: [{ type: 'text', text: 'ok' }] },
    ]);
    expect(parseInline('[bad](javascript:alert(1))')).toEqual([
      { type: 'text', text: 'bad' },
      { type: 'text', text: ')' },
    ]);
    expect(parseInline('[bad](data:text/html,x)')).toEqual([{ type: 'text', text: 'bad' }]);
  });

  it('autolinks bare URLs and trims trailing punctuation', () => {
    expect(parseInline('See https://example.com/a.')).toEqual([
      { type: 'text', text: 'See ' },
      { type: 'link', href: 'https://example.com/a', children: [{ type: 'text', text: 'https://example.com/a' }] },
      { type: 'text', text: '.' },
    ]);
  });

  it('treats raw HTML as text', () => {
    expect(parseInline('<img src=x onerror=alert(1)>')).toEqual([
      { type: 'text', text: '<img src=x onerror=alert(1)>' },
    ]);
  });

  it('supports backslash escapes', () => {
    expect(parseInline('\\*not em\\*')).toEqual([{ type: 'text', text: '*not em*' }]);
  });

  it('turns newlines into breaks', () => {
    expect(parseInline('a\nb')).toEqual([{ type: 'text', text: 'a' }, { type: 'br' }, { type: 'text', text: 'b' }]);
  });
});

describe('safeHref', () => {
  it('rejects non-http schemes', () => {
    expect(safeHref('javascript:alert(1)')).toBeNull();
    expect(safeHref('JaVaScRiPt:alert(1)')).toBeNull();
    expect(safeHref('//evil.com')).toBeNull();
    expect(safeHref('https://ok.dev')).toBe('https://ok.dev');
  });
});

describe('parseBlocks', () => {
  it('splits paragraphs on blank lines', () => {
    const blocks = parseBlocks('one\ntwo\n\nthree');
    expect(blocks).toHaveLength(2);
    expect(blocks[0]).toEqual({
      type: 'paragraph',
      children: [{ type: 'text', text: 'one' }, { type: 'br' }, { type: 'text', text: 'two' }],
    });
  });

  it('parses ordered lists and keeps numbering across blank lines', () => {
    const blocks = parseBlocks('1. a\n\n2. b\n3. c');
    expect(blocks).toHaveLength(1);
    expect(blocks[0]).toMatchObject({ type: 'list', ordered: true, start: 1 });
    expect((blocks[0] as { items: unknown[] }).items).toHaveLength(3);
  });

  it('respects the start number of an ordered list', () => {
    expect(parseBlocks('3. c\n4. d')[0]).toMatchObject({ type: 'list', start: 3 });
  });

  it('nests indented lists', () => {
    const [list] = parseBlocks('- a\n  - b\n  - c\n- d');
    expect(list).toMatchObject({ type: 'list', ordered: false });
    const items = (list as { items: { sub?: { items: unknown[] } }[] }).items;
    expect(items).toHaveLength(2);
    expect(items[0].sub?.items).toHaveLength(2);
  });

  it('parses fenced code without inline formatting', () => {
    expect(parseBlocks('```rust\nlet **x** = 1;\n```')).toEqual([
      { type: 'code', lang: 'rust', text: 'let **x** = 1;' },
    ]);
  });

  it('tolerates an unclosed fence', () => {
    expect(parseBlocks('```\ncode')).toEqual([{ type: 'code', lang: '', text: 'code' }]);
  });

  it('parses headings, quotes and rules', () => {
    const blocks = parseBlocks('## Title\n> quoted\n\n---\ntext');
    expect(blocks.map((b) => b.type)).toEqual(['heading', 'quote', 'hr', 'paragraph']);
    expect(blocks[0]).toMatchObject({ level: 2 });
  });

  it('ends a list at an unindented paragraph after a blank line', () => {
    expect(parseBlocks('- a\n- b\n\nafter').map((b) => b.type)).toEqual(['list', 'paragraph']);
  });

  it('does not treat a horizontal rule as a list', () => {
    expect(parseBlocks('* * *')).toEqual([{ type: 'hr' }]);
  });
});

describe('images', () => {
  it('renders images as links, never as <img>', () => {
    expect(parseInline('![chart](https://x.dev/a.png)')).toEqual([
      { type: 'link', href: 'https://x.dev/a.png', children: [{ type: 'text', text: 'chart' }] },
    ]);
    expect(parseInline('![x](javascript:alert(1))')).toEqual([{ type: 'text', text: 'x' }, { type: 'text', text: ')' }]);
  });
});

describe('adversarial input', () => {
  const cases: Record<string, string> = {
    'deeply nested quotes': '>'.repeat(19999) + 'x',
    'unmatched stars': '*'.repeat(20000),
    'unmatched strong': '**a '.repeat(5000),
    'unmatched brackets': '['.repeat(20000),
    'unmatched backticks': '`'.repeat(20000),
    'inline backtick run': 'x' + '`'.repeat(19999),
    'many short backtick runs': '`a'.repeat(10000),
    'mixed backtick runs': 'x ' + '``a`'.repeat(5000),
    'nested emphasis': '*_'.repeat(5000) + 'x' + '_*'.repeat(5000),
    'many link openers': '[a](' .repeat(5000),
    'deep list nesting': Array.from({ length: 140 }, (_, i) => `${' '.repeat(i * 2)}- item`).join('\n'),
  };
  for (const [name, input] of Object.entries(cases)) {
    it(`handles ${name} quickly without throwing`, () => {
      const start = performance.now();
      expect(() => parseBlocks(input)).not.toThrow();
      expect(performance.now() - start).toBeLessThan(500);
    });
  }

  it('caps quote nesting and keeps the rest as text', () => {
    const blocks = parseBlocks('>>>>>>deep');
    let depth = 0;
    let node = blocks[0];
    while (node && node.type === 'quote') {
      depth += 1;
      node = node.children[0];
    }
    expect(depth).toBe(4);
    expect(node).toMatchObject({ type: 'paragraph' });
  });
});

describe('regex denial of service', () => {
  const cases: Record<string, string> = {
    'heading, spaces inside': '# a' + ' '.repeat(19996) + 'b',
    'heading, run of #': '# ' + '#'.repeat(19990) + 'x',
    'fence opener, trailing spaces': '```' + ' '.repeat(19990) + '!',
    'bullet, spaces': '-' + ' '.repeat(20000) + 'x',
    'quote, spaces': '>' + ' '.repeat(20000) + 'x',
    'ordered, spaces': '1.' + ' '.repeat(20000) + 'x',
    'rule, spaces': '- - -' + ' '.repeat(20000) + 'x',
    'one line of "# "': '# '.repeat(10000),
    'only spaces': ' '.repeat(20000),
    'many headings with inner spaces': Array(400).fill('# a' + ' '.repeat(40) + 'b').join('\n'),
    'autolink ending in dots': 'http://' + '.'.repeat(19990) + 'a',
    'unclosed fence, many lines': '```\n' + 'x\n'.repeat(9990),
  };
  for (const [name, input] of Object.entries(cases)) {
    it(`parses ${name} in under 50 ms`, () => {
      parseBlocks(input); // warm up
      const start = performance.now();
      parseBlocks(input);
      expect(performance.now() - start).toBeLessThan(50);
    });
  }

  it('treats a line over 2000 characters as paragraph text', () => {
    const long = (prefix: string) => prefix + 'a'.repeat(2000);
    for (const prefix of ['# ', '- ', '1. ', '> ', '---', '```']) {
      expect(parseBlocks(long(prefix)).map((b) => b.type)).toEqual(['paragraph']);
    }
    expect(parseBlocks('# ' + 'a'.repeat(1990)).map((b) => b.type)).toEqual(['heading']);
  });

  it('keeps a long line after a list item inside the item', () => {
    expect(parseBlocks('- a\n# ' + 'b'.repeat(2000)).map((b) => b.type)).toEqual(['list']);
  });
});

describe('headings', () => {
  const text = (src: string) => {
    const [b] = parseBlocks(src);
    return b.type === 'heading' ? { level: b.level, text: b.children.map((c) => (c.type === 'text' ? c.text : '?')).join('') } : b.type;
  };

  it('parses levels and caps them at 3', () => {
    expect(text('# Title')).toEqual({ level: 1, text: 'Title' });
    expect(text('#### Deep')).toEqual({ level: 3, text: 'Deep' });
    expect(text('####### seven')).toBe('paragraph');
    expect(text('#nospace')).toBe('paragraph');
  });

  it('removes a closing run of hashes', () => {
    expect(text('## Title ##')).toEqual({ level: 2, text: 'Title' });
    expect(text('# Title   ###   ')).toEqual({ level: 1, text: 'Title' });
  });

  it('keeps hashes that are part of the text', () => {
    expect(text('# C#')).toEqual({ level: 1, text: 'C#' });
    expect(text('# a ## b')).toEqual({ level: 1, text: 'a ## b' });
  });

  it('gives an empty heading when only hashes follow', () => {
    expect(text('# ##')).toEqual({ level: 1, text: '' });
  });
});

describe('fences', () => {
  it('reads the language and ignores indentation and trailing spaces', () => {
    expect(parseBlocks('  ```c++  \nx\n```')).toEqual([{ type: 'code', lang: 'c++', text: 'x' }]);
  });

  it('supports tilde fences and does not mix fence characters', () => {
    expect(parseBlocks('~~~py\n```\nx\n~~~')).toEqual([{ type: 'code', lang: 'py', text: '```\nx' }]);
  });

  it('closes only on a run at least as long as the opener', () => {
    expect(parseBlocks('````\na\n```\nb\n`````\nafter')).toEqual([
      { type: 'code', lang: '', text: 'a\n```\nb' },
      { type: 'paragraph', children: [{ type: 'text', text: 'after' }] },
    ]);
  });

  it('does not close on a line with other text', () => {
    expect(parseBlocks('```\na\n``` x\nb')).toEqual([{ type: 'code', lang: '', text: 'a\n``` x\nb' }]);
  });

  it('is not a fence when the info string has spaces or two backticks open', () => {
    expect(parseBlocks('``` a b')[0].type).toBe('paragraph');
    expect(parseBlocks('``x``')[0].type).toBe('paragraph');
  });
});

describe('horizontal rules', () => {
  it('accepts spaced and unspaced rules', () => {
    for (const src of ['---', '***', '___', '- - -', '  * * * *  ']) expect(parseBlocks(src)).toEqual([{ type: 'hr' }]);
  });

  it('rejects mixed marks, short runs and deep indentation', () => {
    for (const src of ['--', '-*-', '--- a']) expect(parseBlocks(src)[0].type).not.toBe('hr');
    expect(parseBlocks('    ---')[0].type).not.toBe('hr');
  });
});

describe('link safety', () => {
  const link = (src: string) => parseInline(src).find((n) => n.type === 'link');
  const labelOf = (src: string) => {
    const l = link(src);
    return l && l.type === 'link' ? l.children : null;
  };

  it('rejects URLs with userinfo', () => {
    expect(safeHref('https://good.example@evil.example/')).toBeNull();
    expect(safeHref('https://user:pass@evil.example/')).toBeNull();
    expect(safeHref('https://good.example/@x')).toBe('https://good.example/@x');
    expect(link('[a](https://good.example@evil.example/)')).toBeUndefined();
    expect(link('go https://good.example@evil.example/ now')).toBeUndefined();
  });

  it('still refuses script-like schemes in any spelling', () => {
    for (const url of [
      'javascript:alert(1)',
      'JAVASCRIPT:alert(1)',
      ' javascript:alert(1)',
      'java\tscript:alert(1)',
      'data:text/html,x',
      'DaTa:text/html,x',
      'vbscript:msgbox(1)',
      'VBScript:x',
      '\nvbscript:x',
    ]) {
      expect(safeHref(url)).toBeNull();
    }
    expect(link('[x](vbscript:msgbox(1))')).toBeUndefined();
  });

  it('recognises labels that look like a URL or host', () => {
    for (const label of ['https://identity.internetcomputer.org', 'paypal.com', 'www.bank.co.uk/login', 'example.com:8080/x', 'http://a.io']) {
      expect(looksLikeUrl(label)).toBe(true);
    }
    for (const label of ['click here', 'docs', 'v1.2', 'e.g.', 'see https://a.example', 'a.b', '']) {
      expect(looksLikeUrl(label)).toBe(false);
    }
  });

  it('shows the real target when a URL-like label names another host', () => {
    expect(labelOf('[https://identity.internetcomputer.org](https://evil.example/login)')).toEqual([
      { type: 'text', text: 'https://evil.example/login' },
    ]);
    expect(labelOf('[paypal.com](https://evil.example)')).toEqual([{ type: 'text', text: 'https://evil.example' }]);
    expect(labelOf('[**paypal.com**](https://evil.example)')).toEqual([{ type: 'text', text: 'https://evil.example' }]);
  });

  it('keeps ordinary labels and labels on the same host', () => {
    expect(labelOf('[the docs](https://evil.example)')).toEqual([{ type: 'text', text: 'the docs' }]);
    expect(labelOf('[internetcomputer.org](https://internetcomputer.org/docs)')).toEqual([{ type: 'text', text: 'internetcomputer.org' }]);
    expect(labelOf('[www.example.com](https://example.com)')).toEqual([{ type: 'text', text: 'www.example.com' }]);
  });

  it('never nests a link in a link', () => {
    expect(parseInline('[see https://a.example](https://b.example)')).toEqual([
      { type: 'link', href: 'https://b.example', children: [{ type: 'text', text: 'see https://a.example' }] },
    ]);
    // The label ends at the first "]", so this splits into sibling links instead of nesting.
    const hasNested = (nodes: Inline[], inLink = false): boolean =>
      nodes.some((n) => (n.type === 'link' && inLink) || ('children' in n && hasNested(n.children, inLink || n.type === 'link')));
    for (const src of ['[a [b](https://c.example)](https://d.example)', '[**https://a.example**](https://b.example)', '[_[x](https://c.example)_](https://d.example)']) {
      expect(hasNested(parseInline(src))).toBe(false);
    }
    expect(hasNested(parseInline('[![x](https://i.example/a.png)](https://d.example)'))).toBe(false);
  });
});
