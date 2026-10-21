/**
 * 会话内搜索：在已经拿到手的 records 上做过滤，不额外请求后端。
 *
 * 数据本来就在本地（历史会话与当前会话的 records），所以搜索是纯前端、
 * 输入即出结果；真正的"跨会话全文索引"是另一件事，留到后面再做。
 */

export interface SearchableRecord {
  event_id: string;
  role: string;
  text: string;
}

export interface SearchHit<T> {
  record: T;
  /** 命中的位置（用于高亮；空数组表示按 role 命中或未命中文本） */
  ranges: Array<[number, number]>;
}

function rangesFor(text: string, needle: string): Array<[number, number]> {
  const ranges: Array<[number, number]> = [];
  const haystack = text.toLowerCase();
  const target = needle.toLowerCase();
  let cursor = 0;
  while (cursor <= haystack.length) {
    const found = haystack.indexOf(target, cursor);
    if (found < 0) break;
    ranges.push([found, found + target.length]);
    cursor = found + Math.max(1, target.length);
    if (ranges.length >= 20) break;
  }
  return ranges;
}

/**
 * 过滤记录：文本或角色包含关键字即命中；空查询返回全部（ranges 为空）。
 * 结果保持原始顺序，便于和日志的时间线对应。
 */
export function searchRecords<T extends SearchableRecord>(
  records: readonly T[],
  query: string,
  limit = 200,
): Array<SearchHit<T>> {
  const needle = query.trim();
  if (!needle) return records.slice(0, limit).map((record) => ({ record, ranges: [] }));
  const hits: Array<SearchHit<T>> = [];
  for (const record of records) {
    const ranges = rangesFor(record.text, needle);
    const roleHit = record.role.toLowerCase().includes(needle.toLowerCase());
    if (ranges.length === 0 && !roleHit) continue;
    hits.push({ record, ranges });
    if (hits.length >= limit) break;
  }
  return hits;
}

/** 把一条记录按命中区间切成若干片，交给模板渲染 <mark>。 */
export interface Segment {
  text: string;
  hit: boolean;
}

export function highlightSegments(text: string, ranges: ReadonlyArray<readonly [number, number]>): Segment[] {
  if (ranges.length === 0) return [{ text, hit: false }];
  const segments: Segment[] = [];
  let cursor = 0;
  for (const [start, end] of ranges) {
    if (start > cursor) segments.push({ text: text.slice(cursor, start), hit: false });
    segments.push({ text: text.slice(start, end), hit: true });
    cursor = end;
  }
  if (cursor < text.length) segments.push({ text: text.slice(cursor), hit: false });
  return segments;
}

export function matchLabel(query: string, matched: number, total: number): string {
  if (!query.trim()) return `${total}`;
  return `${matched} / ${total}`;
}
