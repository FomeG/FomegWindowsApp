<script>
  // Trang Biến môi trường (bố cục bảng + ngăn chi tiết dùng chung), như hộp thoại "Environment Variables" của Windows
  // nhưng dễ đọc hơn: Path và các biến danh sách được tách từng mục và đánh dấu mục hỏng / trùng; sửa, thêm, xoá,
  // hoàn tác ngay trong ngăn chi tiết. Trợ lý AI đọc biến bằng tool chỉ đọc (giá trị bí mật bị che) và đề xuất sửa qua
  // khối `bien_moi_truong` -> popup xem lại; không bao giờ tự sửa.
  import { onMount, tick } from 'svelte';
  import { store } from './store.svelte.js';
  import { ai } from './ai.svelte.js';
  import { env, SCOPE_LABELS } from './env.svelte.js';
  import { collator, fmtInt, copyText } from './format.js';
  import { t } from './i18n.svelte.js';
  import { loadPref, savePref } from './prefs.svelte.js';
  import { InspState } from './inspector.svelte.js';
  import SplitView from './SplitView.svelte';
  import EnvInspector from './EnvInspector.svelte';
  import DataTable from './DataTable.svelte';
  import FilterTabs from './FilterTabs.svelte';
  import SearchBox from './SearchBox.svelte';
  import Glyph from './Glyph.svelte';
  import RefreshButton from './RefreshButton.svelte';
  import ContextMenu from './ContextMenu.svelte';

  const insp = new InspState('env');
  let table = $state();
  let q = $state('');
  let tab = $state('all');
  let showExpanded = $state(loadPref('env:expanded', false));
  let sort = $state(loadPref('env:sort', { key: 'name', desc: false }));
  let focusId = $state(null);
  let menu = $state(null);

  onMount(() => env.load());
  // vừa thêm biến / mở từ đề xuất AI: chọn và cuộn tới
  $effect(() => {
    const id = env.reveal;
    if (!id || !env.vars.some((v) => v.id === id)) return;
    env.reveal = null;
    focusId = id;
    tick().then(() => table?.reveal(id));
  });

  const columns = $derived([
    { key: 'name', label: t('env.col.name'), width: 230, min: 120 },
    { key: 'scope', label: t('env.col.scope'), width: 110, min: 80 },
    { key: 'value', label: t('env.col.value'), width: 460, min: 160, sortable: false },
    { key: 'issues', label: t('env.col.status'), width: 170, min: 90 },
  ]);

  const inTab = (v, tb) => tb === 'all' || (tb === 'issues' ? v.issues > 0 : v.scope === tb);
  const counts = $derived({
    all: env.vars.length,
    user: env.vars.filter((v) => v.scope === 'user').length,
    system: env.vars.filter((v) => v.scope === 'system').length,
    issues: env.vars.filter((v) => v.issues > 0).length,
  });
  const rows = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    // tìm theo tên và giá trị (không tìm trong giá trị bí mật)
    const hit = (v) => !needle || v.name.toLowerCase().includes(needle) || (!v.secret && (v.value.toLowerCase().includes(needle) || v.expanded.toLowerCase().includes(needle)));
    const list = env.vars.filter((v) => inTab(v, tab) && hit(v));
    const dir = sort.desc ? -1 : 1;
    const cmp =
      sort.key === 'scope' ? (a, b) => collator.compare(a.scope, b.scope) * dir || collator.compare(a.name, b.name)
      : sort.key === 'issues' ? (a, b) => (a.issues - b.issues) * dir || collator.compare(a.name, b.name)
      : (a, b) => collator.compare(a.name, b.name) * dir || collator.compare(a.scope, b.scope);
    return list.sort(cmp);
  });
  const target = $derived(focusId ? env.vars.find((v) => v.id === focusId) || null : null);

  function onsort(key) {
    sort = sort.key === key ? { key, desc: !sort.desc } : { key, desc: key === 'issues' };
    savePref('env:sort', sort);
  }
  function setExpanded(on) {
    showExpanded = on;
    savePref('env:expanded', on);
  }

  function pick(v) {
    env.adding = false;
    focusId = v.id;
    tick().then(() => table?.reveal(v.id));
    insp.open();
  }
  function addNew() {
    focusId = null;
    env.adding = true;
    insp.open();
  }

  const MASK = '••••••••';
  const shown = (v) => (showExpanded ? v.expanded : v.value);
  const valueText = (v) => (v.secret ? MASK : v.list ? v.entries.map((e) => (showExpanded ? e.expanded : e.raw)).join('  ;  ') : shown(v));
  const statusParts = (v) => {
    const miss = v.entries.filter((e) => e.exists === false).length;
    const dup = v.entries.filter((e) => e.dup || e.other).length;
    return [miss ? t('env.status.missing', { count: miss, n: fmtInt(miss) }) : '', dup ? t('env.status.dup', { count: dup, n: fmtInt(dup) }) : ''].filter(Boolean);
  };

  function rowMenu(e, v) {
    focusId = v.id;
    menu = { x: e.clientX, y: e.clientY, v };
  }
  const canWrite = (v) => v.scope === 'user' || env.canSystem;
  const menuItems = $derived(
    menu
      ? [
          { id: 'edit', label: t('env.menu.edit'), glyph: 'edit', disabled: !canWrite(menu.v) },
          { id: 'copy_name', label: t('env.menu.copy_name'), glyph: 'copy' },
          { id: 'copy_value', label: t('env.menu.copy_value'), glyph: 'copy' },
          { sep: true },
          { id: 'ai', label: t('env.menu.ask_ai'), glyph: 'chat' },
          { sep: true },
          { id: 'delete', label: t('env.menu.delete'), glyph: 'delete', danger: true, disabled: menu.v.protected || !canWrite(menu.v) },
        ]
      : []
  );
  function pickMenu(id) {
    const v = menu?.v;
    menu = null;
    if (!v) return;
    if (id === 'edit') pick(v);
    else if (id === 'copy_name') copyText(v.name);
    else if (id === 'copy_value') copyText(v.value);
    else if (id === 'ai') askVar(v);
    else if (id === 'delete') remove(v);
  }
  function askVar(v) {
    ai.askEnv(t('env.ask.var', { name: v.name, scope: SCOPE_LABELS[v.scope] }));
  }
  async function remove(v) {
    const ok = await store.confirm({
      title: t('env.confirm.delete_title', { name: v.name }),
      message: t('env.confirm.delete_msg', { scope: SCOPE_LABELS[v.scope] }),
      okLabel: t('env.confirm.delete_ok'),
      danger: true,
    });
    if (!ok) return;
    try {
      await env.set(v.scope, v.name, null);
      if (focusId === v.id) focusId = null;
    } catch {}
  }
