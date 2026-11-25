<script>
  // Trang Cổng (theo RustWTM "Ports"): chương trình nào nghe cổng nào, ai truy cập được, giải phóng một cổng đang bận,
  // bảng công cụ lập trình / máy chủ quen thuộc (Vite, PostgreSQL, Redis…) và theo dõi cổng tuỳ chọn.
  //
  // Hiệu năng: chỉ hỏi backend khi trang đang mở, cửa sổ đang hiện và chưa tạm dừng cập nhật – mỗi 2 giây
  // (đọc bảng IP Helper ~2 ms). Tên / CPU / RAM của tiến trình giữ cổng lấy từ lần đo sẵn có của monitor,
  // không bật luồng tiến trình đầy đủ.
  import { onMount } from 'svelte';
  import { store } from './store.svelte.js';
  import { ai } from './ai.svelte.js';
  import { procs } from './procs.svelte.js';
  import { prefs, loadPref, savePref } from './prefs.svelte.js';
  import * as api from './api.js';
  import { toasts, summarize } from './toasts.svelte.js';
  import { PORTS_CHANGED, KNOWN, CAT, known, browsable, REACH, REACH_ORDER, parsePort, endBlocked, relayHint } from './ports.js';
  import { collator, errText, fmtBytes, fmtPct, fmtInt } from './format.js';
  import DataTable from './DataTable.svelte';
  import SearchBox from './SearchBox.svelte';
  import Glyph from './Glyph.svelte';
  import RefreshButton from './RefreshButton.svelte';
  import ContextMenu from './ContextMenu.svelte';
  import { copyText } from './format.js';
  import { t } from './i18n.svelte.js';

  const POLL_MS = 2000;

  let uses = $state.raw([]);
  let loaded = $state(false);
  let err = $state('');
  let tookMs = $state(0);
  let udp = $state(loadPref('ports:udp', false));
  let showCatalog = $state(loadPref('ports:catalog', false));
  let watched = $state(loadPref('ports:watched', []));
  let q = $state('');
  let freeText = $state('');
  let watchText = $state('');
  let sort = $state(loadPref('ports:sort', { key: 'port', desc: false }));

  // ---------- tải định kỳ ----------
  let busy = false;
  let quick = 0;
  let destroyed = false;

  function canPoll() {
    return !destroyed && !document.hidden && prefs.interval > 0;
  }

  async function load() {
    if (busy || !canPoll()) return;
    busy = true;
    try {
      const r = await api.portsList(udp);
      uses = r?.uses || [];
      tookMs = r?.took_ms || 0;
      err = '';
      loaded = true;
      // tiến trình mới chưa có tên -> hỏi lại sớm một lần (monitor vừa được đánh thức)
      if (r?.pending && !quick && canPoll()) {
        quick = setTimeout(() => {
          quick = 0;
          load();
        }, 400);
      }
    } catch (e) {
      err = errText(e);
      loaded = true;
    } finally {
      busy = false;
    }
  }
  onMount(() => {
    load();
    const timer = setInterval(load, POLL_MS);
    window.addEventListener(PORTS_CHANGED, load);
    return () => {
      destroyed = true;
      clearInterval(timer);
      clearTimeout(quick);
      window.removeEventListener(PORTS_CHANGED, load);
    };
  });
  function setUdp(v) {
    udp = v;
    savePref('ports:udp', v);
    load();
  }

  // ---------- gộp theo cổng ----------
  // port -> [use…] (chỉ TCP cho bảng công cụ & ô giải phóng: UDP không "bận" kiểu address already in use)
  const byPort = $derived.by(() => {
    const m = new Map();
    for (const u of uses) {
      let a = m.get(u.port);
      if (!a) m.set(u.port, (a = []));
      a.push(u);
    }
    return m;
  });
  const tcpPorts = $derived(new Set(uses.filter((u) => u.proto === 'tcp').map((u) => u.port)));
  const summary = $derived.by(() => {
    let tools = 0, open = 0;
    const openPorts = new Set();
    for (const u of uses) if (u.proto === 'tcp' && u.reach === 'all') openPorts.add(u.port);
    for (const p of tcpPorts) if (known(p)) tools++;
    open = openPorts.size;
    return { tcp: tcpPorts.size, tools, open };
  });

  const procText = (list) => [...new Set(list.map((u) => `${u.name || '?'} (${u.pid})`))].join(', ');
  const reachOf = (list) => list.reduce((best, u) => (REACH_ORDER[u.reach] > REACH_ORDER[best] ? u.reach : best), 'local');
  const blockedOf = (list) => list.map((u) => endBlocked(u.pid, u.name)).find(Boolean) || null;
  const relayOf = (list) => list.map((u) => relayHint(u.name)).find(Boolean) || null;

  // ---------- giải phóng cổng ----------
  const freePort = $derived(parsePort(freeText));
  const freeUses = $derived(freePort ? (byPort.get(freePort) || []).filter((u) => u.proto === 'tcp') : []);

  // ---------- bảng công cụ ----------
  const toolRows = $derived.by(() => {
    const list = watched.map((p) => ({ port: p, k: known(p), watched: true }));
    for (const k of KNOWN) if (!watched.includes(k.port)) list.push({ port: k.port, k, watched: false });
    return list
      .map((r) => ({ ...r, uses: (byPort.get(r.port) || []).filter((u) => u.proto === 'tcp') }))
      .filter((r) => r.watched || showCatalog || r.uses.length);
  });
  function addWatch() {
    const p = parsePort(watchText);
    if (!p) return;
    if (!watched.includes(p)) {
      watched = [...watched, p].sort((a, b) => a - b);
      savePref('ports:watched', watched);
    }
    watchText = '';
  }
  function unwatch(p) {
    watched = watched.filter((x) => x !== p);
    savePref('ports:watched', watched);
  }

  // ---------- bảng mọi cổng ----------
  const columns = $derived([
    { key: 'port', label: t('ports.col.port'), width: 70, min: 56, align: 'right' },
    { key: 'proto', label: t('ports.col.proto'), width: 92, min: 80 },
    { key: 'known', label: t('ports.col.known'), width: 220, min: 100 },
    { key: 'name', label: t('ports.col.program'), width: 170, min: 90 },
    { key: 'pid', label: 'PID', width: 70, min: 50, align: 'right' },
    { key: 'reach', label: t('ports.col.reach'), width: 110, min: 80 },
    { key: 'conns', label: t('ports.col.conns'), width: 80, min: 70, align: 'right' },
    { key: 'cpu', label: 'CPU', width: 66, min: 50, align: 'right' },
    { key: 'mem', label: t('ports.col.mem'), width: 90, min: 66, align: 'right' },
    { key: 'addrs', label: t('ports.col.addrs'), width: 200, min: 90 },
  ]);
  const rows = $derived.by(() => {
    const f = q;
    const list = [];
    for (const u of uses) {
      if (!udp && u.proto !== 'tcp') continue;
      const k = known(u.port);
      if (f && !(String(u.port).includes(f) || (u.name || '').toLowerCase().includes(f) || (k && k.tool.toLowerCase().includes(f)) || String(u.pid) === f)) continue;
      list.push({ ...u, id: `${u.proto}:${u.port}:${u.pid}`, k });
    }
    const { key, desc } = sort;
    const dir = desc ? -1 : 1;
    const num = { port: 1, pid: 1, conns: 1, cpu: 1, mem: 1 };
    let cmp;
    if (num[key]) cmp = (a, b) => (a[key] - b[key]) * dir || a.port - b.port;
    else if (key === 'reach') cmp = (a, b) => (REACH_ORDER[a.reach] - REACH_ORDER[b.reach]) * dir || a.port - b.port;
    else if (key === 'known') cmp = (a, b) => collator.compare(a.k?.tool || '', b.k?.tool || '') * dir || a.port - b.port;
    else if (key === 'addrs') cmp = (a, b) => collator.compare(a.addrs.join(','), b.addrs.join(',')) * dir || a.port - b.port;
    else cmp = (a, b) => collator.compare(String(a[key] || ''), String(b[key] || '')) * dir || a.port - b.port;
    return list.sort(cmp);
  });
  function onsort(key) {
    sort = sort.key === key ? { key, desc: !sort.desc } : { key, desc: key === 'cpu' || key === 'mem' || key === 'conns' };
    savePref('ports:sort', sort);
  }

  // ---------- thao tác ----------
  async function endUses(list, port) {
    const why = blockedOf(list);
    if (why) return toasts.error(t('ports.toast.end_failed', { port }), [why]);
    const pids = [...new Set(list.map((u) => u.pid))];
    const snapshot = new Map(pids.map((pid) => [pid, procs.map.get(pid)]));
    const ownerKey = ({ pid, port, proto }) => `${pid}:${port}:${proto}`;
    const confirmedOwners = new Set(list.map(ownerKey));
    const relay = relayOf(list);
    const yes = await store.confirm({
      title: t('ports.confirm.title', { port }),
      message: t('ports.confirm.message', { procs: procText(list), port }) + (relay ? ' ' + relay + '.' : ''),
      okLabel: t('ports.act.end'),
      danger: true,
    });
    if (!yes) return;
    try {
      // Trang Cổng không bật luồng tiến trình, nên kiểm tra lại chủ cổng nếu chưa có trong bộ nhớ.
      const verifiedOwners = new Set();
      if (pids.some((pid) => !snapshot.get(pid)) && canPoll()) {
        const fresh = await api.portsList(udp);
        for (const owner of fresh?.uses || []) {
          if (confirmedOwners.has(ownerKey(owner))) verifiedOwners.add(owner.pid);
        }
      }
      const currentPids = [];
      const errors = [];
      for (const pid of pids) {
        const previous = snapshot.get(pid);
        const sameOwner = previous ? procs.map.get(pid) === previous : verifiedOwners.has(pid);
        if (sameOwner) currentPids.push(pid);
        else errors.push(`PID ${pid}: ${t('ai.review.kill.not_found')}`);
      }
      const res = currentPids.length ? (await api.procKill(currentPids, false)) || [] : [];
      const failed = res.filter((x) => !x.ok);
      for (const { pid, error } of failed) errors.push(`PID ${pid}: ${error || t('ports.error')}`);
      const ok = res.length - failed.length;
      if (ok + errors.length === 1) {
        if (errors.length) toasts.error(t('ports.toast.end_failed', { port }), errors);
        else toasts.success(t('ports.toast.freed', { port }), [procText(list)]);
      } else summarize(t('ports.summ.verb'), t('ports.summ.unit'), ok, errors);
    } catch (e) {
      toasts.error(t('ports.toast.end_failed', { port }), [errText(e)]);
    }
    load();
  }
  async function openPort(port) {
    try { await api.openPort(port); } catch (e) { toasts.error(t('ports.toast.open_failed', { port }), [errText(e)]); }
  }
  function showProc(pid) {
    procs.revealPid = pid;
    store.page = 'processes';
  }
  // ---------- menu chuột phải của bảng mọi cổng ----------
  let menu = $state(null); // { x, y, r }
  const menuItems = $derived.by(() => {
    if (!menu) return [];
    const r = menu.r;
    const why = endBlocked(r.pid, r.name);
    return [
      { id: 'open', label: t('ports.act.open_url', { port: r.port }), glyph: 'open', disabled: !browsable(r.port, r.proto) },
      { id: 'show', label: t('ports.act.show_proc'), glyph: 'info', hint: t('ports.act.dblclick') },
      { sep: true },
      { id: 'end', label: why ? t('ports.act.end_blocked', { why }) : r.name ? t('ports.act.end_name', { name: r.name }) : t('ports.act.end_program'), glyph: 'close', danger: true, disabled: !!why },
      { sep: true },
      { id: 'copy', label: t('ports.act.copy'), glyph: 'copy', sub: [
        { id: 'copy:port', label: t('ports.col.port') }, { id: 'copy:pid', label: 'PID' }, { id: 'copy:addr', label: t('ports.col.addrs') },
        { id: 'copy:path', label: t('ports.act.copy_path'), disabled: !r.exe },
      ] },
    ];
  });
  async function pickMenu(id) {
    const r = menu?.r;
    menu = null;
    if (!r) return;
    if (id === 'open') return openPort(r.port);
    if (id === 'show') return showProc(r.pid);
    if (id === 'end') return endUses([r], r.port);
    if (id.startsWith('copy:')) {
      const k = id.slice(5);
      const text = k === 'port' ? String(r.port) : k === 'pid' ? String(r.pid) : k === 'addr' ? r.addrs.map((a) => (a.includes(':') ? `[${a}]` : a) + ':' + r.port).join('\n') : r.exe;
      if (text && (await copyText(text))) toasts.success(t('ports.toast.copied'));
      else toasts.error(t('ports.toast.copy_failed'));
    }
  }

  function freeKey(e) {
    if (e.key === 'Enter' && freePort && freeUses.length) endUses(freeUses, freePort);
  }
