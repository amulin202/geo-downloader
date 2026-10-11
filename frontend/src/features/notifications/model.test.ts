import assert from 'node:assert/strict';
import test from 'node:test';
import { applyGuestReads, cacheKey, safeMessageUrl, validFeed } from './model.ts';
const feed = () => ({ schemaVersion: 1, product: 'geod', accountId: null, checkedAt: new Date().toISOString(), unreadCount: 1, items: [{ id: 'notice', revision: 1, title: { zh: '消息', en: 'Notice' }, body: { zh: '内容' }, priority: 'important', publishedAt: new Date().toISOString(), expiresAt: null, readAt: null, action: null }] });
test('legacy inbox rejects wrong accounts, products, invalid revisions and corrupt caches', () => {
  assert.equal(validFeed(feed()), true);
  assert.equal(validFeed({ ...feed(), accountId: "account-b" }), false);
  assert.equal(validFeed({ ...feed(), product: 'geod-agent' }), false);
  assert.equal(validFeed({ ...feed(), items: [{ ...feed().items[0], revision: 0 }] }), false);
  assert.equal(validFeed({ ...feed(), items: [{ ...feed().items[0], publishedAt: 'invalid' }] }), false);
  assert.equal(validFeed({ ...feed(), items: Array(101).fill(feed().items[0]) }), false);
  assert.equal(validFeed(null), false);
  assert.equal(cacheKey(), 'geod-public-messages-1');
});
test('notification actions only open HTTPS links without credentials', () => {
  assert.equal(safeMessageUrl('https://geod.laogao.xyz/'), 'https://geod.laogao.xyz/');
  for (const url of ['javascript:alert(1)', 'file:///C:/test', 'http://example.test/', 'https://u:p@example.test', 'https://example.test/' + 'a'.repeat(2000)]) assert.throws(() => safeMessageUrl(url));
});
test('public notice receipts persist per revision', () => {
  const publicFeed = { ...feed(), accountId: null };
  assert.equal(validFeed(publicFeed), true);
  assert.equal(validFeed({ ...feed(), accountId: "account-a" }), false);
  assert.equal(cacheKey(), 'geod-public-messages-1');
  const date = '2026-10-11T01:00:00Z';
  const read = applyGuestReads(publicFeed as Parameters<typeof applyGuestReads>[0], { 'notice:1': date });
  assert.equal(read.unreadCount, 0); assert.equal(read.items[0].readAt, date);
  const revised = { ...publicFeed, items: [{ ...publicFeed.items[0], revision: 2 }] };
  assert.equal(applyGuestReads(revised as Parameters<typeof applyGuestReads>[0], { 'notice:1': date }).unreadCount, 1);
});
