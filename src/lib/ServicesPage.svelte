<script>
  // Trang Dịch vụ (bố cục kiểu RustWTM): ô tìm + tab Tất cả / Đang chạy / Đã dừng, lọc kiểu khởi động + "chỉ bên thứ ba",
  // nút mở services.msc, bảng gọn + ngăn chi tiết dùng chung bên phải. Chọn nhiều -> thanh thao tác hàng loạt.
  import { untrack, tick } from 'svelte';
  import { store } from './store.svelte.js';
  import * as api from './api.js';
  import { toasts } from './toasts.svelte.js';
  import { collator, haystack, errText, START_ORDER, STATE_ORDER, START_LABELS, STATE_LABELS, CRITICAL } from './format.js';
  import { t } from './i18n.svelte.js';
  import { InspState } from './inspector.svelte.js';
  import SplitView from './SplitView.svelte';
  import ItemInspector from './ItemInspector.svelte';
  import DataTable from './DataTable.svelte';
  import FilterTabs from './FilterTabs.svelte';
  import RamCell from './RamCell.svelte';
  import SearchBox from './SearchBox.svelte';
  import RefreshButton from './RefreshButton.svelte';
  import BulkBar from './BulkBar.svelte';
  import ModeSelect from './ModeSelect.svelte';
  import StatePill from './StatePill.svelte';
  import Glyph from './Glyph.svelte';
  import Icon from './Icon.svelte';

  const ui = store.servicesUi;
  const sel = store.selServices;
  const insp = new InspState('services');
  let table = $state();

  const columns = $derived([
    { key: 'name', label: t('startup.svc.col.display_name'), width: 240, min: 120 },
    { key: 'key1', label: t('startup.svc.col.name'), width: 140, min: 80 },
    { key: 'pid', label: 'PID', width: 64, min: 50, align: 'right' },
    { key: 'state', label: t('startup.col.status'), width: 140, min: 110 },
    { key: 'start_type', label: t('startup.svc.col.start'), width: 150, min: 120 },
    { key: 'ram', label: 'RAM', width: 120, min: 70, align: 'right' },
    { key: 'account', label: t('startup.svc.col.account'), width: 150, min: 80 },
    { key: 'publisher', label: t('startup.col.publisher'), width: 160, min: 80 },
    { key: 'command', label: t('startup.svc.col.path'), width: 320, min: 120 },
  ]);

  const STARTS = $derived([
    ['', t('startup.svc.start_all')],
    ['auto', START_LABELS.auto],
    ['demand', START_LABELS.demand],
    ['disabled', START_LABELS.disabled],
  ]);
  const startOk = (s, f) => !f || (f === 'auto' ? s.start_type === 'auto' || s.start_type === 'delayed' : s.start_type === f);
  // Tên tài khoản gọn: "NT AUTHORITY\LocalService" -> "LocalService"
  const acct = (a) => (a ? a.slice(a.lastIndexOf('\\') + 1) : '');

  const base = $derived.by(() => {
    const q = ui.q;
    const third = ui.third;
    const f = ui.start;
    return store.services.filter((s) => (!third || !s.is_ms) && startOk(s, f) && (!q || haystack(s).includes(q) || (s.pid && String(s.pid) === q)));
  });
  const runCount = $derived(base.reduce((n, s) => n + (s.state === 'running' ? 1 : 0), 0));
  const stopCount = $derived(base.reduce((n, s) => n + (s.state === 'stopped' ? 1 : 0), 0));
  const startCounts = $derived.by(() => {
    const c = { '': 0, auto: 0, demand: 0, disabled: 0 };
    const q = ui.q;
    for (const s of store.services) {
      if ((ui.third && s.is_ms) || (q && !haystack(s).includes(q))) continue;
      c['']++;
      if (s.start_type === 'auto' || s.start_type === 'delayed') c.auto++;
      else if (s.start_type === 'demand') c.demand++;
      else if (s.start_type === 'disabled') c.disabled++;
    }
    return c;
  });

  const rows = $derived.by(() => {
    const list = ui.tab === 'running' ? base.filter((s) => s.state === 'running') : ui.tab === 'stopped' ? base.filter((s) => s.state !== 'running') : base.slice();
    const { key, desc } = ui.sort;
    const dir = desc ? -1 : 1;
    let cmp;
    if (key === 'ram') {
      store.snapVersion;
      const snap = store.ramSnap;
      cmp = (a, b) => ((snap.get(a.id) || 0) - (snap.get(b.id) || 0)) * dir || collator.compare(a.name, b.name);
    } else if (key === 'pid') {
      cmp = (a, b) => ((a.pid || 0) - (b.pid || 0)) * dir || collator.compare(a.name, b.name);
    } else if (key === 'start_type') {
      cmp = (a, b) => ((START_ORDER[a.start_type] ?? 9) - (START_ORDER[b.start_type] ?? 9)) * dir || collator.compare(a.name, b.name);
    } else if (key === 'state') {
      cmp = (a, b) => ((STATE_ORDER[a.state] ?? 9) - (STATE_ORDER[b.state] ?? 9)) * dir || collator.compare(a.name, b.name);
    } else {
      cmp = (a, b) => collator.compare(a[key] || '', b[key] || '') * dir || collator.compare(a.name, b.name);
    }
    return list.sort(cmp);
  });

  const selectedSvcs = $derived(sel.size ? store.services.filter((s) => sel.has(s.id)) : []);
  const target = $derived(sel.size === 1 ? store.servicesById.get(sel.values().next().value) || null : null);

  function onsort(key) {
    if (ui.sort.key === key) ui.sort = { key, desc: !ui.sort.desc };
    else ui.sort = { key, desc: key === 'ram' };
    store.snapshotRam();
  }
  async function openConsole() {
    try { await api.openTool('services'); } catch (e) { toasts.error(t('startup.svc.err.open_console'), [errText(e)]); }
  }

  $effect(() => {
    const f = store.focus;
    if (!f || f.list !== 'services' || !store.loaded) return;
    untrack(async () => {
      store.focus = null;
      const s = store.servicesById.get(f.id);
      if (!s) return;
      ui.tab = 'all';
      ui.start = '';
      if (s.is_ms) ui.third = false;
      sel.clear();
      sel.add(s.id);
      await tick();
      table?.reveal(s.id);
      insp.open();
    });
  });