</script>

<section class="page ports-page">
  <header class="page-head">
    <div>
      <h1>{t('ports.title')}</h1>
      <p class="muted ports-sum">
        {#if loaded && !err}
          <b>{fmtInt(summary.tcp)}</b> {t('ports.sum.tcp', { count: summary.tcp })} <span class="dot-sep">|</span>
          <b>{fmtInt(summary.tools)}</b> {t('ports.sum.tools', { count: summary.tools })} <span class="dot-sep">|</span>
          <span title={t('ports.sum.open_tip')}><b class:warn-text={summary.open > 0}>{fmtInt(summary.open)}</b> {t('ports.sum.open')}</span>
        {:else if err}
          <span class="warn-text">{err}</span>
        {:else}
          {t('ports.reading')}
        {/if}
      </p>
    </div>
    <div class="ph-actions">
      <button type="button" class="btn" onclick={() => ai.askPorts()} title={t('ports.ai.ask_tip')}><Glyph name="chat" size={12} /> {t('ports.ai.ask')}</button>
      <RefreshButton onclick={load} info={loaded && !err ? `${tookMs.toFixed(1)} ms` : ''} title={t('ports.refresh_tip')} />
    </div>
  </header>

  <div class="free-bar">
    <label class="free-lbl" for="free-port">{t('ports.free.label')}</label>
    <input id="free-port" class="input sm port-input" placeholder={t('ports.free.placeholder', { port: 3000 })} bind:value={freeText} onkeydown={freeKey} spellcheck="false" />
    {#if !freeText.trim()}
      <span class="muted small">{t('ports.free.hint')}</span>
    {:else if !freePort}
      <span class="warn-text small">{t('ports.free.not_port')}</span>
    {:else if !freeUses.length}
      <span class="ok small">{t('ports.free.is_free', { port: freePort })}</span>
    {:else}
      {@const why = blockedOf(freeUses)}
      <button type="button" class="btn sm danger" disabled={!!why} title={why || t('ports.free.end_tip')} onclick={() => endUses(freeUses, freePort)}>
        {t('ports.free.end', { port: freePort })}
      </button>
      <span class="small">{procText(freeUses)}</span>
      {#if why}<span class="muted small">{why}</span>{:else if relayOf(freeUses)}<span class="warn-text small">{relayOf(freeUses)}</span>{/if}
    {/if}
  </div>

  <section class="card tools-card">
    <div class="card-head">
      <h3>{t('ports.tools.title')}</h3>
      <div class="tools-ctl">
        <label class="check"><input type="checkbox" checked={showCatalog} onchange={(e) => { showCatalog = e.currentTarget.checked; savePref('ports:catalog', showCatalog); }} /> {t('ports.tools.show_free')}</label>
        <span class="sep"></span>
        <label class="free-lbl" for="watch-port">{t('ports.tools.watch_label')}</label>
        <input id="watch-port" class="input sm port-input" placeholder={t('ports.free.placeholder', { port: 8090 })} bind:value={watchText} onkeydown={(e) => e.key === 'Enter' && addWatch()} spellcheck="false" />
        <button type="button" class="btn sm" disabled={!parsePort(watchText)} onclick={addWatch}>{t('ports.tools.watch')}</button>
      </div>
    </div>
    {#if !toolRows.length}
      <p class="muted small">{t('ports.tools.none')}</p>
    {:else}
      <div class="tools-scroll">
        <table class="tools">
          <thead>
            <tr><th></th><th class="r">{t('ports.col.port')}</th><th>{t('ports.tools.col.tool')}</th><th>{t('ports.tools.col.group')}</th><th>{t('ports.col.program')}</th><th>{t('ports.col.reach')}</th><th class="r">{t('ports.col.conns')}</th><th></th></tr>
          </thead>
          <tbody>
            {#each toolRows as r (r.port)}
              {@const inUse = r.uses.length > 0}
              <tr>
                <td>{#if inUse}<span class="ok" title={t('ports.tools.in_use')}>●</span>{:else}<span class="faint" title={t('ports.tools.free')}>○</span>{/if}</td>
                <td class="r mono"><b>{r.port}</b></td>
                <td class="ellipsis">{r.k?.tool || t('ports.tools.watched')}</td>
                <td class="muted ellipsis">{r.k ? CAT[r.k.cat] : ''}</td>
                {#if inUse}
                  <td class="ellipsis" title={relayOf(r.uses) || procText(r.uses)}>{procText(r.uses)}</td>
                  <td><span class="reach reach-{reachOf(r.uses)}" title={REACH[reachOf(r.uses)][1]}>{REACH[reachOf(r.uses)][0]}</span></td>
                  <td class="r mono">{r.uses.reduce((a, u) => a + u.conns, 0)}</td>
                {:else}
                  <td class="faint">{t('ports.tools.free')}</td><td></td><td></td>
                {/if}
                <td>
                  <!-- ô nút cố định: nút vắng mặt vẫn giữ chỗ để các cột nút luôn thẳng hàng -->
                  <span class="act-slots">
                    {#if inUse}
                      {@const why = blockedOf(r.uses)}
                      {#if browsable(r.port, 'tcp')}<button type="button" class="btn xs" title={t('ports.act.open_url', { port: r.port })} onclick={() => openPort(r.port)}><Glyph name="open" size={10} /> {t('ports.act.open')}</button>{:else}<span></span>{/if}
                      <button type="button" class="btn xs" title={t('ports.act.show_proc')} onclick={() => showProc(r.uses[0].pid)}>{t('ports.act.show')}</button>
                      <button type="button" class="btn xs danger" disabled={!!why} title={why || relayOf(r.uses) || t('ports.act.end_tip')} onclick={() => endUses(r.uses, r.port)}>{t('ports.act.end')}</button>
                    {:else}<span></span><span></span><span></span>{/if}
                    {#if r.watched}<button type="button" class="icon-btn xs" title={t('ports.tools.unwatch')} aria-label={t('ports.tools.unwatch_port', { port: r.port })} onclick={() => unwatch(r.port)}><Glyph name="close" size={10} /></button>{:else}<span></span>{/if}
                  </span>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {/if}
  </section>

  <div class="toolbar">
    <h3>{t('ports.all.title')}</h3>
    <SearchBox value={q} placeholder={t('ports.all.search')} onsearch={(v) => (q = v)} />
    <label class="check"><input type="checkbox" checked={udp} onchange={(e) => setUdp(e.currentTarget.checked)} /> {t('ports.all.udp')}</label>
    <span class="grow"></span>
    <span class="muted small">{t('ports.all.count', { n: fmtInt(rows.length), count: rows.length })}</span>
  </div>

  {#snippet cell(r, key)}
    {#if key === 'port'}<b class="mono">{r.port}</b>
    {:else if key === 'proto'}<span class="muted">{r.proto.toUpperCase()}</span>
    {:else if key === 'known'}<span class="ellipsis" class:faint={!r.k} title={r.k ? `${r.k.tool} · ${CAT[r.k.cat]}` : ''}>{r.k?.tool || '—'}</span>
    {:else if key === 'name'}<span class="ellipsis" title={r.exe || r.name}>{r.name || '?'}</span>
    {:else if key === 'pid'}<span class="mono">{r.pid}</span>
    {:else if key === 'reach'}<span class="reach reach-{r.reach}" title={REACH[r.reach][1]}>{REACH[r.reach][0]}</span>
    {:else if key === 'conns'}<span class="mono">{r.proto === 'tcp' ? r.conns : ''}</span>
    {:else if key === 'cpu'}<span class="mono">{r.name ? fmtPct(r.cpu, r.cpu > 0 && r.cpu < 10 ? 1 : 0) : ''}</span>
    {:else if key === 'mem'}<span class="mono">{r.mem ? fmtBytes(r.mem) : ''}</span>
    {:else if key === 'addrs'}<span class="ellipsis mono muted" title={r.addrs.join(', ')}>{r.addrs.join(', ')}</span>
    {/if}
  {/snippet}

  <DataTable
    {columns}
    {rows}
    {sort}
    {onsort}
    loading={!loaded}
    storageKey="ports"
    label={t('ports.all.title')}
    rowHeight={30}
    {cell}
    onopen={(r) => showProc(r.pid)}
    onmenu={(e, r) => (menu = { x: e.clientX, y: e.clientY, r })}
    emptyText={q ? t('ports.all.no_match') : t('ports.all.empty')}
  />
</section>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} items={menuItems} label={t('ports.menu')} onpick={pickMenu} onclose={() => (menu = null)} />
{/if}
