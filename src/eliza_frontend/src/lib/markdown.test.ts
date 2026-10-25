import { describe, expect, it } from 'vitest';
import { parseBlocks, parseInline, safeHref } from './markdown';

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
