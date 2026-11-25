<script>
  // Trang Khởi động (bố cục kiểu RustWTM): ô tìm + tab Tất cả / Đang bật / Đã tắt, chip nguồn (chọn nhiều;
  // mặc định chỉ Registry Run + Thư mục Startup), bảng gọn + ngăn chi tiết dùng chung bên phải.
  // Chọn: bấm dòng = chọn 1 (ngăn chi tiết hiện mục đó), Ctrl/Shift/ô tích = chọn nhiều -> thanh thao tác hàng loạt.
  import { untrack, tick } from 'svelte';
  import { store, DEFAULT_KINDS } from './store.svelte.js';
  import { KIND_LABELS, KIND_ORDER, SCOPE_LABELS, collator, haystack, scopeOf } from './format.js';
  import { InspState } from './inspector.svelte.js';
  import SplitView from './SplitView.svelte';
  import ItemInspector from './ItemInspector.svelte';
  import DataTable from './DataTable.svelte';
  import FilterTabs from './FilterTabs.svelte';
  import Toggle from './Toggle.svelte';
  import RamCell from './RamCell.svelte';
  import SearchBox from './SearchBox.svelte';
  import RefreshButton from './RefreshButton.svelte';
  import BulkBar from './BulkBar.svelte';
  import Glyph from './Glyph.svelte';
  import Icon from './Icon.svelte';
  import { t } from './i18n.svelte.js';

  const ui = store.startupUi;
  const sel = store.selStartup;
  const insp = new InspState('startup');
  let table = $state();

  const columns = $derived([
    { key: 'name', label: t('startup.col.name'), width: 240, min: 120 },
    { key: 'on', label: t('startup.col.status'), width: 120, min: 100 },
    { key: 'scope', label: t('startup.col.scope'), width: 150, min: 80 },
    { key: 'source', label: t('startup.col.source'), width: 190, min: 90 },
    { key: 'publisher', label: t('startup.col.publisher'), width: 180, min: 90 },
    { key: 'ram', label: 'RAM', width: 90, min: 70, align: 'right' },
    { key: 'command', label: t('startup.col.command'), width: 360, min: 120 },
  ]);

  // "Đang chạy" dựa trên ảnh chụp RAM (không theo RAM sống để danh sách không nhảy).
  const isRunning = (it) => (store.ramSnap.get(it.id) || 0) >= 0.5;
  const kindSet = $derived(new Set(ui.kinds));
  const allKinds = $derived(ui.kinds.length === 0);
  const isDefault = $derived(ui.kinds.length === DEFAULT_KINDS.length && DEFAULT_KINDS.every((k) => kindSet.has(k)));

  // lọc chung (tìm, Microsoft, đang chạy) -> đếm theo nguồn; rồi lọc nguồn -> đếm theo tab; rồi lọc tab
  const base = $derived.by(() => {
    const q = ui.q;
    const needSnap = ui.onlyRunning;
    if (needSnap) store.snapVersion;
    return store.startup.filter((it) => (ui.showMs || !it.is_ms) && (!needSnap || isRunning(it)) && (!q || haystack(it).includes(q)));
  });
  const kindCounts = $derived.by(() => {
    const c = {};
    for (const it of base) c[it.kind] = (c[it.kind] || 0) + 1;
    return c;
  });
  const inKinds = $derived(allKinds ? base : base.filter((it) => kindSet.has(it.kind)));
  const onCount = $derived(inKinds.reduce((n, it) => n + (it.on ? 1 : 0), 0));

  const rows = $derived.by(() => {
    const list = ui.tab === 'on' ? inKinds.filter((it) => it.on) : ui.tab === 'off' ? inKinds.filter((it) => !it.on) : inKinds.slice();
    const { key, desc } = ui.sort;
    const dir = desc ? -1 : 1;
    let cmp;
    if (key === 'ram') {
      store.snapVersion;
      const snap = store.ramSnap;
      cmp = (a, b) => ((snap.get(a.id) || 0) - (snap.get(b.id) || 0)) * dir || collator.compare(a.name, b.name);
    } else if (key === 'on') {
      cmp = (a, b) => (Number(a.on) - Number(b.on)) * dir || collator.compare(a.name, b.name);
    } else if (key === 'scope') {
      cmp = (a, b) => collator.compare(SCOPE_LABELS[scopeOf(a)] || '', SCOPE_LABELS[scopeOf(b)] || '') * dir || collator.compare(a.name, b.name);
    } else {
      cmp = (a, b) => collator.compare(a[key] || '', b[key] || '') * dir || collator.compare(a.name, b.name);
    }
    return list.sort(cmp);
  });

  const selectedItems = $derived(sel.size ? store.startup.filter((it) => sel.has(it.id)) : []);
  const target = $derived(sel.size === 1 ? store.startupById.get(sel.values().next().value) || null : null);

  function onsort(key) {
    if (ui.sort.key === key) ui.sort = { key, desc: !ui.sort.desc };
    else ui.sort = { key, desc: key === 'ram' || key === 'on' };
    store.snapshotRam();
  }
  function toggleKind(k) {
    // đang "mọi nguồn" mà bấm một nguồn -> chỉ nguồn đó
    if (allKinds) ui.kinds = [k];
    else if (kindSet.has(k)) ui.kinds = ui.kinds.filter((x) => x !== k);
    else ui.kinds = [...ui.kinds, k];
  }

  // Mở trang tới một mục (từ Tổng quan…): bỏ bộ lọc đang che nó, chọn và cuộn tới.
  $effect(() => {
    const f = store.focus;
    if (!f || f.list !== 'startup' || !store.loaded) return;
    untrack(async () => {
      store.focus = null;
      const it = store.startupById.get(f.id);
      if (!it) return;
      if (!allKinds && !kindSet.has(it.kind)) ui.kinds = [...ui.kinds, it.kind];
      if (it.is_ms && !ui.showMs) ui.showMs = true;
      ui.tab = 'all';
      ui.onlyRunning = false;
      sel.clear();
      sel.add(it.id);
      await tick();
      table?.reveal(it.id);
      insp.open();
    });
  });
