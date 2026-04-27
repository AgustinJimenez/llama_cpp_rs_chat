const TIMESTAMP_PARTS_COUNT = 6;
const DAYS_PER_WEEK = 7;
const DAYS_PER_MONTH = 30;
const MONTHS_PER_YEAR = 12;
const DAYS_PER_YEAR = 365;
const MS_PER_MINUTE = 60000;

export const DATE_GROUPS = [
  'Today',
  'Yesterday',
  'Previous 7 Days',
  'Previous 30 Days',
  'Older',
] as const;

export function relativeTime(timestamp: string): string {
  const parts = timestamp.split('-');
  if (parts.length < TIMESTAMP_PARTS_COUNT) return timestamp;
  const [year, month, day, hour, minute, second] = parts;
  const date = new Date(
    Date.UTC(
      Number(year),
      Number(month) - 1,
      Number(day),
      Number(hour),
      Number(minute),
      Number(second),
    ),
  );
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMin = Math.floor(diffMs / MS_PER_MINUTE);
  if (diffMin < 1) return 'now';
  if (diffMin < 60) return `${diffMin}m`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h`;
  const diffDay = Math.floor(diffHr / 24);
  if (diffDay < DAYS_PER_WEEK) return `${diffDay}d`;
  const diffWeek = Math.floor(diffDay / DAYS_PER_WEEK);
  if (diffWeek < 5) return `${diffWeek}w`;
  const diffMonth = Math.floor(diffDay / DAYS_PER_MONTH);
  if (diffMonth < MONTHS_PER_YEAR) return `${diffMonth}mo`;
  return `${Math.floor(diffDay / DAYS_PER_YEAR)}y`;
}

export function getDateGroup(timestamp: string): string {
  const parts = timestamp.split('-');
  if (parts.length < 3) return 'Older';
  const date = new Date(
    Date.UTC(
      Number(parts[0]),
      Number(parts[1]) - 1,
      Number(parts[2]),
      parts.length >= 4 ? Number(parts[3]) : 0,
      parts.length >= 5 ? Number(parts[4]) : 0,
      parts.length >= TIMESTAMP_PARTS_COUNT ? Number(parts[5]) : 0,
    ),
  );
  const now = new Date();
  const today = new Date(Date.UTC(now.getUTCFullYear(), now.getUTCMonth(), now.getUTCDate()));
  const yesterday = new Date(today);
  yesterday.setDate(yesterday.getDate() - 1);
  const weekAgo = new Date(today);
  weekAgo.setDate(weekAgo.getDate() - DAYS_PER_WEEK);
  const monthAgo = new Date(today);
  monthAgo.setDate(monthAgo.getDate() - DAYS_PER_MONTH);

  if (date >= today) return 'Today';
  if (date >= yesterday) return 'Yesterday';
  if (date >= weekAgo) return 'Previous 7 Days';
  if (date >= monthAgo) return 'Previous 30 Days';
  return 'Older';
}
