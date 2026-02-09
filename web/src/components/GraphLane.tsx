import type { GraphRow, PadLine } from '../types';

// LinkLine bit constants from sapling-renderdag
const VERT_PARENT     = 0x004;
const VERT_ANCESTOR   = 0x008;
const LEFT_FORK_PARENT  = 0x010;
const LEFT_FORK_ANCESTOR = 0x020;
const RIGHT_FORK_PARENT  = 0x040;
const RIGHT_FORK_ANCESTOR = 0x080;
const LEFT_MERGE_PARENT  = 0x100;
const LEFT_MERGE_ANCESTOR = 0x200;
const RIGHT_MERGE_PARENT = 0x400;
const RIGHT_MERGE_ANCESTOR = 0x800;

export const COL_WIDTH = 18;
const NODE_R = 5;
const STROKE_W = 2;
const PARENT_COLOR = '#94a3b8';
const ANCESTOR_COLOR = '#cbd5e1';

// --- Pure rendering logic (testable) ---

export interface SvgLine {
  type: 'line';
  x1: number; y1: string;
  x2: number; y2: string;
  stroke: string;
  dashed: boolean;
}

export interface SvgCircle {
  type: 'circle';
  cx: number; cy: string;
  r: number;
  fill: string;
}

export type SvgElement = SvgLine | SvgCircle;

function cx(col: number): number {
  return col * COL_WIDTH + COL_WIDTH / 2;
}

/**
 * Compute SVG elements for a graph row.
 * Y-coordinates are percentages (e.g. "0%", "50%", "100%") so the SVG
 * stretches to any container height.
 *
 * prevPadLines: the pad_lines from the previous row, used to determine
 * whether a Node column has a connection arriving from above.
 */
