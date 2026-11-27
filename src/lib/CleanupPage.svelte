<script>
  // Trang Dọn dẹp (bố cục bảng + ngăn chi tiết dùng chung): tệp tạm, báo cáo lỗi, cache cập nhật, Thùng rác và cache
  // ứng dụng tìm được trong AppData / thư mục ẩn của hồ sơ người dùng. Cột "Dọn" là ô tích riêng (bảng chỉ đọc:
  // bấm dòng = xem chi tiết, Space = tích). Dọn = xoá VĨNH VIỄN nội dung bên trong, tệp đang dùng tự bỏ qua.
  //
  // Hiệu năng: chỉ quét khi mở trang lần đầu / bấm Quét lại; kết quả giữ trong cleanup.svelte.js khi chuyển trang.
  import { onMount, tick } from 'svelte';
  import { store } from './store.svelte.js';
  import { ai } from './ai.svelte.js';
  import { cleanup, GROUP_LABELS, SAFETY, partsText } from './cleanup.svelte.js';
  import { procs } from './procs.svelte.js';
  import * as api from './api.js';
  import { collator, fmtBytes, fmtInt, errText } from './format.js';
  import { t } from './i18n.svelte.js';
  import { loadPref, savePref } from './prefs.svelte.js';
  import { toasts } from './toasts.svelte.js';
  import { InspState } from './inspector.svelte.js';
  import SplitView from './SplitView.svelte';
  import CleanupInspector from './CleanupInspector.svelte';
  import DataTable from './DataTable.svelte';
  import FilterTabs from './FilterTabs.svelte';
  import SearchBox from './SearchBox.svelte';
  import Glyph from './Glyph.svelte';
  import FsIcon from './FsIcon.svelte';
  import RefreshButton from './RefreshButton.svelte';
  import ContextMenu from './ContextMenu.svelte';

  const insp = new InspState('cleanup');
  let table = $state();
  let q = $state('');
  let tab = $state('all');
  let sort = $state(loadPref('cleanup:sort', { key: 'bytes', desc: true }));
  let focusId = $state(null);
  let drives = $state.raw([]);
  const drive = $derived(
    drives.find((d) => d.ready && d.letter.toUpperCase() === (cleanup.systemDrive || 'C:'))
      || (!cleanup.systemDrive && drives.find((d) => d.ready && d.kind === 'fixed'))
  );
  let menu = $state(null);

  const columns = $derived([
    { key: 'name', label: t('cleanup.col.name'), width: 250, min: 140 },
    { key: 'group', label: t('cleanup.col.group'), width: 120, min: 80 },
    { key: 'bytes', label: t('cleanup.col.bytes'), width: 170, min: 110, align: 'right' },
    { key: 'files', label: t('cleanup.col.files'), width: 80, min: 60, align: 'right' },
    { key: 'safety', label: t('cleanup.col.safety'), width: 96, min: 80 },
    { key: 'result', label: t('cleanup.col.result'), width: 150, min: 90, sortable: false },
    { key: 'path', label: t('cleanup.col.path'), width: 320, min: 120, sortable: false },
  ]);
  // t() có một tham số in đậm: trả [trước, sau] để vẽ {a}<b>{giá trị}</b>{b}
  const MARK = String.fromCharCode(1);
  const around = (key, name, params) => t(key, { ...params, [name]: MARK }).split(MARK);

  onMount(() => cleanup.ensure());
  $effect(() => {
    // Đo khi mở trang và khi lệnh dọn trả về, kể cả khi dọn từ đề xuất AI.
    cleanup.driveRevision;
    let current = true;
    api.duDrives().then((ds) => { if (current) drives = ds || []; }).catch(() => {});
    return () => { current = false; };
  });

  const vis = $derived(cleanup.visible);
  const maxBytes = $derived(vis.reduce((max, it) => Math.max(max, cleanup.freeable(it)), 1));
  // tab = nhóm của backend (temp | system | cache = ứng dụng | dev | other), cộng "Tất cả" và "Đã chọn"
  const TABS = ['temp', 'system', 'cache', 'dev', 'other'];
  const inTab = (it, t) => t === 'all' || (t === 'chosen' ? cleanup.chosen.has(it.id) : it.group === t);
  const counts = $derived({
    all: vis.length,
    ...Object.fromEntries(TABS.map((g) => [g, vis.filter((it) => it.group === g).length])),
    chosen: vis.filter((it) => cleanup.chosen.has(it.id)).length,
  });
  // tab cũ đã lưu ('sys') hoặc tab trống: về Tất cả
  $effect(() => {
    if (tab !== 'all' && tab !== 'chosen' && !counts[tab] && cleanup.scanId) tab = 'all';
  });
  const rows = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    const list = vis.filter((it) => inTab(it, tab) && (!needle || it.name.toLowerCase().includes(needle) || it.id.includes(needle) || it.paths.some((p) => p.toLowerCase().includes(needle))));
    const { key, desc } = sort;
    const dir = desc ? -1 : 1;
    const GORDER = { temp: 0, system: 1, cache: 2 };
    const num = (it) => it.pending ? -1 : key === 'bytes' ? cleanup.freeable(it) : it.files;
    const cmp =
      key === 'bytes' || key === 'files' ? (a, b) => (num(a) - num(b)) * dir || collator.compare(a.name, b.name)
      : key === 'group' ? (a, b) => (GORDER[a.group] - GORDER[b.group]) * dir || b.bytes - a.bytes
      : key === 'safety' ? (a, b) => collator.compare(a.safety, b.safety) * dir || b.bytes - a.bytes
      : (a, b) => collator.compare(a.name, b.name) * dir;
    return list.sort(cmp);
  });
  const chosen = $derived(cleanup.chosenItems);
  const chosenSize = $derived(cleanup.chosenSize);
  const totalBytes = $derived(vis.reduce((n, it) => n + it.bytes, 0));
  const target = $derived(focusId ? vis.find((it) => it.id === focusId) || null : null);
  const busy = $derived(!!cleanup.phase);

  function onsort(key) {
    sort = sort.key === key ? { key, desc: !sort.desc } : { key, desc: key === 'bytes' || key === 'files' };
    savePref('cleanup:sort', sort);
  }
  const canPick = (it) => !(it.error && !it.paths.length && it.id !== 'recycle');

  async function cleanNow() {
    const list = chosen;
    if (!list.length || busy) return;
    const recycle = list.some((it) => it.id === 'recycle');
    const ok = await store.confirm({
      title: t('cleanup.confirm.title', { count: list.length, n: fmtInt(list.length) }),
      message: [
        list.some((it) => it.pending) ? t('cleanup.confirm.size_pending') : t('cleanup.confirm.size', { size: chosenSize }),
        t(cleanup.keepRecent ? 'cleanup.confirm.skip_keep' : 'cleanup.confirm.skip'),
        recycle ? t('cleanup.confirm.recycle') : '',
      ].filter(Boolean).join(' '),
      okLabel: t('cleanup.confirm.ok'),
      danger: true,
    });
    if (ok) await cleanup.clean(list.map((it) => it.id));
  }

  async function openPath(p) {
    try { await api.duOpen(p, true); } catch (e) { toasts.error(t('cleanup.toast.open_failed'), [errText(e)]); }
  }
  function pick(it) {
    focusId = it.id;
    tick().then(() => table?.reveal(it.id));
    insp.open();
  }
  function rowMenu(e, it) {
    focusId = it.id;
    menu = { x: e.clientX, y: e.clientY, it };
  }
  const menuItems = $derived(
    menu
      ? [
          { id: 'pick', label: t(cleanup.chosen.has(menu.it.id) ? 'cleanup.menu.unpick' : 'cleanup.menu.pick'), glyph: 'check', disabled: !canPick(menu.it) },
          { id: 'open', label: t('cleanup.menu.open'), glyph: 'folder', disabled: !menu.it.paths.length },
          { sep: true },
          { id: 'ai', label: t('cleanup.menu.ask_ai'), glyph: 'chat' },
        ]
      : []
  );
  function pickMenu(id) {
    const it = menu?.it;
    menu = null;
    if (!it) return;
    if (id === 'pick') cleanup.toggle(it.id, !cleanup.chosen.has(it.id));
    else if (id === 'open') openPath(it.paths[0]);
    else if (id === 'ai') askItem(it);
  }
  function askItem(it) {
    ai.askCleanup(t('cleanup.ask.item', { name: it.name, id: it.id }));
  }
  const pathText = (it) => (it.paths.length ? it.paths[0] + (it.paths.length > 1 ? `  (+${it.paths.length - 1})` : '') : it.id === 'recycle' ? t('cleanup.path.recycle') : '–');
