// Browser-only QA: native commands are simulated; no production account is used.
import { pathToFileURL } from 'node:url';
const { chromium } = await import(pathToFileURL(process.env.PLAYWRIGHT_MODULE ?? 'C:/Users/Administrator/.cache/codex-runtimes/codex-primary-runtime/dependencies/node/node_modules/playwright/index.mjs').href);
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
const out = 'G:/code/tif-downloader/artifacts/desktop-3611/qa'; mkdirSync(out, { recursive: true });
const browser = await chromium.launch({ headless: true });
const report = [];
const html = `<!doctype html><meta charset="utf-8"><div id="root"></div><script type="module" src="/@vite/client"></script><script type="module">
import RefreshRuntime from '/@react-refresh';
RefreshRuntime.injectIntoGlobalHook(window); window.$RefreshReg$ = () => {}; window.$RefreshSig$ = () => type => type; window.__vite_plugin_react_preamble_installed__ = true;
</script><script type="module">
import React from '/node_modules/.vite/deps/react.js';
import ReactDOM from '/node_modules/.vite/deps/react-dom_client.js';
import {initializeI18n} from '/src/i18n/index.ts';
import {ThemeProvider} from '/src/components/theme/theme-provider.tsx';
import {Toaster} from '/src/components/ui/sonner.tsx';
import {MessageCenter} from '/src/features/notifications/message-center.tsx';
import '/src/index.css';
await initializeI18n();
ReactDOM.createRoot(document.getElementById('root')).render(React.createElement(ThemeProvider,null,React.createElement('div',{style:{padding:24}},React.createElement(MessageCenter)),React.createElement(Toaster)));
</script>`;
try {
  for (const [lang, theme, size] of [['zh-CN', 'light', { width: 1400, height: 900 }], ['en-US', 'dark', { width: 1000, height: 700 }]]) {
    const page = await browser.newPage({ viewport: size });
    const errors = []; page.on('pageerror', e => { if (errors.length < 20) { errors.push(e.message); if (errors.length <= 2) console.error(e.stack); } });
    // Disable Vite's dev transport inside the routed fixture; keep real product rendering.
    await page.route('**/@vite/client', route => route.fulfill({ contentType: 'application/javascript', body: `
      export const createHotContext = () => ({data:{},accept(){},acceptExports(){},dispose(){},invalidate(){},send(){},on(){},off(){},prune(){}});
      const styles=new Map(); export function updateStyle(id,css){let el=styles.get(id);if(!el){el=document.createElement('style');styles.set(id,el);document.head.append(el);}el.textContent=css;}
      export function removeStyle(id){styles.get(id)?.remove();styles.delete(id);} export const injectQuery=(url)=>url;
    ` }));
    await page.route('**/__qa_messages', route => route.fulfill({ contentType: 'text/html', body: html }));
    await page.addInitScript(({ lang, theme }) => {
      localStorage.clear(); localStorage.setItem('geod-language', lang); localStorage.setItem('gd:theme-mode', theme);
      const items = Array.from({ length: 14 }, (_, i) => ({ id: `notice-${i}`, revision: 1, title: { zh: `GeoD 更新通知 ${i + 1}`, en: `GeoD update ${i + 1}` }, body: { zh: '下载保护与消息中心已更新。\n无需登录即可查看产品更新与服务公告。', en: 'Download protections and the message center have been updated.\nRead public product updates and service notices without signing in.' }, priority: i === 0 ? 'important' : 'normal', publishedAt: '2026-10-11T05:00:00Z', expiresAt: null, readAt: null, action: i === 0 ? { kind: 'link', label: { zh: '查看更新说明', en: 'Release notes' }, url: 'https://geod.laogao.xyz/' } : null }));
      const qa = window.__qa = { items, calls: [], fail: false };
      window.__TAURI_INTERNALS__ = { invoke: async (command, args) => {
        qa.calls.push({ command, args });
        if (command === 'notifications_list') {
          if (qa.fail) throw new Error('QA offline');
          const items = structuredClone(qa.items).map(x => ({ ...x, readAt: null }));
          return { schemaVersion: 1, product: 'geod', accountId: null, checkedAt: new Date().toISOString(), unreadCount: items.length, items };
        }
        if (command === 'notifications_open_link') return;
        throw new Error('Unexpected native command ' + command);
      } };
    }, { lang, theme });
    await page.goto('http://127.0.0.1:1431/__qa_messages');
    const zh = lang === 'zh-CN';
    await page.getByRole('button', { name: zh ? '消息中心' : 'Message center', exact: true }).click();
    await page.getByRole('button', { name: new RegExp(zh ? 'GeoD 更新通知 1' : 'GeoD update 1') }).first().click();

    await page.waitForFunction(() => document.querySelector('[role=dialog]')?.textContent.includes('13'));
    const scroll = await page.locator('[role=dialog] .overflow-y-auto').evaluate(el => ({ height: el.clientHeight, total: el.scrollHeight }));
    assert(scroll.total > scroll.height); assert(scroll.height <= size.height * .55 + 2);
    await page.screenshot({ path: `${out}/messages-${lang}-${theme}.png` });
    await page.getByRole('button', { name: zh ? '全部已读' : 'Mark all read' }).click();

    await page.getByRole('button', { name: zh ? '未读' : 'Unread', exact: true }).click();
    await page.getByText(zh ? '没有未读消息' : 'No unread messages', { exact: true }).waitFor();
    await page.evaluate(() => window.__qa.fail = true);
    await page.getByRole('button', { name: zh ? '刷新消息' : 'Refresh messages' }).click();
    await page.getByRole('alert').filter({ hasText: 'QA offline' }).waitFor();
    await page.evaluate(() => window.__qa.fail = false);
    await page.getByRole('button', { name: zh ? '全部消息' : 'All messages', exact: true }).click();
    await page.getByRole('button', { name: zh ? '刷新消息' : 'Refresh messages' }).click();
    await page.getByRole('button', { name: zh ? '未读' : 'Unread', exact: true }).click();
    await page.getByText(zh ? '没有未读消息' : 'No unread messages', { exact: true }).waitFor();
    await page.getByRole('button', { name: zh ? '全部消息' : 'All messages', exact: true }).click();
    assert.equal(await page.getByRole('button', { name: zh ? '登录 GeoD' : 'Sign in to GeoD' }).count(), 0);
    assert.equal(await page.evaluate(() => window.__qa.calls.some(x => x.command.startsWith('auth_') || x.command === 'notifications_read')), false);
    await page.screenshot({ path: out + '/public-messages-' + lang + '-' + theme + '.png' });
    assert.deepEqual(errors, []);
    report.push({ lang, theme, viewport: size, checks: ['unread', 'local-read-persistence', 'read-all', 'scroll-bound', 'offline-cache', 'no-login-controls', 'no-account-commands'], scroll });
    await page.close();
  }
  // Full desktop shell (no native mock) checks placement and real layout integration.
  const page = await browser.newPage({ viewport: { width: 1400, height: 900 } });
  page.on('pageerror', e => console.error('Full shell:', e.message));
  await page.addInitScript(() => localStorage.setItem('geod-language', 'zh-CN'));
  await page.goto('http://127.0.0.1:1431/');
  await page.screenshot({ path: `${out}/desktop-initial-debug.png` });
  await page.locator('button[aria-label="消息中心"]').click();
  await page.getByText('公共公告 · 已读状态保存在本机', { exact: true }).waitFor();
  await page.screenshot({ path: `${out}/desktop-message-entry.png` });
  writeFileSync(`${out}/ui-report.json`, JSON.stringify({ fixtureOnly: true, report, fullShell: true }, null, 2));
  console.log(JSON.stringify({ ok: true, report, fullShell: true }));
} finally { await browser.close(); }
