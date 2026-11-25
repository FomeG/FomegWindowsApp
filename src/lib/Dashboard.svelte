<script>
  import { store } from './store.svelte.js';
  import { fmtGB, fmtMB, ramClass, KIND_LABELS } from './format.js';
  import Chart from './Chart.svelte';
  import ResourceCards from './ResourceCards.svelte';
  import { sys } from './sys.svelte.js';
  import { C } from './colors.js';
  import Toggle from './Toggle.svelte';
  import RefreshButton from './RefreshButton.svelte';
  import Icon from './Icon.svelte';
  import { t } from './i18n.svelte.js';

  let range = $state(Number(localStorage.getItem('chartRange')) || 15);
  $effect(() => { try { localStorage.setItem('chartRange', String(range)); } catch {} });

  const ramTotal = $derived(sys.m?.mem.total ?? store.ramTotal);
  const gbFmt = (v) => fmtGB(v, 2);

  const st = $derived.by(() => {
    let on = 0, third = 0, thirdOn = 0;
    for (const it of store.startup) {
      if (it.on) on++;
      if (!it.is_ms) { third++; if (it.on) thirdOn++; }
    }
    return { on, total: store.startup.length, third, thirdOn };
  });
  const sv = $derived.by(() => {
    let running = 0, autoThird = 0, autoThirdRunning = 0;
    for (const s of store.services) {
      if (s.state === 'running') running++;
      if (!s.is_ms && (s.start_type === 'auto' || s.start_type === 'delayed')) {
        autoThird++;
        if (s.state === 'running') autoThirdRunning++;
      }
    }
    return { running, total: store.services.length, autoThird, autoThirdRunning };
  });

  const topMax = $derived(Math.max(1, ...store.top.map((p) => p.mb)));

  // Bên thứ ba đang bật, tốn RAM nhất (danh sách nhỏ nên đọc RAM sống là rẻ)
  const heavy = $derived.by(() => {
    const list = [];
    for (const it of store.startup) {
      if (!it.on || it.is_ms) continue;
      const mb = store.ram.get(it.id) ?? 0;
      if (mb >= 0.5) list.push({ it, mb });
    }
    list.sort((a, b) => b.mb - a.mb);
    return list.slice(0, 8);
  });

  function goStartupThird() {
    store.startupUi.showMs = false;
    store.startupUi.onlyOn = true;
    store.startupUi.kind = '';
    store.page = 'startup';
  }
  function goServicesThird() {
    store.servicesUi.chip = 'third';
    store.page = 'services';
  }
</script>

<section class="page dash">
  <header class="page-head">
    <div>
      <h1>{t('perf.dash.title')}</h1>
      <p class="muted">{t('perf.dash.sub')}</p>
    </div>
    <RefreshButton {...store.refreshProps} />
  </header>

  <ResourceCards />

  <div class="kpis">
    <button type="button" class="kpi kpi-link" onclick={goStartupThird} title={t('perf.dash.startup_tip')}>
      <div class="kpi-label"><Icon name="rocket" size={15} /> {t('perf.dash.startup_on')}</div>
      <div class="kpi-value">{#if store.loaded}{st.on} <span class="muted kpi-sub">/ {st.total}</span>{:else}<span class="sk" style="width:60px"></span>{/if}</div>
      <div class="muted small">{t('perf.dash.third_pre')} <b class="fg">{st.thirdOn}</b> {t('perf.dash.third_post', { n: st.third })}</div>
    </button>
    <button type="button" class="kpi kpi-link" onclick={() => { store.servicesUi.chip = 'running'; store.page = 'services'; }} title={t('perf.dash.svc_running_tip')}>
      <div class="kpi-label"><Icon name="gear" size={15} /> {t('perf.dash.svc_running')}</div>
      <div class="kpi-value">{#if store.loaded}{sv.running} <span class="muted kpi-sub">/ {sv.total}</span>{:else}<span class="sk" style="width:60px"></span>{/if}</div>
      <div class="muted small">{t('perf.dash.svc_total')}</div>
    </button>
    <button type="button" class="kpi kpi-link" onclick={goServicesThird} title={t('perf.dash.svc_third_tip')}>
      <div class="kpi-label"><Icon name="power" size={15} /> {t('perf.dash.svc_auto_third')}</div>
      <div class="kpi-value">{#if store.loaded}{sv.autoThird}{:else}<span class="sk" style="width:40px"></span>{/if}</div>
      <div class="muted small">{t('perf.dash.n_running', { n: sv.autoThirdRunning })}</div>
    </button>
  </div>

  <div class="card chart-card">
    <div class="card-head">
      <h3>{t('perf.dash.ram_chart')}</h3>
      <div class="seg" role="radiogroup" aria-label={t('perf.range.label')}>
        {#each [5, 15, 60] as m (m)}
          <button type="button" role="radio" aria-checked={range === m} class:active={range === m} onclick={() => (range = m)}>{t('perf.range.min', { n: m })}</button>
        {/each}
      </div>
    </div>
    <div class="dash-chart">
      <Chart t={sys.h.t} version={sys.hv} range={range * 60000} max={ramTotal || null} series={[{ data: sys.h.mem, color: C.mem, label: t('perf.mem.used_series') }]} fmt={gbFmt} label={t('perf.dash.ram_chart_label')} />
    </div>
  </div>

  <div class="dash-grid">
    <div class="card">
      <div class="card-head"><h3>{t('perf.dash.top_procs')}</h3><span class="muted small">{t('perf.dash.by_exe')}</span></div>
      {#if store.top.length}
        <ul class="bars">
          {#each store.top as p, i (i)}
            <li title={t('perf.dash.top_tip', { name: p.name, mb: fmtMB(p.mb), n: p.count })}>
              <span class="bar-name ellipsis">{p.name}{#if p.count > 1}<span class="muted"> ×{p.count}</span>{/if}</span>
              <span class="bar-track"><span class="bar-fill {ramClass(p.mb)}" style="width:{(p.mb / topMax) * 100}%"></span></span>
              <span class="bar-val {ramClass(p.mb)}">{fmtMB(p.mb)}</span>
            </li>
          {/each}
        </ul>
      {:else}
        <div class="placeholder">{#each Array(6) as _, i (i)}<span class="sk" style="width:{90 - i * 10}%"></span>{/each}</div>
      {/if}
    </div>

    <div class="card">
      <div class="card-head"><h3>{t('perf.dash.heavy')}</h3><button type="button" class="btn ghost sm" onclick={goStartupThird}>{t('perf.dash.see_all')} <Icon name="chevron" size={13} /></button></div>
      {#if !store.loaded}
        <div class="placeholder">{#each Array(6) as _, i (i)}<span class="sk" style="width:{90 - i * 8}%"></span>{/each}</div>
      {:else if heavy.length === 0}
        <p class="muted empty-note">{t('perf.dash.heavy_empty')}</p>
      {:else}
        <ul class="heavy">
          {#each heavy as { it, mb } (it.id)}
            <li>
              <Toggle on={it.on} pending={store.pending.has(it.id)} label={t('perf.dash.toggle', { name: it.name })} onchange={(v) => store.toggle([it], v)} />
              <button type="button" class="heavy-name" onclick={() => store.reveal('startup', it.id)} title={t('perf.dash.details')}>
                <span class="ellipsis">{it.name}</span>
                <span class="muted small ellipsis">{it.publisher || KIND_LABELS[it.kind]}</span>
              </button>
              <span class="bar-val {ramClass(mb)}">{fmtMB(mb)}</span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>
  </div>
</section>