</script>

<SplitView {insp} hasTarget={!!target} cls="cleanup-page">
  {#snippet main()}
    <header class="page-head">
      <div>
        <h1>{t('cleanup.title')}</h1>
        <p class="muted ports-sum">
          {#if cleanup.error}
            <span class="warn-text">{cleanup.error}</span>
          {:else if !cleanup.scanId}
            {t('cleanup.sum.scanning')}
          {:else}
            {@const sf = around('cleanup.sum.found', 'bytes')}
            {@const sc = around('cleanup.sum.chosen', 'bytes', { count: chosen.length, n: fmtInt(chosen.length) })}
            {sf[0]}<b>{fmtBytes(totalBytes)}</b>{sf[1]} <span class="dot-sep">|</span>
            {sc[0]}<b class:warn-text={chosen.length > 0}>{chosenSize}</b>{sc[1]}
            {#if drive}{@const sd = around('cleanup.sum.drive', 'free', { letter: drive.letter, total: fmtBytes(drive.total) })}<span class="dot-sep">|</span> {sd[0]}<b>{fmtBytes(drive.free)}</b>{sd[1]}{/if}
          {/if}
        </p>
      </div>
      <div class="ph-actions">
        <button type="button" class="btn" onclick={() => ai.askCleanup()} disabled={!cleanup.scanId} title={t('cleanup.ask_ai_tip')}><Glyph name="chat" size={12} /> {t('cleanup.ask_ai')}</button>
        {#if insp.fits}
          <button type="button" class="btn icon-only" class:active={insp.on} aria-pressed={insp.on} onclick={() => insp.toggle()} title={insp.on ? t('cleanup.pane.hide') : t('cleanup.pane.show')} aria-label={t('cleanup.pane.label')}>
            <Glyph name={insp.on ? 'paneClose' : 'pane'} size={13} />
          </button>
        {/if}
        <RefreshButton onclick={() => cleanup.scan()} busy={cleanup.phase === 'scan'} disabled={busy} label={t('cleanup.rescan')} busyLabel={t('cleanup.rescanning')} info={cleanup.scanId ? `${(cleanup.tookMs / 1000).toFixed(1)} s` : ''} title={t('cleanup.rescan_tip')} />
        <button type="button" class="btn danger solid" onclick={cleanNow} disabled={busy || !chosen.length} title={t('cleanup.clean_tip')}>
          <Glyph name="delete" size={12} /> {cleanup.phase === 'clean' ? t('cleanup.cleaning') : chosen.length ? t('cleanup.clean_n', { count: chosen.length, n: fmtInt(chosen.length) }) : t('cleanup.clean')}
        </button>
      </div>
    </header>

    <div class="toolbar wtm-bar">
      <SearchBox value={q} placeholder={t('cleanup.search_ph')} onsearch={(v) => (q = v)} />
      <FilterTabs
        label={t('cleanup.tabs.label')}
        value={tab}
        onchange={(v) => (tab = v)}
        items={[['all', t('cleanup.tabs.all'), counts.all], ...TABS.filter((g) => counts[g]).map((g) => [g, t(`cleanup.tabs.${g}`), counts[g]]), ['chosen', t('cleanup.tabs.chosen'), counts.chosen]]}
      />
      <span class="grow"></span>
      <label class="check" title={t('cleanup.keep_recent_tip')}>
        <input type="checkbox" checked={cleanup.keepRecent} onchange={(e) => cleanup.setKeepRecent(e.currentTarget.checked)} /> {t('cleanup.keep_recent')}
      </label>
    </div>

    <p class="muted small count-line">
      {t('cleanup.count', { count: rows.length, n: rows.length })}
      {#if cleanup.pendingCount}· <span class="spin xs"></span> {t('cleanup.count.pending', { count: cleanup.pendingCount, n: fmtInt(cleanup.pendingCount) })}{/if}
      {#if !cleanup.admin}· <span class="warn-text">{t('cleanup.count.admin')}</span>{/if}
      · {t('cleanup.count.cache_hint')}
    </p>

    {#snippet cell(it, key)}
      {#if key === 'name'}
        {@const img = (procs.iconV, it.icon ? procs.icons.get(it.icon) : null)}
        <span class="cell-name ellipsis" title={it.note}>
          {#if img}<span class="app-ico"><img src={img} width="16" height="16" alt="" /></span>{:else}<FsIcon kind={it.id === 'recycle' ? 'recycle' : 'folder'} />{/if}
          <span class="ellipsis" class:muted={!canPick(it)}>{it.name}</span>
          {#if cleanup.parts.has(it.id)}<span class="faint small nowrap" title={t('cleanup.cell.partial_tip')}>{t('cleanup.cell.partial')}</span>{/if}
        </span>
      {:else if key === 'group'}
        <span class="muted ellipsis" title={it.kinds?.length ? partsText(it) : ''}>{it.kinds?.length ? partsText(it) : GROUP_LABELS[it.group]}</span>
      {:else if key === 'bytes'}
        {#if it.pending}
          <span class="faint small"><span class="spin xs"></span> {t('cleanup.cell.measuring')}</span>
        {:else if it.error && !it.paths.length && it.id !== 'recycle'}
          <span class="faint small ellipsis" title={it.error}>{t('cleanup.cell.need_admin')}</span>
        {:else}
          {@const bytes = cleanup.freeable(it)}
          <span class="size-cell" title={cleanup.retainedDetails(it).join('\n')}>
            <span class="res-bar thin"><span style="width:{(bytes / maxBytes) * 100}%"></span></span>
            <span class="mono">{fmtBytes(bytes)}</span>
          </span>
        {/if}
      {:else if key === 'files'}
        <span class="mono muted">{it.pending ? '' : fmtInt(it.files)}</span>
      {:else if key === 'safety'}
        <span class="safety {SAFETY[it.safety][1]}">{SAFETY[it.safety][0]}</span>
      {:else if key === 'result'}
        {@const r = cleanup.results.get(it.id)}
        {#if r}<span class="small ellipsis" class:warn-text={!!r.error} title={r.error || ''}>{r.skipped + r.kept_recent ? t('cleanup.cell.result_skipped', { bytes: fmtBytes(r.bytes), n: fmtInt(r.skipped + r.kept_recent) }) : t('cleanup.cell.result', { bytes: fmtBytes(r.bytes) })}</span>{/if}
      {:else if key === 'path'}
        <span class="ellipsis mono muted" title={it.paths.join('\n')}>{pathText(it)}</span>
      {/if}
    {/snippet}

    <DataTable
      bind:this={table}
      {columns}
      {rows}
      {sort}
      {onsort}
      loading={!cleanup.scanId && !cleanup.error}
      storageKey="cleanup"
      label={t('cleanup.table.label')}
      rowHeight={30}
      {cell}
      onfocus={(id) => (focusId = id)}
      checked={cleanup.chosen}
      oncheck={(ids, on) => cleanup.setMany(ids, on)}
      canCheck={(it) => !busy && canPick(it)}
      checkLabel={t('cleanup.table.check_all')}
      onopen={() => insp.open()}
      onmenu={rowMenu}
      emptyText={cleanup.error ? t('cleanup.table.scan_failed') : t('cleanup.table.empty')}
    />
  {/snippet}

  {#snippet inspector(overlay)}
    <CleanupInspector item={target} {overlay} onpick={pick} onopen={openPath} onask={askItem} onclose={() => (overlay ? insp.closeOverlay() : insp.toggle())} />
  {/snippet}
</SplitView>

{#if menu}<ContextMenu x={menu.x} y={menu.y} items={menuItems} label={t('cleanup.menu.label')} onpick={pickMenu} onclose={() => (menu = null)} />{/if}
