import { useCallback, useEffect, useRef, useState } from 'react';
import { Bell, CheckCheck, ChevronDown, ExternalLink, Loader2, LogIn, LogOut, RefreshCw } from 'lucide-react';
import { useTranslation } from 'react-i18next';
import { toast } from 'sonner';
import { Button } from '@/components/ui/button';
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from '@/components/ui/dialog';
import { invokeCommand, isTauriRuntime } from '@/lib/tauri';
import { checkForUpdates } from '@/features/update/update-api';
import { cacheKey, safeMessageUrl, validFeed, type AppMessage, type MessageFeed, type MessageText } from './model';

interface Auth { state: 'connected' | 'disconnected' | 'waiting' | 'unconfigured'; userId: string | null; error: string | null }
const initial: Auth = { state: 'disconnected', userId: null, error: null };
const errorText = (e: unknown) => e && typeof e === 'object' && 'message' in e ? String(e.message) : String(e);
export function MessageCenter() {
  const { t, i18n } = useTranslation();
  const [auth, setAuth] = useState<Auth>(initial), [open, setOpen] = useState(false), [feed, setFeed] = useState<MessageFeed | null>(null);
  const [loading, setLoading] = useState(false), [working, setWorking] = useState(false), [error, setError] = useState(''), [unreadOnly, setUnreadOnly] = useState(false), [expanded, setExpanded] = useState<string | null>(null);
  const owner = useRef<string | null>(null), epoch = useRef(0), inFlight = useRef<number | null>(null), lastFetch = useRef(0), seen = useRef<string[]>([]);
  const authRequest = useRef(0);
  const messageText = (v: MessageText) => i18n.language.startsWith('en') ? v.en ?? v.zh : v.zh;
  const persist = (id: string, value: MessageFeed) => { try { localStorage.setItem(cacheKey(id), JSON.stringify({ feed: value, seen: seen.current.slice(-500) })); } catch { /* Storage is optional. */ } };
  const refresh = useCallback(async () => {
    const id = owner.current, generation = epoch.current;
    if (!id || !isTauriRuntime() || inFlight.current === generation) return;
    inFlight.current = generation; setLoading(true); setError('');
    try {
      const next = await invokeCommand<unknown>('notifications_list');
      if (generation !== epoch.current || owner.current !== id) return;
      if (!validFeed(next, id)) throw new Error(t('notifications.accountMismatch'));
      setFeed(next); lastFetch.current = Date.now();
      const urgent = next.items.find(item => item.priority === 'important' && !item.readAt && !seen.current.includes(`${item.id}:${item.revision}`));
      if (urgent) {
        seen.current.push(`${urgent.id}:${urgent.revision}`);
        toast(messageText(urgent.title), { description: messageText(urgent.body).slice(0, 120), duration: 12000, action: { label: t('notifications.view'), onClick: () => setOpen(true) } });
      }
      persist(id, next);
    } catch (e) { if (generation === epoch.current) setError(errorText(e)); }
    finally { if (inFlight.current === generation) inFlight.current = null; if (generation === epoch.current) setLoading(false); }
  }, [t, i18n.language]);
  const readAuth = useCallback(async () => {
    if (!isTauriRuntime()) return;
    const request = ++authRequest.current;
    try { const next = await invokeCommand<Auth>('auth_status'); if (request === authRequest.current) setAuth(next); } catch (e) { if (request === authRequest.current) setError(errorText(e)); }
  }, []);
  useEffect(() => { void readAuth(); const focus = () => void readAuth(); window.addEventListener('focus', focus); return () => window.removeEventListener('focus', focus); }, [readAuth]);
  useEffect(() => {
    if (auth.state !== 'waiting') return;
    const timer = setInterval(() => void readAuth(), 1500); return () => clearInterval(timer);
  }, [auth.state, readAuth]);
  useEffect(() => {
    const id = auth.state === 'connected' ? auth.userId : null;
    if (owner.current === id) return;
    epoch.current++; owner.current = id; inFlight.current = null; seen.current = []; lastFetch.current = 0;
    setFeed(null); setExpanded(null); setLoading(false); setWorking(false); setError('');
    if (!id) return;
    try { const raw = localStorage.getItem(cacheKey(id)); if (raw && raw.length < 1_500_000) { const saved = JSON.parse(raw); if (validFeed(saved.feed, id)) setFeed(saved.feed); if (Array.isArray(saved.seen)) seen.current = saved.seen.filter((x: unknown) => typeof x === 'string').slice(-500); } } catch { /* Ignore corrupt cache. */ }
    void refresh();
  }, [auth.state, auth.userId, refresh]);
  useEffect(() => {
    const poll = () => { if (!document.hidden && owner.current && Date.now() - lastFetch.current >= 60000) void refresh(); };
    const timer = setInterval(poll, 60000); window.addEventListener('focus', poll); document.addEventListener('visibilitychange', poll);
    return () => { clearInterval(timer); window.removeEventListener('focus', poll); document.removeEventListener('visibilitychange', poll); };
  }, [refresh]);
  const markRead = async (ids: string[]) => {
    const id = owner.current, generation = epoch.current;
    if (!id || working || !ids.length) return;
    setWorking(true); setError('');
    try {
      const result = await invokeCommand<{ accountId: string; accepted: number }>('notifications_read', { accountId: id, messageIds: ids });
      if (generation !== epoch.current) return;
      if (result.accountId !== id) throw new Error(t('notifications.accountMismatch'));
      // Read back authoritative revisions, not optimistic timestamps for an old revision.
      epoch.current++; inFlight.current = null;
      await refresh();
    } catch (e) { if (generation === epoch.current) setError(errorText(e)); }
    finally { if (owner.current === id) setWorking(false); }
  };
  const login = async () => { const request = ++authRequest.current; setError(''); try { const next = await invokeCommand<Auth>('auth_begin'); if (request === authRequest.current) setAuth(next); } catch (e) { if (request === authRequest.current) setError(errorText(e)); } };
  const logout = async () => { const request = ++authRequest.current; setError(''); try { const next = await invokeCommand<Auth>('auth_logout'); if (request === authRequest.current) setAuth(next); } catch (e) { if (request === authRequest.current) setError(errorText(e)); } };
  const action = async (item: AppMessage) => {
    if (!item.action) return;
    try { if (item.action.kind === 'update') { setOpen(false); await checkForUpdates(false); } else await invokeCommand('notifications_open_link', { url: safeMessageUrl(item.action.url ?? '') }); } catch (e) { setError(errorText(e)); }
  };
  const currentAccount = auth.state === 'connected' ? auth.userId : null;
  const available = (feed?.accountId === currentAccount ? feed.items : []).filter(item => !item.expiresAt || Date.parse(item.expiresAt) > Date.now());
  const items = available.filter(item => !unreadOnly || !item.readAt);
  const unread = available.filter(item => !item.readAt).length;
  return <>
    <Button variant="ghost" size="icon" className="relative" aria-label={t('notifications.title')} title={unread ? t('notifications.unreadCount', { count: unread }) : t('notifications.title')} onClick={() => { setOpen(true); void readAuth(); if (Date.now() - lastFetch.current > 60000) void refresh(); }}>
      <Bell className="size-4" />{unread > 0 && <span className="absolute right-2 top-2 size-1.5 rounded-full bg-destructive" aria-hidden />}
    </Button>
    <Dialog open={open} onOpenChange={setOpen}><DialogContent className="max-w-[calc(100vw-2rem)] sm:max-w-xl gap-4">
      <DialogHeader><DialogTitle className="flex items-center gap-2"><Bell className="size-5 text-primary" />{t('notifications.title')}{unread > 0 && <span className="rounded-full bg-primary/10 px-2 py-0.5 text-xs text-primary">{unread}</span>}</DialogTitle><DialogDescription>{t('notifications.description')}</DialogDescription></DialogHeader>
      {owner.current && <div className="flex flex-wrap items-center justify-between gap-2"><div className="flex gap-1"><Button size="sm" variant={unreadOnly ? 'ghost' : 'secondary'} aria-pressed={!unreadOnly} onClick={() => setUnreadOnly(false)}>{t('notifications.all')}</Button><Button size="sm" variant={unreadOnly ? 'secondary' : 'ghost'} aria-pressed={unreadOnly} onClick={() => setUnreadOnly(true)}>{t('notifications.unread')}</Button></div><div className="flex gap-1"><Button size="sm" variant="ghost" disabled={working || !unread} onClick={() => void markRead((feed?.items ?? []).filter(item => !item.readAt).map(item => item.id))}><CheckCheck className="size-4" />{t('notifications.readAll')}</Button><Button size="icon" variant="ghost" disabled={loading} aria-label={t('notifications.refresh')} onClick={() => void refresh()}><RefreshCw className={`size-4 ${loading ? 'animate-spin' : ''}`} /></Button></div></div>}
      {(error || auth.error) && <p role="alert" className="rounded-lg border border-destructive/20 bg-destructive/5 p-3 text-sm text-destructive">{error || auth.error}{feed && <span className="block mt-1">{t('notifications.cached')}</span>}</p>}
      <div className="max-h-[min(420px,55vh)] min-h-52 overflow-y-auto pr-1 space-y-2">
        {!owner.current ? <div className="flex min-h-52 flex-col items-center justify-center gap-3 text-center"><Bell className="size-8 text-muted-foreground" /><h3 className="font-medium">{t('notifications.connect')}</h3><p className="max-w-sm text-sm text-muted-foreground">{t('notifications.connectDescription')}</p><Button disabled={!isTauriRuntime() || auth.state === 'waiting'} onClick={() => void login()}>{auth.state === 'waiting' ? <Loader2 className="size-4 animate-spin" /> : <LogIn className="size-4" />}{t(auth.state === 'waiting' ? 'notifications.waiting' : 'notifications.login')}</Button></div> : loading && !feed ? <div className="flex min-h-52 items-center justify-center gap-2 text-muted-foreground"><Loader2 className="size-5 animate-spin" />{t('notifications.loading')}</div> : !items.length ? <div className="flex min-h-52 flex-col items-center justify-center gap-3 text-muted-foreground"><CheckCheck className="size-8" /><p>{t(unreadOnly ? 'notifications.noUnread' : 'notifications.empty')}</p></div> : items.map(item => <article key={`${item.id}:${item.revision}`} className={`rounded-lg border ${item.readAt ? 'border-border' : 'border-primary/20 bg-primary/5'}`}>
          <Button variant="ghost" className="h-auto w-full justify-start gap-3 whitespace-normal p-3 text-left" aria-expanded={expanded === item.id} onClick={() => { setExpanded(expanded === item.id ? null : item.id); if (!item.readAt) void markRead([item.id]); }}><span className={`size-1.5 shrink-0 rounded-full ${item.readAt ? 'bg-transparent' : 'bg-primary'}`} /><span className="min-w-0 flex-1"><span className="block font-medium break-words">{messageText(item.title)}</span><span className="mt-1 block text-xs text-muted-foreground">{new Date(item.publishedAt).toLocaleString(i18n.language)}{item.priority === 'important' && ` · ${t('notifications.important')}`}</span></span><ChevronDown className={`size-4 shrink-0 transition-transform ${expanded === item.id ? 'rotate-180' : ''}`} /></Button>
          {expanded === item.id && <div className="px-4 pb-4 pl-7"><p className="whitespace-pre-wrap break-words text-sm leading-6 text-muted-foreground">{messageText(item.body)}</p>{item.action && <Button size="sm" variant="outline" className="mt-3" onClick={() => void action(item)}>{item.action.kind === 'link' && <ExternalLink className="size-4" />}{messageText(item.action.label)}</Button>}</div>}
        </article>)}
      </div>
      {owner.current && <div className="flex items-center justify-between border-t pt-3"><p className="text-xs text-muted-foreground">{t('notifications.connected')}</p><Button size="sm" variant="ghost" onClick={() => void logout()}><LogOut className="size-3.5" />{t('notifications.logout')}</Button></div>}
    </DialogContent></Dialog>
  </>;
}
