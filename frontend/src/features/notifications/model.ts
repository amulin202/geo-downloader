export interface MessageText { zh: string; en?: string }
export interface AppMessage {
  id: string; revision: number; title: MessageText; body: MessageText;
  priority: 'normal' | 'important'; publishedAt: string; expiresAt: string | null; readAt: string | null;
  action: { kind: 'update' | 'link'; label: MessageText; url?: string } | null;
}
export interface MessageFeed { schemaVersion: 1; product: 'geod'; accountId: null; checkedAt: string; unreadCount: number; items: AppMessage[] }
const text = (v: unknown): v is MessageText => !!v && typeof v === 'object' && typeof (v as MessageText).zh === 'string' && ((v as MessageText).en === undefined || typeof (v as MessageText).en === 'string');
export function validFeed(value: unknown): value is MessageFeed {
  if (!value || typeof value !== 'object') return false;
  const feed = value as MessageFeed;
  return feed.schemaVersion === 1 && feed.product === 'geod' && feed.accountId === null && Number.isInteger(feed.unreadCount) && feed.unreadCount >= 0 && Array.isArray(feed.items) && feed.items.length <= 100 && feed.items.every(item =>
    item && /^[A-Za-z0-9_-]{1,100}$/.test(item.id) && Number.isInteger(item.revision) && item.revision > 0 && text(item.title) && text(item.body) && ['normal', 'important'].includes(item.priority) && Number.isFinite(Date.parse(item.publishedAt)) && (item.expiresAt === null || Number.isFinite(Date.parse(item.expiresAt))) && (item.readAt === null || Number.isFinite(Date.parse(item.readAt))) && (item.action === null || item.action && ['update', 'link'].includes(item.action.kind) && text(item.action.label)));
}
export function safeMessageUrl(raw: string): string {
  const url = new URL(raw);
  if (url.protocol !== 'https:' || url.username || url.password || raw.length > 2000) throw new Error('Invalid message link');
  return url.href;
}
export const cacheKey = () => 'geod-public-messages-1';
export type GuestReads = Record<string, string>;
export function applyGuestReads(feed: MessageFeed, reads: GuestReads): MessageFeed {
  if (feed.accountId !== null) return feed;
  const items = feed.items.map(item => ({ ...item, readAt: Number.isFinite(Date.parse(reads[`${item.id}:${item.revision}`])) ? reads[`${item.id}:${item.revision}`] : null }));
  return { ...feed, items, unreadCount: items.filter(item => !item.readAt).length };
}