</script>

<SplitView {insp} hasTarget={!!target || env.adding} cls="env-page">
  {#snippet main()}
    <header class="page-head">
      <div>
        <h1>{t('env.title')}</h1>
        <p class="muted ports-sum">
          {#if env.error}
            <span class="warn-text">{env.error}</span>
          {:else if !env.loaded}
            {t('env.sum.loading')}
          {:else}
            <b>{fmtInt(counts.user)}</b> {t('env.sum.user')} <span class="dot-sep">|</span>
            <b>{fmtInt(counts.system)}</b> {t('env.sum.system')} <span class="dot-sep">|</span>
            {#if env.issueCount}<b class="warn-text">{fmtInt(env.issueCount)}</b> {t('env.sum.issues', { count: env.issueCount })}{:else}<span>{t('env.sum.no_issues')}</span>{/if}
          {/if}
        </p>
      </div>
      <div class="ph-actions">
        <button type="button" class="btn" onclick={() => ai.askEnv()} disabled={!env.loaded} title={t('env.ask_ai_tip')}><Glyph name="chat" size={12} /> {t('env.ask_ai')}</button>
        {#if insp.fits}
          <button type="button" class="btn icon-only" class:active={insp.on} aria-pressed={insp.on} onclick={() => insp.toggle()} title={insp.on ? t('env.pane.hide') : t('env.pane.show')} aria-label={t('env.pane.label')}>
            <Glyph name={insp.on ? 'paneClose' : 'pane'} size={13} />
          </button>
        {/if}
        <RefreshButton onclick={() => env.load()} busy={env.loading} label={t('env.refresh')} busyLabel={t('env.refreshing')} info={env.loaded ? `${env.tookMs} ms` : ''} />
        <button type="button" class="btn primary" onclick={addNew} disabled={!env.loaded}><Glyph name="add" size={12} /> {t('env.add')}</button>
      </div>
    </header>

    <div class="toolbar wtm-bar">
      <SearchBox value={q} placeholder={t('env.search_ph')} onsearch={(v) => (q = v)} />
      <FilterTabs
        label={t('env.tabs.label')}
        value={tab}
        onchange={(v) => (tab = v)}
        items={[['all', t('env.tabs.all'), counts.all], ['user', t('env.tabs.user'), counts.user], ['system', t('env.tabs.system'), counts.system], ['issues', t('env.tabs.issues'), counts.issues]]}
      />
      <span class="grow"></span>
      <label class="check" title={t('env.expanded_tip')}>
        <input type="checkbox" checked={showExpanded} onchange={(e) => setExpanded(e.currentTarget.checked)} /> {t('env.expanded')}
      </label>
    </div>

    <p class="muted small count-line">
      {t('env.count', { count: rows.length, n: fmtInt(rows.length) })}
      · {t('env.count.restart')}
      {#if env.loaded && !env.canSystem}· <span class="warn-text">{t('env.count.admin')}</span>{/if}
    </p>

    {#snippet cell(v, key)}
      {#if key === 'name'}
        <span class="cell-name ellipsis" title={v.name}>
          <Glyph name={v.secret ? 'key' : v.list ? 'bullets' : 'document'} size={13} />
          <span class="ellipsis">{v.name}</span>
          {#if v.protected}<span class="faint" title={t('env.protected_tip')}><Glyph name="lock" size={11} /></span>{/if}
        </span>
      {:else if key === 'scope'}
        <span class="muted">{SCOPE_LABELS[v.scope]}</span>
      {:else if key === 'value'}
        <span class="env-val ellipsis mono" class:muted={v.secret} title={v.secret ? t('env.secret_tip') : v.list ? v.entries.map((e) => e.raw).join('\n') : v.value + (v.expanded !== v.value ? `\n→ ${v.expanded}` : '')}>
          {#if v.list}<span class="env-n">{t('env.cell.entries', { count: v.entries.length, n: fmtInt(v.entries.length) })}</span>{/if}
          {valueText(v) || '–'}
        </span>
      {:else if key === 'issues'}
        {#if v.issues}
          <span class="safety warn ellipsis" title={statusParts(v).join(' · ')}>{statusParts(v).join(' · ')}</span>
        {:else if v.list}
          <span class="safety ok">{t('env.status.ok')}</span>
        {:else}
          <span class="faint">–</span>
        {/if}
      {/if}
    {/snippet}

    <DataTable
      bind:this={table}
      {columns}
      {rows}
      {sort}
      {onsort}
      loading={!env.loaded && !env.error}
      storageKey="env"
      label={t('env.table.label')}
      rowHeight={30}
      {cell}
      onfocus={(id) => { focusId = id; if (id) env.adding = false; }}
      onopen={() => insp.open()}
      onmenu={rowMenu}
      emptyText={env.error ? t('env.table.failed') : tab === 'issues' ? t('env.table.no_issues') : t('env.table.empty')}
    />
  {/snippet}

  {#snippet inspector(overlay)}
    <EnvInspector
      item={env.adding ? null : target}
      adding={env.adding}
      {overlay}
      onpick={pick}
      onask={askVar}
      ondelete={remove}
      oncreated={(id) => { env.adding = false; env.reveal = id; }}
      oncanceladd={() => (env.adding = false)}
      onclose={() => (overlay ? insp.closeOverlay() : insp.toggle())}
    />
  {/snippet}
</SplitView>

{#if menu}<ContextMenu x={menu.x} y={menu.y} items={menuItems} label={t('env.menu.label')} onpick={pickMenu} onclose={() => (menu = null)} />{/if}
