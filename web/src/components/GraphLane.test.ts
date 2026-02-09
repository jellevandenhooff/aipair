import { describe, it, expect } from 'vitest';
import { computeGraphElements, COL_WIDTH, type SvgElement, type SvgLine, type SvgCircle } from './GraphLane';
import type { GraphRow, PadLine } from '../types';

const cx = (col: number) => col * COL_WIDTH + COL_WIDTH / 2;

function lines(els: SvgElement[]): SvgLine[] {
  return els.filter((e): e is SvgLine => e.type === 'line');
}

function circles(els: SvgElement[]): SvgCircle[] {
  return els.filter((e): e is SvgCircle => e.type === 'circle');
}

function makeRow(overrides: Partial<GraphRow>): GraphRow {
  return {
    node: 'test-node',
    glyph: '@',
    merge: false,
    node_line: ['Node'],
    link_line: null,
    term_line: null,
    pad_lines: [],
    ...overrides,
  };
}

describe('computeGraphElements', () => {
  describe('node in linear chain (connected above and below)', () => {
    it('draws a full vertical line and a circle', () => {
      const row = makeRow({ node_line: ['Node'], pad_lines: ['Parent'] });
      const prevPad: PadLine[] = ['Parent'];
      const els = computeGraphElements(row, prevPad);

      const cs = circles(els);
      expect(cs).toHaveLength(1);
      expect(cs[0].cx).toBe(cx(0));
      expect(cs[0].cy).toBe('50%');

      // Vertical line from top to bottom
      const vert = lines(els).find(l => l.x1 === cx(0) && l.y1 === '0%' && l.y2 === '100%');
      expect(vert).toBeDefined();
    });
  });

  describe('orphan node (no connection above)', () => {
    it('only draws bottom half of vertical line from node center down', () => {
      // e.g. a forked branch: node appears at column 1, nothing above in that column
      const LEFT_MERGE_PARENT = 0x100;
      const VERT_PARENT = 0x004;
      const row = makeRow({
        node_line: ['Parent', 'Node'],
        link_line: [VERT_PARENT, LEFT_MERGE_PARENT],
        pad_lines: ['Parent', 'Blank'],
      });
      // Previous row only had 1 column, so no pad at column 1
      const prevPad: PadLine[] = ['Parent'];
      const els = computeGraphElements(row, prevPad);

      // Column 1 node: should NOT have a line from 0% to 50% (nothing above)
      const col1Lines = lines(els).filter(l => l.x1 === cx(1) && l.x2 === cx(1));
      const topHalf = col1Lines.find(l => l.y1 === '0%');
      expect(topHalf).toBeUndefined();

      // Should have stub: 50% to 60% (connects node center to diagonal start)
      const bottomHalf = col1Lines.find(l => l.y1 === '50%' && l.y2 === '60%');
      expect(bottomHalf).toBeDefined();

      // Should still have the circle
      const col1Circles = circles(els).filter(c => c.cx === cx(1));
      expect(col1Circles).toHaveLength(1);
    });
  });

  describe('isolated node (no connections above or below)', () => {
    it('draws only a circle, no vertical lines', () => {
      const row = makeRow({ node_line: ['Node'], pad_lines: ['Blank'] });
      const els = computeGraphElements(row);

      // No vertical lines at node column
      const col0Verts = lines(els).filter(l => l.x1 === cx(0) && l.x2 === cx(0));
      expect(col0Verts).toHaveLength(0);

      // Circle only
      expect(circles(els)).toHaveLength(1);
    });
  });

  describe('parent pass-through', () => {
    it('draws a solid vertical line spanning full height when lane continues below', () => {
      const row = makeRow({ node_line: ['Parent'], pad_lines: ['Parent'] });
      const els = computeGraphElements(row);

      const ls = lines(els);
      expect(ls).toHaveLength(1);
      expect(ls[0]).toMatchObject({
        x1: cx(0), y1: '0%', x2: cx(0), y2: '100%', dashed: false,
      });
      expect(circles(els)).toHaveLength(0);
    });

    it('stops at 60% when lane merges away (pad_lines Blank)', () => {
      const LEFT_MERGE_PARENT = 0x100;
      const row = makeRow({
        node_line: ['Node', 'Parent'],
        link_line: [0, LEFT_MERGE_PARENT],
        pad_lines: ['Parent', 'Blank'],
      });
      const prevPad: PadLine[] = ['Parent', 'Parent'];
      const els = computeGraphElements(row, prevPad);

      // Column 1 Parent line should stop at 60%, not 100%
      const col1Verts = lines(els).filter(l => l.x1 === cx(1) && l.x2 === cx(1));
      const fullVert = col1Verts.find(l => l.y2 === '100%');
      expect(fullVert).toBeUndefined();
      const partialVert = col1Verts.find(l => l.y1 === '0%' && l.y2 === '60%');
      expect(partialVert).toBeDefined();
    });
  });

  describe('ancestor pass-through', () => {
    it('draws a dashed vertical line', () => {
      const row = makeRow({ node_line: ['Ancestor'] });
      const els = computeGraphElements(row);

      const ls = lines(els);
      expect(ls).toHaveLength(1);
      expect(ls[0].dashed).toBe(true);
    });
  });

  describe('blank column', () => {
    it('draws nothing', () => {
      const row = makeRow({ node_line: ['Blank'] });
      const els = computeGraphElements(row);

      expect(lines(els)).toHaveLength(0);
      expect(circles(els)).toHaveLength(0);
    });
  });

  describe('two columns: node + parent', () => {
    it('draws elements in both columns', () => {
      const row = makeRow({ node_line: ['Node', 'Parent'], pad_lines: ['Parent', 'Parent'] });
      const prevPad: PadLine[] = ['Parent', 'Parent'];
      const els = computeGraphElements(row, prevPad);

      // Column 0: circle
      expect(circles(els)).toHaveLength(1);
      expect(circles(els)[0].cx).toBe(cx(0));

      // Column 0: full vertical line (connected above and below)
      const col0Vert = lines(els).find(l => l.x1 === cx(0) && l.y1 === '0%' && l.y2 === '100%');
      expect(col0Vert).toBeDefined();

      // Column 1: vertical line
      const col1Lines = lines(els).filter(l => l.x1 === cx(1));
      expect(col1Lines.length).toBeGreaterThanOrEqual(1);
      expect(col1Lines[0]).toMatchObject({ y1: '0%', y2: '100%' });
    });
  });

  describe('LEFT_MERGE diagonal', () => {
    it('draws a diagonal from this column toward column to the left', () => {
      const LEFT_MERGE_PARENT = 0x100;
      const VERT_PARENT = 0x004;
      const row = makeRow({
        node_line: ['Parent', 'Node'],
        link_line: [VERT_PARENT, LEFT_MERGE_PARENT],
      });
      const els = computeGraphElements(row);

      // Should have a diagonal from column 1 to column 0
      const diag = lines(els).find(l =>
        l.x1 === cx(1) && l.y1 === '60%' && l.x2 === cx(0) && l.y2 === '100%'
      );
      expect(diag).toBeDefined();
      expect(diag!.dashed).toBe(false);
    });
  });

  describe('RIGHT_MERGE diagonal', () => {
    it('draws a diagonal from this column toward column to the right', () => {
      const RIGHT_MERGE_PARENT = 0x400;
      const VERT_PARENT = 0x004;
      const row = makeRow({
        node_line: ['Node', 'Parent'],
        link_line: [RIGHT_MERGE_PARENT, VERT_PARENT],
      });
      const els = computeGraphElements(row);

      const diag = lines(els).find(l =>
        l.x1 === cx(0) && l.y1 === '60%' && l.x2 === cx(1) && l.y2 === '100%'
      );
      expect(diag).toBeDefined();
    });
  });

  describe('RIGHT_FORK with adjacent LEFT_MERGE (same edge)', () => {
    it('draws the diagonal only once', () => {
      const RIGHT_FORK_PARENT = 0x040;
      const LEFT_MERGE_PARENT = 0x100;
      const VERT_PARENT = 0x004;
      const row = makeRow({
        node_line: ['Parent', 'Node'],
        link_line: [VERT_PARENT | RIGHT_FORK_PARENT, LEFT_MERGE_PARENT],
      });
      const els = computeGraphElements(row);

      // Count diagonal lines (lines where x1 !== x2)
      const diags = lines(els).filter(l => l.x1 !== l.x2);
      expect(diags).toHaveLength(1);
      // The diagonal should go from column 1 toward column 0
      expect(diags[0]).toMatchObject({
        x1: cx(1), y1: '60%', x2: cx(0), y2: '100%',
      });
    });
  });

  describe('RIGHT_FORK without adjacent MERGE', () => {
    it('draws the fork diagonal', () => {
      const RIGHT_FORK_PARENT = 0x040;
      const VERT_PARENT = 0x004;
      const row = makeRow({
        node_line: ['Parent', 'Blank'],
        link_line: [VERT_PARENT | RIGHT_FORK_PARENT, 0],
      });
      const els = computeGraphElements(row);

      // Fork should draw diagonal from column 1 to column 0
      const diags = lines(els).filter(l => l.x1 !== l.x2);
      expect(diags).toHaveLength(1);
      expect(diags[0]).toMatchObject({
        x1: cx(1), y1: '60%', x2: cx(0), y2: '100%',
      });
    });
  });

  describe('VERT_PARENT at new blank column (fork creates new lane)', () => {
    it('starts vertical at 60% not 0% when nothing connects from above', () => {
      const VERT_PARENT = 0x004;
      const RIGHT_FORK_PARENT = 0x040;
      // Fork row: node at col 0, new lane at col 1 via fork
      const row = makeRow({
        node_line: ['Node', 'Blank'],
        link_line: [VERT_PARENT | RIGHT_FORK_PARENT, VERT_PARENT],
        pad_lines: ['Parent', 'Parent'],
      });
      // Previous row only had 1 column
      const prevPad: PadLine[] = ['Parent'];
      const els = computeGraphElements(row, prevPad);

      // Column 1 vertical should start at 60%, not 0%
      const col1Verts = lines(els).filter(l => l.x1 === cx(1) && l.x2 === cx(1));
      const fullVert = col1Verts.find(l => l.y1 === '0%');
      expect(fullVert).toBeUndefined();
      const partialVert = col1Verts.find(l => l.y1 === '60%' && l.y2 === '100%');
      expect(partialVert).toBeDefined();
    });

    it('does not draw vertical when nothing connects above or below', () => {
      const VERT_PARENT = 0x004;
      const RIGHT_FORK_PARENT = 0x040;
      const row = makeRow({
        node_line: ['Node', 'Blank'],
        link_line: [VERT_PARENT | RIGHT_FORK_PARENT, VERT_PARENT],
        pad_lines: ['Parent', 'Blank'],
      });
      const prevPad: PadLine[] = ['Parent'];
      const els = computeGraphElements(row, prevPad);

      // Column 1: nothing above, nothing below → no vertical at all
      const col1Verts = lines(els).filter(l => l.x1 === cx(1) && l.x2 === cx(1));
      expect(col1Verts).toHaveLength(0);
    });

    it('draws full vertical when previous row has active lane above', () => {
      const VERT_PARENT = 0x004;
      const row = makeRow({
        node_line: ['Node', 'Blank'],
        link_line: [VERT_PARENT, VERT_PARENT],
        pad_lines: ['Parent', 'Parent'],
      });
      const prevPad: PadLine[] = ['Parent', 'Parent'];
      const els = computeGraphElements(row, prevPad);

      // Column 1 should have full 0-100% vertical
      const col1Verts = lines(els).filter(l => l.x1 === cx(1) && l.x2 === cx(1));
      const fullVert = col1Verts.find(l => l.y1 === '0%' && l.y2 === '100%');
      expect(fullVert).toBeDefined();
    });
  });

  describe('term line', () => {
    it('draws a dashed line segment at the bottom', () => {
      const row = makeRow({
        node_line: ['Node'],
        term_line: [true],
      });
      const els = computeGraphElements(row);

      const termLine = lines(els).find(l => l.y1 === '85%' && l.y2 === '100%' && l.dashed);
      expect(termLine).toBeDefined();
    });
  });

  describe('node circle is last (drawn on top)', () => {
    it('circle appears after all lines in the elements array', () => {
      const row = makeRow({ node_line: ['Node'], pad_lines: ['Parent'] });
      const prevPad: PadLine[] = ['Parent'];
      const els = computeGraphElements(row, prevPad);

      const lastEl = els[els.length - 1];
      expect(lastEl.type).toBe('circle');
    });
  });
});
