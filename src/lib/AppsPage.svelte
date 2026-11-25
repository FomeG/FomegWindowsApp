<script>
  // Trang Ứng dụng (Hệ thống → Ứng dụng): như Cài đặt → Ứng dụng → Ứng dụng đã cài của Windows, theo bố cục bảng +
  // ngăn chi tiết dùng chung. Mỗi dòng có nút "…" (Sửa đổi / Gỡ cài đặt…) giống Windows; chuột phải cũng ra menu đó.
  // Gỡ cài đặt mở popup ActionReview (loại go_cai) liệt kê thư mục liên quan để chọn dọn sau khi gỡ.
  // Dung lượng: đo thư mục cài đặt thật ở nền (lớn trước), chưa đo xong thì hiện số trình cài khai báo (≈).
  // Tab "Còn sót lại": thư mục của app đã gỡ, dữ liệu app Store mồ côi, WindowsApps (cột tích riêng của DataTable).
  //
  // Hiệu năng: đọc danh sách khi mở trang lần đầu / bấm Làm mới; quét phần còn sót chỉ khi mở tab đó; rời trang thì dừng đo.
  import { onMount, onDestroy, tick, untrack } from 'svelte';
  import { ai } from './ai.svelte.js';
  import { apps, leftId, isStale, ago, isStoreArea, KIND_LABELS } from './apps.svelte.js';
  import { procs } from './procs.svelte.js';
  import * as api from './api.js';
  import { collator, fmtBytes, fmtInt, copyText, errText } from './format.js';
  import { loadPref, savePref } from './prefs.svelte.js';
  import { InspState } from './inspector.svelte.js';
  import { toasts } from './toasts.svelte.js';
  import { t, i18n } from './i18n.svelte.js';
  import SplitView from './SplitView.svelte';
  import AppInspector from './AppInspector.svelte';
  import LeftoverInspector from './LeftoverInspector.svelte';
  import DataTable from './DataTable.svelte';
  import FilterTabs from './FilterTabs.svelte';
  import SearchBox from './SearchBox.svelte';
  import Glyph from './Glyph.svelte';
  import FsIcon from './FsIcon.svelte';
  import RefreshButton from './RefreshButton.svelte';
  import ContextMenu from './ContextMenu.svelte';

  const insp = new InspState('apps');
  const RECENT_MS = 30 * 864e5;
  let table = $state();
  let q = $state('');
  let tab = $state('all');
  let sort = $state(loadPref('apps:sort', { key: 'size', desc: true }));
  let lsort = $state(loadPref('leftovers:sort', { key: 'bytes', desc: true }));
  let focusId = $state(null);
  let leftFocus = $state(null);
  let menu = $state(null);
  const isLeft = $derived(tab === 'left');

  const columns = $derived([
    { key: 'name', label: t('apps.col.name'), width: 250, min: 150 },
    { key: 'publisher', label: t('apps.col.publisher'), width: 150, min: 80 },
    { key: 'version', label: t('apps.col.version'), width: 100, min: 60 },
    { key: 'installed', label: t('apps.col.installed'), width: 88, min: 76, align: 'right' },
    { key: 'used', label: t('apps.col.used'), width: 110, min: 80, align: 'right' },
    { key: 'size', label: t('apps.col.size'), width: 140, min: 100, align: 'right' },
    { key: 'kind', label: t('apps.col.kind'), width: 110, min: 70 },
    { key: 'more', label: '', width: 36, min: 36, sortable: false },
  ]);
  const leftColumns = $derived([
    { key: 'path', label: t('apps.left.col.path'), width: 280, min: 160 },
    { key: 'owner', label: t('apps.left.col.owner'), width: 130, min: 80 },
    { key: 'area', label: t('apps.left.col.area'), width: 120, min: 80 },
    { key: 'bytes', label: t('apps.col.size'), width: 120, min: 100, align: 'right' },
    { key: 'newest', label: t('apps.left.col.newest'), width: 100, min: 80, align: 'right' },
    { key: 'reason', label: t('apps.left.col.reason'), width: 300, min: 120, sortable: false },
  ]);

  onMount(() => apps.ensure());
  onDestroy(() => apps.pause());

  // mở tab "Còn sót lại" lần đầu (hoặc sau khi vừa gỡ một app) -> quét
  $effect(() => {
    if (isLeft && apps.left === null && !apps.leftLoading && !apps.leftError) untrack(() => apps.scanLeft());
  });

  const fmtDay = (ms) => (ms ? new Date(ms).toLocaleDateString(i18n.intl, { day: '2-digit', month: '2-digit', year: 'numeric' }) : '–');
  const now = Date.now();
  const inTab = (a, t) => t === 'all' || (t === 'recent' ? a.installed > now - RECENT_MS : t === 'stale' ? isStale(a, apps.usageFull, now) : a.kind === t);
  const counts = $derived({
    all: apps.list.length,
    win32: apps.list.filter((a) => a.kind === 'win32').length,
    store: apps.list.filter((a) => a.kind === 'store').length,
    recent: apps.list.filter((a) => a.installed > now - RECENT_MS).length,
    stale: apps.list.filter((a) => isStale(a, apps.usageFull, now)).length,
  });
  const sizeOf = (a) => apps.size(a)?.bytes ?? -1;
  const rows = $derived.by(() => {
    apps.sizeV;
    if (isLeft) return [];
    const needle = q.trim().toLowerCase();
    const list = apps.list.filter((a) => inTab(a, tab) && (!needle || a.name.toLowerCase().includes(needle) || a.publisher.toLowerCase().includes(needle)));
    const { key, desc } = sort;
    const dir = desc ? -1 : 1;
    const by = (f) => (a, b) => (f(a) - f(b)) * dir || collator.compare(a.name, b.name);
    const cmp =
      key === 'size' ? by(sizeOf)
      : key === 'installed' ? by((a) => a.installed)
      : key === 'used' ? by((a) => a.last_used)
      : key === 'kind' ? (a, b) => collator.compare(a.kind, b.kind) * dir || collator.compare(a.name, b.name)
      : (a, b) => collator.compare(a[key] || '', b[key] || '') * dir || collator.compare(a.name, b.name);
    return list.sort(cmp);
  });
  const leftRows = $derived.by(() => {
    if (!isLeft || !apps.left) return [];
    const needle = q.trim().toLowerCase();
    const list = apps.left.map((f) => ({ ...f, id: leftId(f) })).filter((f) => !needle || f.path.toLowerCase().includes(needle) || f.owner.toLowerCase().includes(needle));
    const { key, desc } = lsort;
    const dir = desc ? -1 : 1;
    const cmp =
      key === 'bytes' || key === 'newest' ? (a, b) => (a[key] - b[key]) * dir || collator.compare(a.path, b.path)
      : (a, b) => collator.compare(a[key] || '', b[key] || '') * dir || b.bytes - a.bytes;
    return list.sort(cmp);
  });
  const maxBytes = $derived((apps.sizeV, Math.max(1, ...apps.list.map(sizeOf))));
  const leftMax = $derived(Math.max(1, ...(apps.left || []).map((f) => f.bytes)));
  const totalBytes = $derived((apps.sizeV, apps.list.reduce((n, a) => n + Math.max(0, sizeOf(a)), 0)));
  const leftChosen = $derived((apps.left || []).filter((f) => f.kind !== 'info' && apps.leftChosen.has(leftId(f))));
  const leftChosenBytes = $derived(leftChosen.reduce((n, f) => n + f.bytes, 0));
  const leftTotal = $derived((apps.left || []).filter((f) => f.kind !== 'info').reduce((n, f) => n + f.bytes, 0));
  const target = $derived(focusId ? apps.list.find((a) => a.id === focusId) || null : null);
  const leftTarget = $derived(leftFocus ? (apps.left || []).find((f) => leftId(f) === leftFocus) || null : null);

  function onsort(key) {
    sort = sort.key === key ? { key, desc: !sort.desc } : { key, desc: key === 'size' || key === 'installed' || key === 'used' };
    savePref('apps:sort', sort);
  }
  function onlsort(key) {
    lsort = lsort.key === key ? { key, desc: !lsort.desc } : { key, desc: key === 'bytes' || key === 'newest' };
    savePref('leftovers:sort', lsort);
  }
  function pick(a) {
    focusId = a.id;
    tick().then(() => table?.reveal(a.id));
    insp.open();
  }
  function pickLeft(f) {
    leftFocus = leftId(f);
    tick().then(() => table?.reveal(leftFocus));
    insp.open();
  }
  function rowMenu(e, a) {
    focusId = a.id;
    menu = { x: e.clientX, y: e.clientY, a };
  }
  function leftMenu(e, f) {
    leftFocus = f.id;
    menu = { x: e.clientX, y: e.clientY, f };
  }
  function moreMenu(e, a) {
    e.stopPropagation();
    const r = e.currentTarget.getBoundingClientRect();
    focusId = a.id;
    menu = { x: r.right - 4, y: r.bottom + 2, a };
  }
  const menuItems = $derived(
    menu?.a
      ? [
          { id: 'modify', label: t('apps.act.modify'), glyph: 'edit', disabled: !menu.a.can_modify },
          { id: 'uninstall', label: apps.removing === menu.a.id ? t('apps.act.uninstalling') : t('apps.act.uninstall'), glyph: 'delete', danger: true, disabled: apps.removing === menu.a.id },
          { sep: true },
          { id: 'folder', label: t('apps.act.open_install_folder'), glyph: 'folder', disabled: !menu.a.location },
          ...(menu.a.kind === 'win32' ? [{ id: 'registry', label: t('apps.act.open_registry'), glyph: 'open' }] : []),
          { id: 'copy', label: t('apps.act.copy_uninstall'), glyph: 'copy' },
          { sep: true },
          { id: 'ai', label: t('apps.act.ask_ai_app'), glyph: 'chat' },
        ]
      : menu?.f
        ? [
            { id: 'pick', label: apps.leftChosen.has(leftId(menu.f)) ? t('apps.left.act.unpick') : t('apps.left.act.pick'), glyph: 'check', disabled: menu.f.kind === 'info' },
            { id: 'lopen', label: t('apps.act.open_folder'), glyph: 'folder' },
            { id: 'forget', label: t('apps.left.act.forget', { name: menu.f.owner }), glyph: 'close', disabled: isStoreArea(menu.f) },
            { sep: true },
            { id: 'lai', label: t('apps.left.act.ask_ai'), glyph: 'chat' },
          ]
        : []
  );
  function pickMenu(id) {
    const { a, f } = menu || {};
    menu = null;
    if (a) {
      if (id === 'modify') apps.modify(a);
      else if (id === 'uninstall') apps.uninstall(a);
      else if (id === 'folder') apps.open(a, 'folder');
      else if (id === 'registry') apps.open(a, 'registry');
      else if (id === 'copy') copyText(a.command).then((ok) => (ok ? toasts.success(t('apps.toast.copied_uninstall')) : toasts.error(t('apps.toast.copy_failed'))));
      else if (id === 'ai') askApp(a);
    } else if (f) {
      const k = leftId(f);
      if (id === 'pick') apps.leftChosen.has(k) ? apps.leftChosen.delete(k) : apps.leftChosen.add(k);
      else if (id === 'lopen') openPath(f.path);
      else if (id === 'forget') apps.forget(f.owner);
      else if (id === 'lai') askLeft(f);
    }
  }
  async function openPath(p) {
    try { await api.duOpen(p, true); } catch (e) { toasts.error(t('apps.toast.open_folder_failed'), [errText(e)]); }
  }
  function askApp(a) {
    ai.askApps(a.publisher ? t('apps.ai.ask_app_pub', { name: a.name, publisher: a.publisher }) : t('apps.ai.ask_app', { name: a.name }));
  }
  function askLeft(f) {
    ai.askApps(t('apps.ai.ask_left', { path: f.path, area: f.area, owner: f.owner, reason: f.reason }));
  }
