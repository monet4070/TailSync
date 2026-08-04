export const HISTORY_PAGE_SIZE = 30;

export function historyPageCount(total: number, pageSize = HISTORY_PAGE_SIZE): number {
  if (!Number.isFinite(total) || total <= 0) return 1;
  if (!Number.isInteger(pageSize) || pageSize <= 0) {
    throw new Error("pageSize must be a positive integer");
  }
  return Math.max(1, Math.ceil(total / pageSize));
}

export function normalizeHistoryPage(
  requestedPage: number,
  total: number,
  pageSize = HISTORY_PAGE_SIZE,
): number {
  const lastPage = historyPageCount(total, pageSize) - 1;
  if (!Number.isFinite(requestedPage)) return 0;
  return Math.min(Math.max(0, Math.trunc(requestedPage)), lastPage);
}