</script>

<SplitView {insp} hasTarget={!!target}>
  {#snippet main()}
    <header class="page-head">
      <div>
        <h1>{t('startup.title')}</h1>
        <p class="muted">{t('startup.subtitle')}</p>
      </div>
      <div class="ph-actions">
        {#if insp.fits}
          <button type="button" class="btn icon-only" class:active={insp.on} aria-pressed={insp.on} onclick={() => insp.toggle()} title={insp.on ? t('startup.pane.hide') : t('startup.pane.show')} aria-label={t('startup.pane.label')}>
            <Glyph name={insp.on ? 'paneClose' : 'pane'} size={13} />
          </button>
        {/if}
        <RefreshButton {...store.refreshProps} />
      </div>
    </header>

    <div class="toolbar wtm-bar">
      <SearchBox value={ui.q} placeholder={t('startup.search')} onsearch={(q) => (ui.q = q)} />
      <FilterTabs
        label={t('startup.tabs_label')}
        value={ui.tab}
        onchange={(v) => (ui.tab = v)}
        items={[['all', t('startup.tab.all'), inKinds.length], ['on', t('startup.tab.on'), onCount], ['off', t('startup.tab.off'), inKinds.length - onCount]]}
      />
      <span class="grow"></span>
      <label class="check"><input type="checkbox" bind:checked={ui.showMs} /> {t('startup.show_ms')}</label>
      <label class="check" title={t('startup.only_running_tip')}>
        <input type="checkbox" checked={ui.onlyRunning} onchange={(e) => { store.snapshotRam(); ui.onlyRunning = e.currentTarget.checked; }} /> {t('startup.only_running')}
      </label>
    </div>

    <div class="chips src-chips" role="group" aria-label={t('startup.chips_label')}>
      <span class="chips-lbl">{t('startup.chips')}</span>
      <button type="button" class="chip" class:active={allKinds} aria-pressed={allKinds} onclick={() => (ui.kinds = [])}>
        {t('startup.all_sources')} <span class="count">{base.length}</span>
      </button>
      {#each KIND_ORDER as k (k)}
        <button type="button" class="chip" class:active={!allKinds && kindSet.has(k)} class:empty={!kindCounts[k]} aria-pressed={!allKinds && kindSet.has(k)} onclick={() => toggleKind(k)}>
          {KIND_LABELS[k]} <span class="count">{kindCounts[k] || 0}</span>
        </button>
      {/each}
      {#if !isDefault}
        <button type="button" class="btn sm ghost" onclick={() => (ui.kinds = [...DEFAULT_KINDS])} title={t('startup.reset_tip')}>{t('startup.reset')}</button>
      {/if}
    </div>

    <p class="muted small count-line">
      {t('startup.count', { count: rows.length, on: rows.reduce((n, it) => n + (it.on ? 1 : 0), 0) })}
      {#if !allKinds}{t('startup.count_sources', { list: ui.kinds.map((k) => KIND_LABELS[k]).join(', ') })}{/if}
    </p>

    {#snippet cell(it, key)}
      {#if key === 'on'}
        <span class="status-cell">
          <Toggle on={it.on} pending={store.pending.has(it.id)} label={t('startup.toggle_label', { name: it.name })} onchange={(v) => store.toggle([it], v)} />
          <span class="small" class:muted={!it.on}>{it.on ? t('startup.status.enabled') : t('startup.status.disabled')}</span>
        </span>
      {:else if key === 'name'}
        <span class="cell-name ellipsis" title={it.name}>
          {#if it.is_ms}<span class="ms-badge" title="Microsoft"><Icon name="ms" size={11} /></span>{/if}
          <span class="ellipsis" class:off={!it.on}>{it.name}</span>
        </span>
      {:else if key === 'scope'}
        <span class="ellipsis">{SCOPE_LABELS[scopeOf(it)] || '–'}</span>
      {:else if key === 'source'}
        <span class="ellipsis" title={it.source}>{KIND_LABELS[it.kind] || it.source}</span>
      {:else if key === 'publisher'}
        <span class="ellipsis" class:muted={!it.publisher} title={it.publisher}>{it.publisher || '—'}</span>
      {:else if key === 'ram'}
        <RamCell id={it.id} shared={it.shared} />
      {:else if key === 'command'}
        <span class="ellipsis mono muted" title={it.command}>{it.command}</span>
      {/if}
    {/snippet}

    {#snippet emptyMsg()}
      {#if !store.startup.length}
        {t('startup.empty.none')}
      {:else if !allKinds && base.length > inKinds.length}
        {t('startup.empty.no_source')}
        <button type="button" class="btn sm" onclick={() => (ui.kinds = [])}>{t('startup.empty.show_all', { n: base.length })}</button>
      {:else}
        {t('startup.empty.no_match')}
      {/if}
    {/snippet}

    <DataTable
      bind:this={table}
      {columns}
      {rows}
      sort={ui.sort}
      {onsort}
      selection={sel}
      loading={!store.loaded}
      storageKey="startup2"
      label={t('startup.table_label')}
      {cell}
      empty={emptyMsg}
      onopen={() => insp.open()}
    />

    <BulkBar count={sel.size} onclear={() => sel.clear()}>
      <button type="button" class="btn success" onclick={() => store.toggle(selectedItems, true)}><Icon name="power" size={14} /> {t('startup.enable')}</button>
      <button type="button" class="btn danger" onclick={() => store.toggle(selectedItems, false)}><Icon name="power" size={14} /> {t('startup.disable')}</button>
    </BulkBar>
  {/snippet}

  {#snippet inspector(overlay)}
    <ItemInspector list="startup" item={target} selCount={sel.size} {overlay} onclose={() => (overlay ? insp.closeOverlay() : insp.toggle())} />
  {/snippet}
</SplitView>
