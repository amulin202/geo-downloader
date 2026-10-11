import { useCallback, useEffect, useRef, useState } from 'react';
import { Bell, CheckCheck, ChevronDown, ExternalLink, Loader2, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { invokeCommand, isTauriRuntime } from '@/lib/tauri';
import { checkForUpdates } from '@/features/update/update-api';
import { applyGuestReads, cacheKey, safeMessageUrl, validFeed, type AppMessage, type GuestReads, type MessageFeed, type MessageText } from './model';
const errorText = (e: unknown) => e && typeof e === 'object' && 'message' in e ? String(e.message) : String(e);
export function MessageCenter() {
  const { t, i18n } = useTranslation();
  const [open, setOpen] = useState(false), [feed, setFeed] = useState<MessageFeed | null>(null), [loading, setLoading] = useState(false), [error, setError] = useState(''), [unreadOnly, setUnreadOnly] = useState(false), [expanded, setExpanded] = useState<string | null>(null);
  const epoch = useRef(0), lastFetch = useRef(0), seen = useRef<string[]>([]), guestReads = useRef<GuestReads>({}), fetching = useRef(false);
  const messageText = (v: MessageText) => i18n.language.startsWith('en') ? v.en ?? v.zh : v.zh;
  const persist = (value: MessageFeed) => { try { localStorage.setItem(cacheKey(), JSON.stringify({ feed: value, seen: seen.current.slice(-500), guestReads: guestReads.current })); } catch { /* Storage is optional. */ } };
  const refresh = useCallback(async () => {
    if (!isTauriRuntime() || fetching.current) return;
    const generation = ++epoch.current; fetching.current = true; setLoading(true); setError('');
    try {
      const response = await invokeCommand<unknown>('notifications_list');
      if (generation !== epoch.current) return;
      if (!validFeed(response)) throw new Error(t('notifications.accountMismatch'));
      const next = applyGuestReads(response, guestReads.current); setFeed(next); lastFetch.current = Date.now();
      const urgent = next.items.find(item => item.priority === 'important' && !item.readAt && !seen.current.includes(item.id + ':' + item.revision));
      if (urgent) { seen.current.push(urgent.id + ':' + urgent.revision); toast(messageText(urgent.title), { description: messageText(urgent.body).slice(0, 120), duration: 12000, action: { label: t('notifications.view'), onClick: () => setOpen(true) } }); }
      persist(next);
    } catch (e) { if (generation === epoch.current) setError(errorText(e)); }
    finally { if (generation === epoch.current) { fetching.current = false; setLoading(false); } }
  }, [t, i18n.language]);
  useEffect(() => {
    try { const raw = localStorage.getItem(cacheKey()); if (raw && raw.length < 1_500_000) { const saved = JSON.parse(raw); if (saved.guestReads && typeof saved.guestReads === 'object') guestReads.current = Object.fromEntries(Object.entries(saved.guestReads).slice(-1000).filter(([key, value]) => /^[A-Za-z0-9_-]{1,100}:\d+$/.test(key) && typeof value === 'string' && Number.isFinite(Date.parse(value)))) as GuestReads; if (validFeed(saved.feed)) setFeed(applyGuestReads(saved.feed, guestReads.current)); if (Array.isArray(saved.seen)) seen.current = saved.seen.filter((v: unknown) => typeof v === 'string').slice(-500); } } catch { /* Ignore corrupt cache. */ }
    void refresh();
    const poll = () => { if (!document.hidden && Date.now() - lastFetch.current >= 60000) void refresh(); };
    const timer = setInterval(poll, 60000); window.addEventListener('focus', poll); document.addEventListener('visibilitychange', poll);
    return () => { clearInterval(timer); window.removeEventListener('focus', poll); document.removeEventListener('visibilitychange', poll); epoch.current++; fetching.current = false; };
  }, [refresh]);
  const markRead = (ids: string[]) => {
    if (!feed) return; const wanted = new Set(ids), readAt = new Date().toISOString();
    for (const item of feed.items) if (wanted.has(item.id)) guestReads.current[item.id + ':' + item.revision] = readAt;
    guestReads.current = Object.fromEntries(Object.entries(guestReads.current).slice(-1000));
    const next = applyGuestReads(feed, guestReads.current); setFeed(next); persist(next);
  };
  const action = async (item: AppMessage) => {
    if (!item.action) return;
    try { if (item.action.kind === 'update') { setOpen(false); await checkForUpdates(false); } else await invokeCommand('notifications_open_link', { url: safeMessageUrl(item.action.url ?? '') }); } catch (e) { setError(errorText(e)); }
  };
  const available = (feed?.items ?? []).filter(item => !item.expiresAt || Date.parse(item.expiresAt) > Date.now());
  const items = available.filter(item => !unreadOnly || !item.readAt), unread = available.filter(item => !item.readAt).length;
  return <>
    <Button variant="ghost" size="icon" className="relative" aria-label={t('notifications.title')} title={unread ? t('notifications.unreadCount', { count: unread }) : t('notifications.title')} onClick={() => { setOpen(true); if (Date.now() - lastFetch.current > 60000) void refresh(); }}>
      <Bell className="size-4" />{unread > 0 && <span className="absolute right-2 top-2 size-1.5 rounded-full bg-destructive" aria-hidden />}
    </Button>
    <Dialog open={open} onOpenChange={setOpen}><DialogContent className="max-w-[calc(100vw-2rem)] sm:max-w-xl gap-4">
      <DialogHeader><DialogTitle className="flex items-center gap-2"><Bell className="size-5 text-primary" />{t('notifications.title')}{unread > 0 && <span className="rounded-full bg-primary/10 px-2 py-0.5 text-xs text-primary">{unread}</span>}</DialogTitle><DialogDescription>{t('notifications.description')}</DialogDescription></DialogHeader>
      {<div className="flex flex-wrap items-center justify-between gap-2"><div className="flex gap-1"><Button size="sm" variant={unreadOnly ? 'ghost' : 'secondary'} aria-pressed={!unreadOnly} onClick={() => setUnreadOnly(false)}>{t('notifications.all')}</Button><Button size="sm" variant={unreadOnly ? 'secondary' : 'ghost'} aria-pressed={unreadOnly} onClick={() => setUnreadOnly(true)}>{t('notifications.unread')}</Button></div><div className="flex gap-1"><Button size="sm" variant="ghost" disabled={!unread} onClick={() => void markRead((feed?.items ?? []).filter(item => !item.readAt).map(item => item.id))}><CheckCheck className="size-4" />{t('notifications.readAll')}</Button><Button size="icon" variant="ghost" disabled={loading} aria-label={t('notifications.refresh')} onClick={() => void refresh()}><RefreshCw className={`size-4 ${loading ? 'animate-spin' : ''}`} /></Button></div></div>}
      {error && <p role="alert" className="rounded-lg border border-destructive/20 bg-destructive/5 p-3 text-sm text-destructive">{error}{feed && <span className="block mt-1">{t('notifications.cached')}</span>}</p>}
      <div className="max-h-[min(420px,55vh)] min-h-52 overflow-y-auto pr-1 space-y-2">
        {loading && !feed ? <div className="flex min-h-52 items-center justify-center gap-2 text-muted-foreground"><Loader2 className="size-5 animate-spin" />{t('notifications.loading')}</div> : !items.length ? <div className="flex min-h-52 flex-col items-center justify-center gap-3 text-muted-foreground"><CheckCheck className="size-8" /><p>{t(unreadOnly ? 'notifications.noUnread' : 'notifications.empty')}</p></div> : items.map(item => <article key={`${item.id}:${item.revision}`} className={`rounded-lg border ${item.readAt ? 'border-border' : 'border-primary/20 bg-primary/5'}`}>
          <Button variant="ghost" className="h-auto w-full justify-start gap-3 whitespace-normal p-3 text-left" aria-expanded={expanded === item.id} onClick={() => { setExpanded(expanded === item.id ? null : item.id); if (!item.readAt) void markRead([item.id]); }}><span className={`size-1.5 shrink-0 rounded-full ${item.readAt ? 'bg-transparent' : 'bg-primary'}`} /><span className="min-w-0 flex-1"><span className="block font-medium break-words">{messageText(item.title)}</span><span className="mt-1 block text-xs text-muted-foreground">{new Date(item.publishedAt).toLocaleString(i18n.language)}{item.priority === 'important' && ` · ${t('notifications.important')}`}</span></span><ChevronDown className={`size-4 shrink-0 transition-transform ${expanded === item.id ? 'rotate-180' : ''}`} /></Button>
          {expanded === item.id && <div className="px-4 pb-4 pl-7"><p className="whitespace-pre-wrap break-words text-sm leading-6 text-muted-foreground">{messageText(item.body)}</p>{item.action && <Button size="sm" variant="outline" className="mt-3" onClick={() => void action(item)}>{item.action.kind === 'link' && <ExternalLink className="size-4" />}{messageText(item.action.label)}</Button>}</div>}
        </article>)}
      </div>
      {<div className="flex flex-wrap items-center justify-between gap-2 border-t pt-3"><p className="text-xs text-muted-foreground">{t('notifications.guest')}</p></div>}
    </DialogContent></Dialog>
  </>;
}