</script>

<SplitView {insp} hasTarget={isLeft ? !!leftTarget : !!target} cls="apps-page">
  {#snippet main()}
    <header class="page-head">
      <div>
        <h1>{t('apps.title')}</h1>
        <p class="muted ports-sum">
          {#if apps.error}
            <span class="warn-text">{apps.error}</span>
          {:else if !apps.loaded}
            {t('apps.reading')}
          {:else if isLeft && apps.left}
            <b>{fmtBytes(leftTotal)}</b> {t('apps.left.sum.left')} <span class="dot-sep">|</span>
            <b class:warn-text={leftChosen.length > 0}>{fmtBytes(leftChosenBytes)}</b> {t('apps.left.sum.chosen', { n: fmtInt(leftChosen.length), count: leftChosen.length })}
          {:else}
            <b>{fmtInt(apps.list.length)}</b> {t('apps.sum.apps', { count: apps.list.length })} <span class="dot-sep">|</span>
            <b>{fmtBytes(totalBytes)}</b> {apps.measuring ? t('apps.sum.measured') : t('apps.sum.total')}
            <span class="dot-sep">|</span> <b>{fmtInt(counts.recent)}</b> {t('apps.sum.recent')}
          {/if}
        </p>
      </div>
      <div class="ph-actions">
        <button type="button" class="btn" onclick={() => (isLeft ? ai.askApps(t('apps.ai.ask_leftovers')) : ai.askApps())} disabled={!apps.loaded} title={t('apps.ai.ask_tip')}><Glyph name="chat" size={12} /> {t('apps.ai.ask')}</button>
        {#if insp.fits}
          <button type="button" class="btn icon-only" class:active={insp.on} aria-pressed={insp.on} onclick={() => insp.toggle()} title={insp.on ? t('apps.pane.hide') : t('apps.pane.show')} aria-label={t('apps.pane.label')}>
            <Glyph name={insp.on ? 'paneClose' : 'pane'} size={13} />
          </button>
        {/if}
        {#if isLeft}
          <RefreshButton onclick={() => apps.scanLeft()} busy={apps.leftLoading} disabled={apps.leftBusy} label={t('apps.left.rescan')} busyLabel={t('apps.left.scanning')} info={apps.left ? `${(apps.leftInfo.took_ms / 1000).toFixed(1)} s` : ''} title={t('apps.left.rescan_tip')} />
          <button type="button" class="btn danger solid" onclick={() => apps.cleanLeft()} disabled={apps.leftBusy || !leftChosen.length} title={t('apps.left.clean_tip')}>
            <Glyph name="delete" size={12} /> {apps.leftBusy ? t('apps.left.cleaning') : leftChosen.length ? t('apps.left.clean_n', { n: fmtInt(leftChosen.length), count: leftChosen.length }) : t('apps.left.clean')}
          </button>
        {:else}
          <RefreshButton onclick={() => apps.load({ force: true })} busy={apps.loading} label={t('apps.refresh')} busyLabel={t('apps.refreshing')} info={apps.loaded ? `${(apps.tookMs / 1000).toFixed(1)} s` : ''} title={t('apps.refresh_tip')} />
        {/if}
      </div>
    </header>

    <div class="toolbar wtm-bar">
      <SearchBox value={q} placeholder={isLeft ? t('apps.left.search') : t('apps.search')} onsearch={(v) => (q = v)} />
      <FilterTabs
        label={t('apps.tabs.label')}
        value={tab}
        onchange={(v) => (tab = v)}
        items={[['all', t('apps.tab.all'), counts.all], ['win32', KIND_LABELS.win32, counts.win32], ['store', KIND_LABELS.store, counts.store], ['recent', t('apps.tab.recent'), counts.recent], ['stale', t('apps.tab.stale'), counts.stale], ['left', t('apps.tab.left'), apps.left ? apps.left.length : null]]}
      />
    </div>

    {#if isLeft}
      <p class="muted small count-line">
        {t('apps.left.count', { n: fmtInt(leftRows.length), count: leftRows.length })}
        {#if apps.leftLoading}· <span class="spin xs"></span> {t('apps.left.searching')}{/if}
        {#if apps.left}· {t('apps.left.remembered', { n: fmtInt(apps.leftInfo.removed_known), count: apps.leftInfo.removed_known })}{/if}
        {#if apps.leftInfo.windowsapps_error}· <span class="warn-text" title={apps.leftInfo.windowsapps_error}>{t('apps.left.windowsapps_error')}</span>{/if}
        · {t('apps.left.to_recycle')}
      </p>
    {:else}
      <p class="muted small count-line">
        {t('apps.count', { n: fmtInt(rows.length), count: rows.length })}
        {#if apps.measuring}· <span class="spin xs"></span> {t('apps.measuring', { n: fmtInt(apps.measuring) })}{/if}
        {#if apps.removing}· <span class="warn-text"><span class="spin xs"></span> {t('apps.waiting_uninstaller', { name: apps.list.find((a) => a.id === apps.removing)?.name || '' })}</span> <button type="button" class="link-btn small" onclick={() => api.appsUninstallStop()} title={t('apps.stop_waiting_tip')}>{t('apps.stop_waiting')}</button>{/if}
        {#if apps.storeError}· <span class="warn-text" title={apps.storeError}>{t('apps.store_error')}</span>{/if}
        · {t('apps.size_hint')}
      </p>
    {/if}

    {#snippet cell(a, key)}
      {#if key === 'name'}
        {@const img = (procs.iconV, a.icon ? procs.icons.get(a.icon) : null)}
        <span class="cell-name ellipsis" title={a.name}>
          <span class="app-ico">{#if img}<img src={img} width="18" height="18" alt="" />{:else}<Glyph name={a.kind === 'store' ? 'store' : 'app'} size={14} />{/if}</span>
          <span class="ellipsis">{a.name}</span>
          {#if apps.removing === a.id}<span class="pill st-pending app-pill"><span class="spin xs"></span> {t('apps.pill.uninstalling')}</span>{/if}
        </span>
      {:else if key === 'publisher'}
        <span class="ellipsis muted" title={a.publisher}>{a.publisher || '–'}</span>
      {:else if key === 'version'}
        <span class="ellipsis mono muted" title={a.version}>{a.version || '–'}</span>
      {:else if key === 'installed'}
        <span class="mono muted">{fmtDay(a.installed)}</span>
      {:else if key === 'used'}
        {#if a.last_used}
          <span class="muted small" class:warn-text={isStale(a, apps.usageFull, now)} title="{fmtDay(a.last_used)}{a.launches ? ` · ${t('apps.used.launches_tip', { n: fmtInt(a.launches), count: a.launches })}` : ''}">{ago(a.last_used, now)}</span>
        {:else}
          <span class="faint small" title={apps.usageFull ? t('apps.used.none_tip') : t('apps.used.need_admin')}>{apps.usageFull ? t('apps.used.none') : '–'}</span>
        {/if}
      {:else if key === 'size'}
        {@const sz = (apps.sizeV, apps.size(a))}
        {#if sz}
          <span class="size-cell" title={sz.measured ? (sz.partial ? t('apps.size.measured_partial_tip', { n: fmtInt(sz.files), count: sz.files }) : t('apps.size.measured_tip', { n: fmtInt(sz.files), count: sz.files })) : t('apps.size.estimated_tip')}>
            <span class="res-bar thin"><span style="width:{(sz.bytes / maxBytes) * 100}%"></span></span>
            <span class="mono" class:muted={!sz.measured}>{sz.measured ? (sz.partial ? '≥ ' : '') : '≈ '}{fmtBytes(sz.bytes)}</span>
          </span>
        {:else if apps.measuring && a.location}
          <span class="faint small"><span class="spin xs"></span> {t('apps.size.measuring')}</span>
        {:else}
          <span class="faint">–</span>
        {/if}
      {:else if key === 'kind'}
        <span class="muted ellipsis">{KIND_LABELS[a.kind]}{a.arch ? ` · ${a.arch}` : ''}</span>
      {:else if key === 'more'}
        <button type="button" class="icon-btn xs app-more" title={t('apps.more')} aria-label={t('apps.more_for', { name: a.name })} onclick={(e) => moreMenu(e, a)}><Glyph name="more" size={13} /></button>
      {/if}
    {/snippet}

    {#snippet leftCell(f, key)}
      {#if key === 'path'}
        <span class="cell-name ellipsis" title={f.path}>
          <FsIcon kind="folder" />
          <span class="ellipsis mono small" class:muted={f.kind === 'info'}>{f.path}</span>
        </span>
      {:else if key === 'owner'}
        <span class="ellipsis" title={f.owner}>{f.owner}</span>
      {:else if key === 'area'}
        <span class="muted ellipsis">{f.area}</span>
      {:else if key === 'bytes'}
        <span class="size-cell">
          <span class="res-bar thin"><span style="width:{(f.bytes / leftMax) * 100}%"></span></span>
          <span class="mono">{f.partial ? '≥ ' : ''}{fmtBytes(f.bytes)}</span>
        </span>
      {:else if key === 'newest'}
        <span class="mono muted">{fmtDay(f.newest)}</span>
      {:else if key === 'reason'}
        <span class="ellipsis small" class:muted={f.kind !== 'info'} class:faint={f.kind === 'info'} title={f.reason}>{f.kind === 'info' ? t('apps.left.prefix.info') : f.kind === 'package' ? t('apps.left.prefix.package') : ''}{f.reason}</span>
      {/if}
    {/snippet}

    {#if isLeft}
      <DataTable
        bind:this={table}
        columns={leftColumns}
        rows={leftRows}
        sort={lsort}
        onsort={onlsort}
        loading={apps.leftLoading && !apps.left}
        storageKey="leftovers"
        label={t('apps.left.table_label')}
        rowHeight={30}
        cell={leftCell}
        onfocus={(id) => (leftFocus = id)}
        checked={apps.leftChosen}
        oncheck={(ids, on) => { for (const id of ids) on ? apps.leftChosen.add(id) : apps.leftChosen.delete(id); }}
        canCheck={(f) => !apps.leftBusy && f.kind !== 'info'}
        checkLabel={t('apps.left.check_all')}
        onopen={() => insp.open()}
        onmenu={leftMenu}
        emptyText={apps.leftError || t('apps.left.empty')}
      />
    {:else}
      <DataTable
        bind:this={table}
        {columns}
        {rows}
        {sort}
        {onsort}
        loading={!apps.loaded && !apps.error}
        storageKey="apps"
        label={t('apps.table_label')}
        rowHeight={32}
        {cell}
        onfocus={(id) => (focusId = id)}
        onopen={() => insp.open()}
        onmenu={rowMenu}
        emptyText={apps.error ? t('apps.empty_error') : t('apps.empty')}
      />
    {/if}
  {/snippet}

  {#snippet inspector(overlay)}
    {#if isLeft}
      <LeftoverInspector item={leftTarget} {overlay} onpick={pickLeft} onopen={openPath} onask={askLeft} onclose={() => (overlay ? insp.closeOverlay() : insp.toggle())} />
    {:else}
      <AppInspector app={target} {overlay} onpick={pick} onask={askApp} onclose={() => (overlay ? insp.closeOverlay() : insp.toggle())} />
    {/if}
  {/snippet}
</SplitView>

{#if menu}<ContextMenu x={menu.x} y={menu.y} items={menuItems} label={menu.a ? t('apps.menu.app') : t('apps.menu.left')} onpick={pickMenu} onclose={() => (menu = null)} />{/if}
