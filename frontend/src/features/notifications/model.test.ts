import assert from 'node:assert/strict';
import test from 'node:test';
import { cacheKey, safeMessageUrl, validFeed } from './model.ts';
const feed = () => ({ schemaVersion: 1, product: 'geod', accountId: 'a', checkedAt: new Date().toISOString(), unreadCount: 1, items: [{ id: 'notice', revision: 1, title: { zh: '消息', en: 'Notice' }, body: { zh: '内容' }, priority: 'important', publishedAt: new Date().toISOString(), expiresAt: null, readAt: null, action: null }] });
test('legacy inbox rejects wrong accounts, products, invalid revisions and corrupt caches', () => {
  assert.equal(validFeed(feed(), 'a'), true);
  assert.equal(validFeed(feed(), 'b'), false);
  assert.equal(validFeed({ ...feed(), product: 'geod-agent' }, 'a'), false);
  assert.equal(validFeed({ ...feed(), items: [{ ...feed().items[0], revision: 0 }] }, 'a'), false);
  assert.equal(validFeed({ ...feed(), items: [{ ...feed().items[0], publishedAt: 'invalid' }] }, 'a'), false);
  assert.equal(validFeed({ ...feed(), items: Array(101).fill(feed().items[0]) }, 'a'), false);
  assert.equal(validFeed(null, 'a'), false);
  assert.notEqual(cacheKey('a'), cacheKey('b'));
});
test('notification actions only open HTTPS links without credentials', () => {
  assert.equal(safeMessageUrl('https://geod.laogao.xyz/'), 'https://geod.laogao.xyz/');
  for (const url of ['javascript:alert(1)', 'file:///C:/test', 'http://example.test/', 'https://u:p@example.test', 'https://example.test/' + 'a'.repeat(2000)]) assert.throws(() => safeMessageUrl(url));
});