</script>

<SplitView {insp} hasTarget={!!target}>
  {#snippet main()}
    <header class="page-head">
      <div>
        <h1>{t('startup.svc.title')}</h1>
        <p class="muted">{t('startup.svc.subtitle')}</p>
      </div>
      <div class="ph-actions">
        <button type="button" class="btn" onclick={openConsole} title={t('startup.svc.open_tip')}><Glyph name="open" size={12} /> {t('startup.svc.open')}</button>
        {#if insp.fits}
          <button type="button" class="btn icon-only" class:active={insp.on} aria-pressed={insp.on} onclick={() => insp.toggle()} title={insp.on ? t('startup.pane.hide') : t('startup.pane.show')} aria-label={t('startup.pane.label')}>
            <Glyph name={insp.on ? 'paneClose' : 'pane'} size={13} />
          </button>
        {/if}
        <RefreshButton {...store.refreshProps} />
      </div>
    </header>

    <div class="toolbar wtm-bar">
      <SearchBox value={ui.q} placeholder={t('startup.svc.search')} onsearch={(q) => (ui.q = q)} />
      <FilterTabs
        label={t('startup.tabs_label')}
        value={ui.tab}
        onchange={(v) => (ui.tab = v)}
        items={[['all', t('startup.tab.all'), base.length], ['running', STATE_LABELS.running, runCount], ['stopped', STATE_LABELS.stopped, base.length - runCount]]}
      />
      <span class="grow"></span>
      <label class="check"><input type="checkbox" bind:checked={ui.third} /> {t('startup.svc.third')}</label>
    </div>

    <div class="chips src-chips" role="radiogroup" aria-label={t('startup.svc.chips_label')}>
      <span class="chips-lbl">{t('startup.svc.chips')}</span>
      {#each STARTS as [k, label] (k)}
        <button type="button" class="chip" class:active={ui.start === k} role="radio" aria-checked={ui.start === k} onclick={() => (ui.start = k)}>
          {label} <span class="count">{startCounts[k]}</span>
        </button>
      {/each}
    </div>

    <p class="muted small count-line">{t('startup.svc.count', { shown: rows.length, total: store.services.length, stopped: stopCount })}</p>

    {#snippet cell(s, key)}
      {#if key === 'name'}
        <span class="cell-name ellipsis" title={s.description || s.name}>
          {#if s.critical}<span class="lock" title={CRITICAL.tip}><Icon name="shield" size={12} /></span>{/if}
          <span class="ellipsis">{s.name}</span>
        </span>
      {:else if key === 'key1'}
        <span class="ellipsis mono muted" title={s.key1}>{s.key1}</span>
      {:else if key === 'pid'}
        <span class="mono">{s.pid || ''}</span>
      {:else if key === 'start_type'}
        <ModeSelect svc={s} />
      {:else if key === 'state'}
        <StatePill svc={s} />
      {:else if key === 'account'}
        <span class="ellipsis muted" title={s.account}>{acct(s.account)}</span>
      {:else if key === 'publisher'}
        <span class="ellipsis" class:muted={!s.publisher} title={s.publisher}>{s.publisher || '—'}</span>
      {:else if key === 'ram'}
        <RamCell id={s.id} shared={s.shared} />
      {:else if key === 'command'}
        <span class="ellipsis mono muted" title={s.command}>{s.command}</span>
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
      storageKey="services2"
      label={t('startup.svc.table_label')}
      {cell}
      onopen={() => insp.open()}
    />

    <BulkBar count={sel.size} onclear={() => sel.clear()}>
      <button type="button" class="btn success" onclick={() => store.setMode(selectedSvcs, 'auto')}>{START_LABELS.auto}</button>
      <button type="button" class="btn" onclick={() => store.setMode(selectedSvcs, 'demand')}>{START_LABELS.demand}</button>
      <button type="button" class="btn danger" onclick={() => store.setMode(selectedSvcs, 'disabled')}>{t('startup.svc.disable_hard')}</button>
      <span class="sep"></span>
      <button type="button" class="btn" onclick={() => store.control(selectedSvcs, 'start')}><Icon name="play" size={13} /> {t('startup.svc.start')}</button>
      <button type="button" class="btn" onclick={() => store.control(selectedSvcs, 'stop')}><Icon name="stop" size={13} /> {t('startup.svc.stop')}</button>
    </BulkBar>
  {/snippet}

  {#snippet inspector(overlay)}
    <ItemInspector list="services" item={target} selCount={sel.size} {overlay} onclose={() => (overlay ? insp.closeOverlay() : insp.toggle())} />
  {/snippet}
</SplitView>