export function computeGraphElements(row: GraphRow, prevPadLines?: PadLine[]): SvgElement[] {
  const elements: SvgElement[] = [];
  const nodeColor = PARENT_COLOR;

  // --- Node line: vertical lines + node circle ---
  for (let i = 0; i < row.node_line.length; i++) {
    const col = row.node_line[i];
    const x = cx(i);

    if (col === 'Parent' || col === 'Ancestor') {
      const stroke = col === 'Parent' ? PARENT_COLOR : ANCESTOR_COLOR;
      const dashed = col === 'Ancestor';
      // If the lane doesn't continue below (merges away via diagonal), stop at 60%
      const belowPad = row.pad_lines[i];
      const continues = belowPad === 'Parent' || belowPad === 'Ancestor';
      const y2 = continues ? '100%' : '60%';
      elements.push({ type: 'line', x1: x, y1: '0%', x2: x, y2, stroke, dashed });
    } else if (col === 'Node') {
      // Draw top half (0%→50%) only if the previous row connects into this column
      const abovePad = prevPadLines?.[i];
      const hasAbove = abovePad === 'Parent' || abovePad === 'Ancestor';
      // Bottom: pad_lines means vertical continues to next row (50%→100%)
      // link_line bits means diagonal starts at 60% (only need 50%→60% stub)
      const belowPad = row.pad_lines[i];
      const hasBelowPad = belowPad === 'Parent' || belowPad === 'Ancestor';
      const hasBelowDiag = !hasBelowPad
        && row.link_line != null && (row.link_line[i] ?? 0) !== 0;

      const topY = '0%';
      const bottomY = hasBelowPad ? '100%' : hasBelowDiag ? '60%' : '50%';

      if (hasAbove) {
        elements.push({ type: 'line', x1: x, y1: topY, x2: x, y2: bottomY, stroke: nodeColor, dashed: false });
      } else if (hasBelowPad || hasBelowDiag) {
        elements.push({ type: 'line', x1: x, y1: '50%', x2: x, y2: bottomY, stroke: nodeColor, dashed: false });
      }
      // If neither above nor below, just the circle (no vertical line)
    }
    // 'Blank' → nothing
  }

  // --- Link line: fork/merge diagonals ---
  // These occupy the bottom portion of the row, connecting to the next row.
  // MERGE at column i: "my lane merges toward column i±1" → diagonal from (cx(i), 60%) to (cx(i±1), 100%)
  // FORK at column i: "a child lane forks from column i±1 toward me" → diagonal from (cx(i±1), 60%) to (cx(i), 100%)
  // Both describe the same edge from different perspectives, so we only draw MERGE to avoid duplicates.
  if (row.link_line) {
    for (let i = 0; i < row.link_line.length; i++) {
      const bits = row.link_line[i];
      if (bits === 0) continue;
      const x = cx(i);

      // Vertical continuation through link section (only if no node_line vertical already covers it)
      if (((bits & VERT_PARENT) || (bits & VERT_ANCESTOR)) && row.node_line[i] === 'Blank') {
        const isAncestor = !(bits & VERT_PARENT);
        const stroke = isAncestor ? ANCESTOR_COLOR : PARENT_COLOR;
        const abovePad = prevPadLines?.[i];
        const hasAbove = abovePad === 'Parent' || abovePad === 'Ancestor';
        const belowPad = row.pad_lines[i];
        const hasBelow = belowPad === 'Parent' || belowPad === 'Ancestor';
        // Only draw the portions that actually connect to something
        const y1 = hasAbove ? '0%' : '60%';
        const y2 = hasBelow ? '100%' : '60%';
        if (hasAbove || hasBelow) {
          elements.push({ type: 'line', x1: x, y1, x2: x, y2, stroke, dashed: isAncestor });
        }
        // If neither above nor below, diagonals handle the connection
      }

      // Merge diagonals: from this column toward an adjacent column
      if (bits & LEFT_MERGE_PARENT) {
        elements.push({ type: 'line', x1: x, y1: '60%', x2: cx(i - 1), y2: '100%', stroke: PARENT_COLOR, dashed: false });
      }
      if (bits & LEFT_MERGE_ANCESTOR) {
        elements.push({ type: 'line', x1: x, y1: '60%', x2: cx(i - 1), y2: '100%', stroke: ANCESTOR_COLOR, dashed: true });
      }
      if (bits & RIGHT_MERGE_PARENT) {
        elements.push({ type: 'line', x1: x, y1: '60%', x2: cx(i + 1), y2: '100%', stroke: PARENT_COLOR, dashed: false });
      }
      if (bits & RIGHT_MERGE_ANCESTOR) {
        elements.push({ type: 'line', x1: x, y1: '60%', x2: cx(i + 1), y2: '100%', stroke: ANCESTOR_COLOR, dashed: true });
      }

      // Fork diagonals: only draw if there's no corresponding MERGE on the adjacent column
      // (to avoid drawing the same edge twice)
      if (bits & RIGHT_FORK_PARENT) {
        const adj = row.link_line[i + 1] ?? 0;
        if (!(adj & LEFT_MERGE_PARENT)) {
          elements.push({ type: 'line', x1: cx(i + 1), y1: '60%', x2: x, y2: '100%', stroke: PARENT_COLOR, dashed: false });
        }
      }
      if (bits & RIGHT_FORK_ANCESTOR) {
        const adj = row.link_line[i + 1] ?? 0;
        if (!(adj & LEFT_MERGE_ANCESTOR)) {
          elements.push({ type: 'line', x1: cx(i + 1), y1: '60%', x2: x, y2: '100%', stroke: ANCESTOR_COLOR, dashed: true });
        }
      }
      if (bits & LEFT_FORK_PARENT) {
        const adj = row.link_line[i - 1] ?? 0;
        if (!(adj & RIGHT_MERGE_PARENT)) {
          elements.push({ type: 'line', x1: cx(i - 1), y1: '60%', x2: x, y2: '100%', stroke: PARENT_COLOR, dashed: false });
        }
      }
      if (bits & LEFT_FORK_ANCESTOR) {
        const adj = row.link_line[i - 1] ?? 0;
        if (!(adj & RIGHT_MERGE_ANCESTOR)) {
          elements.push({ type: 'line', x1: cx(i - 1), y1: '60%', x2: x, y2: '100%', stroke: ANCESTOR_COLOR, dashed: true });
        }
      }
    }
  }

  // --- Term line (dashed terminator) ---
  if (row.term_line) {
    for (let i = 0; i < row.term_line.length; i++) {
      if (row.term_line[i]) {
        elements.push({ type: 'line', x1: cx(i), y1: '85%', x2: cx(i), y2: '100%', stroke: PARENT_COLOR, dashed: true });
      }
    }
  }

  // --- Node circle (drawn last = on top) ---
  for (let i = 0; i < row.node_line.length; i++) {
    if (row.node_line[i] === 'Node') {
      elements.push({ type: 'circle', cx: cx(i), cy: '50%', r: NODE_R, fill: nodeColor });
    }
  }

  return elements;
}

// --- React component ---

interface GraphLaneProps {
  row: GraphRow;
  prevPadLines?: PadLine[];
}

export function GraphLane({ row, prevPadLines }: GraphLaneProps) {
  const cols = row.node_line.length;
  if (cols === 0) return null;

  const width = cols * COL_WIDTH;
  const elements = computeGraphElements(row, prevPadLines);

  return (
    <svg width={width} overflow="visible" style={{ height: '100%', display: 'block' }}>
      {elements.map((el, i) => {
        if (el.type === 'line') {
          return (
            <line key={i} x1={el.x1} y1={el.y1} x2={el.x2} y2={el.y2}
              stroke={el.stroke} strokeWidth={STROKE_W}
              strokeDasharray={el.dashed ? '3 3' : undefined} />
          );
        }
        return (
          <circle key={i} cx={el.cx} cy={el.cy} r={el.r} fill={el.fill} />
        );
      })}
    </svg>
  );
}
