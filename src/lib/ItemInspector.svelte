<script>
  // Ngăn chi tiết cho trang Khởi động và Dịch vụ (cùng khung InspectorShell với trang Tiến trình).
  // - Đúng 1 mục được chọn -> chi tiết mục: bật/tắt hoặc kiểu khởi động + chạy/dừng, RAM sống + biểu đồ, vị trí, lệnh.
  // - Chưa chọn / chọn nhiều -> tổng quan danh sách (số lượng, theo nguồn / kiểu khởi động, tốn RAM nhất).
  //
  // Chỉ hiện RAM (không CPU / GPU / đĩa): mục khởi động và dịch vụ không phải lúc nào cũng chạy, và app chỉ
  // theo dõi RAM cho chúng (state.rs::matcher). Muốn xem đủ số liệu -> "Xem trong Tiến trình".
  // Biểu đồ RAM chỉ lấy mẫu khi đang xem một mục (1 lần/giây, tối đa 120 điểm, dừng khi cửa sổ ẩn).
  import { untrack } from 'svelte';
  import { store } from './store.svelte.js';
  import { procs } from './procs.svelte.js';
  import { ai } from './ai.svelte.js';
  import { C } from './colors.js';
  import { KIND_LABELS, KIND_ORDER, START_LABELS, STATE_LABELS, SCOPE_LABELS, CRITICAL, scopeOf, fmtMB, fmtBytes, fmtInt, copyText } from './format.js';
  import { toasts } from './toasts.svelte.js';
  import InspectorShell from './InspectorShell.svelte';
  import Glyph from './Glyph.svelte';
  import NavIcon from './NavIcon.svelte';
  import Chart from './Chart.svelte';
  import Toggle from './Toggle.svelte';
  import RamCell from './RamCell.svelte';
  import ModeSelect from './ModeSelect.svelte';
  import StatePill from './StatePill.svelte';
  import { t } from './i18n.svelte.js';

  let { list, item = null, selCount = 0, overlay = false, onclose } = $props();

  const isSvcList = $derived(list === 'services');
  const isSvc = $derived(!!item && (item.kind === 'Svc' || item.kind === 'Service'));
  // bản Svc tương ứng (để đổi kiểu khởi động / chạy-dừng từ tab Khởi động)
  const svc = $derived.by(() => {
    if (!item) return null;
    if (item.kind === 'Svc') return item;
    if (item.kind === 'Service') return store.services.find((s) => s.key1 === item.key1) || null;
    return null;
  });

  // ---------- icon ----------
  $effect(() => {
    const p = item?.file_path;
    if (p && !procs.icons.has(p)) procs.wantIcons([p]);
  });
  const exeIcon = $derived((procs.iconV, item?.file_path ? procs.icons.get(item.file_path) : null));

  // ---------- RAM sống + biểu đồ ----------
  const RANGE = 120000;
  const N = 120;
  const mb = $derived(item ? store.ram.get(item.id) ?? 0 : 0);
  const running = $derived(item ? (svc ? svc.state === 'running' : mb >= 0.5) : false);
  let ht = [];
  let hm = [];
  let hv = $state(0);
  // chỉ phụ thuộc id (mục được thay bằng đối tượng mới khi bật/tắt -> không làm mất biểu đồ)
  const itemId = $derived(item?.id ?? null);
  $effect(() => {
    const id = itemId;
    // lấy mẫu không được theo dõi hv / store.ram, nếu không effect tự kích hoạt lại vô hạn
    const sample = () => untrack(() => {
      if (document.hidden) return;
      ht.push(Date.now());
      hm.push((store.ram.get(id) ?? 0) * 1048576);
      if (ht.length > N) { ht.shift(); hm.shift(); }
      hv++;
    });
    ht = [];
    hm = [];
    untrack(() => hv++);
    if (!id) return;
    sample();
    const timer = setInterval(sample, 1000);
    return () => clearInterval(timer);
  });

  // ---------- vị trí theo loại ----------
  const locFields = $derived.by(() => {
    const it = item;
    if (!it) return [];
    switch (it.kind) {
      case 'Run': return [[t('startup.loc.run_key'), it.key3], [t('startup.loc.value_name'), it.key2], [t('startup.loc.approved_key'), it.key1]];
      case 'Folder': return [[t('startup.loc.folder'), it.key3], [t('startup.loc.shortcut'), it.key2], [t('startup.loc.approved_key'), it.key1]];
      case 'RegMove': return [[t('startup.loc.orig_key'), it.key1], [t('startup.loc.value_name'), it.key2], [t('startup.loc.backup_name'), it.key3]];
      case 'Task': return [[t('startup.loc.task_path'), (it.key1 || '') + (it.key2 || '')]];
      case 'Packaged': return [[t('startup.loc.startup_task_key'), it.key1], [t('startup.loc.task_name'), it.key2]];
      default: return [];
    }
  });

  /** okKey: khoá thông báo khi chép xong (startup.insp.copied_command / copied_path). */
  async function copy(text, okKey) {
    if (text && (await copyText(text))) toasts.success(t(okKey));
    else toasts.error(t('startup.insp.copy_failed'));
  }
  function askAi() {
    const it = item;
    const lines = [
      isSvc ? t('startup.ai.svc_q', { name: it.name, key: it.key1 }) : t('startup.ai.item_q', { name: it.name }),
      t('startup.ai.source', { v: it.source || KIND_LABELS[it.kind] }),
      t('startup.ai.publisher', { v: it.publisher || t('startup.ai.unknown') }),
      t('startup.ai.command', { v: it.command || t('startup.ai.unknown') }),
    ];
    if (it.description) lines.push(t('startup.ai.description', { v: it.description }));
    if (svc) lines.push(t('startup.ai.mode', { mode: START_LABELS[svc.start_type] || svc.start_type, state: STATE_LABELS[svc.state] || svc.state }));
    if (mb >= 0.5) lines.push(t('startup.ai.ram', { v: fmtMB(mb) }));
    ai.ask(lines.join('\n'));
  }
  function showProcess() {
    procs.revealPid = item.pid;
    store.page = 'processes';
  }

  // ---------- tổng quan ----------
  const all = $derived(isSvcList ? store.services : store.startup);
  const counts = $derived.by(() => {
    const c = { on: 0, run: 0, auto: 0, demand: 0, disabled: 0, other: 0, kinds: {} };
    for (const it of all) {
      if (it.on) c.on++;
      if (it.state === 'running') c.run++;
      if (isSvcList) {
        const st = it.start_type;
        if (st === 'auto' || st === 'delayed') c.auto++;
        else if (st === 'demand') c.demand++;
        else if (st === 'disabled') c.disabled++;
        else c.other++;
      } else c.kinds[it.kind] = (c.kinds[it.kind] || 0) + 1;
    }
    return c;
  });
  // tốn RAM nhất: theo ảnh chụp RAM (ổn định, không tính lại mỗi giây); ô RAM vẫn là số sống
  const heavy = $derived.by(() => {
    store.snapVersion;
    const snap = store.ramSnap;
    const pool = selCount > 1 ? all.filter((it) => sel().has(it.id)) : all;
    return pool
      .map((it) => [it, snap.get(it.id) || 0])
      .filter((x) => x[1] >= 1)
      .sort((a, b) => b[1] - a[1])
      .slice(0, 6)
      .map((x) => x[0]);
  });
  const selRam = $derived.by(() => {
    if (selCount < 2) return 0;
    let t = 0;
    const s = sel();
    for (const it of all) if (s.has(it.id)) t += store.ram.get(it.id) ?? 0;
    return t;
  });
  function sel() {
    return isSvcList ? store.selServices : store.selStartup;
  }
  function pick(it) {
    const s = sel();
    s.clear();
    s.add(it.id);
  }
  const bars = $derived(
    isSvcList
      ? [[START_LABELS.auto, counts.auto, C.cpu], [START_LABELS.demand, counts.demand, C.mem], [START_LABELS.disabled, counts.disabled, C.disk], [t('startup.ov.boot_system'), counts.other, C.gpu]]
      : KIND_ORDER.filter((k) => counts.kinds[k]).map((k) => [KIND_LABELS[k], counts.kinds[k], C.cpu])
  );
  const barMax = $derived(Math.max(1, ...bars.map((b) => b[1])));
</script>

{#if item}
  <InspectorShell
    label={isSvc ? t('startup.insp.svc_details') : t('startup.insp.item_details')}
    kicker={isSvcList ? t('startup.insp.svc_details') : t('startup.insp.item_details')}
    title={item.name}
    titleTip={item.file_path || item.name}
    {overlay}
    {onclose}
  >
    {#snippet icon()}
      {#if exeIcon}<img src={exeIcon} width="28" height="28" alt="" />{:else}<NavIcon name={isSvc ? 'services' : 'startup'} size={19} />{/if}
    {/snippet}
    {#snippet sub()}
      <span>{isSvc ? item.key1 : KIND_LABELS[item.kind] || item.kind}</span>
      {#if item.publisher}<span class="ellipsis">{item.publisher}</span>{/if}
      {#if item.is_ms}<span>Microsoft</span>{/if}
    {/snippet}
    {#snippet actions()}
      {#if item.kind !== 'Svc'}
        <span class="insp-ctl" title={t('startup.insp.with_windows')}>
          <Toggle on={item.on} pending={store.pending.has(item.id)} label={t('startup.toggle_label', { name: item.name })} onchange={(v) => store.toggle([item], v)} />
          <span class="small" class:ok={item.on} class:muted={!item.on}>{item.on ? t('startup.status.on') : t('startup.status.disabled')}</span>
        </span>
      {/if}
      {#if svc}
        <span class="insp-ctl"><ModeSelect {svc} /></span>
        <StatePill {svc} />
      {/if}
      <span class="insp-act-break"></span>
      <button type="button" class="btn sm" onclick={() => store.openLocation(item)}><Glyph name="folder" size={12} /> {t('startup.insp.open_location')}</button>
      <button type="button" class="btn sm" onclick={() => copy(item.command, 'startup.insp.copied_command')} disabled={!item.command}><Glyph name="copy" size={12} /> {t('startup.insp.copy_command')}</button>
      <button type="button" class="btn sm icon-only" onclick={askAi} title={t('startup.insp.ask_ai_tip')} aria-label={t('startup.insp.ask_ai')}><Glyph name="chat" size={12} /></button>
    {/snippet}

    <section class="insp-card">
      <div class="ic-stats two">
        <div><span>{t('startup.insp.memory')}</span><b style="color:rgb({C.mem})"><RamCell id={item.id} shared={item.shared} /></b></div>
        <div>
          <span>{t('startup.col.status')}</span>
          <b class:ok={running} class:muted={!running}>{running ? STATE_LABELS.running : t('startup.insp.not_running')}</b>
        </div>
      </div>
      <div class="ic-chart">
        <Chart t={ht} version={hv} range={RANGE} bytes minMax={1048576} series={[{ data: hm, color: C.mem, fill: true, label: 'RAM' }]} fmt={(v) => fmtBytes(v)} showMax label={t('startup.insp.chart_label')} />
      </div>
      <div class="faint small insp-note">
        {t('startup.insp.ram_only')}
        {#if item.pid && running}<button type="button" class="link" onclick={showProcess}>{t('startup.insp.show_process', { pid: item.pid })}</button>{/if}
      </div>
    </section>

    <dl class="props insp-props">
      <dt>{t('startup.col.source')}</dt><dd>{item.source || KIND_LABELS[item.kind]}</dd>
      {#if SCOPE_LABELS[scopeOf(item)]}<dt>{t('startup.col.scope')}</dt><dd>{SCOPE_LABELS[scopeOf(item)]}</dd>{/if}
      <dt>{t('startup.col.publisher')}</dt><dd>{item.publisher || '–'}</dd>
      {#if isSvc}
        <dt>{t('startup.svc.col.name')}</dt><dd class="mono">{item.key1}</dd>
        {#if svc}<dt>{t('startup.insp.mode_state')}</dt><dd>{START_LABELS[svc.start_type] || svc.start_type || '–'} · {STATE_LABELS[svc.state] || svc.state || '–'}</dd>{/if}
        {#if svc?.account}<dt>{t('startup.svc.col.account')}</dt><dd class="ellipsis" title={svc.account}>{svc.account}</dd>{/if}
        {#if item.description}<dt>{t('startup.insp.description')}</dt><dd>{item.description}</dd>{/if}
      {/if}
      {#if item.pid}<dt>PID</dt><dd class="mono">{item.pid}</dd>{/if}
      {#each locFields as [l, v] (l)}
        {#if v}<dt>{l}</dt><dd class="mono break">{v}</dd>{/if}
      {/each}
    </dl>

    {#if item.file_path}
      <div class="cmd-block">
        <div class="cmd-head">
          <span class="lbl">{t('startup.insp.file_path')}</span>
          <button type="button" class="icon-btn xs" title={t('startup.insp.copy_path')} aria-label={t('startup.insp.copy_path')} onclick={() => copy(item.file_path, 'startup.insp.copied_path')}><Glyph name="copy" size={12} /></button>
        </div>
        <div class="mono small break insp-path">{item.file_path}</div>
      </div>
    {/if}
    <div class="cmd-block">
      <div class="cmd-head">
        <span class="lbl">{t('startup.insp.full_command')}</span>
        <button type="button" class="icon-btn xs" title={t('startup.insp.copy_command')} aria-label={t('startup.insp.copy_command')} onclick={() => copy(item.command, 'startup.insp.copied_command')} disabled={!item.command}><Glyph name="copy" size={12} /></button>
      </div>
      <pre class="mono">{item.command || '–'}</pre>
    </div>
    {#if item.critical}
      <p class="note warn"><Glyph name="lock" size={13} /> {CRITICAL.tip}</p>
    {/if}
  </InspectorShell>
{:else}
  <InspectorShell
    label={isSvcList ? t('startup.ov.svc_title') : t('startup.ov.title')}
    kicker={selCount > 1 ? t('startup.ov.selected', { count: selCount }) : t('startup.ov.pick')}
    title={isSvcList ? t('startup.ov.svc_title') : t('startup.ov.title')}
    accent
    {overlay}
    {onclose}
  >
    {#snippet icon()}<NavIcon name={isSvcList ? 'services' : 'startup'} size={19} />{/snippet}
    {#snippet sub()}
      <span>{t(isSvcList ? 'startup.ov.n_services' : 'startup.ov.n_items', { count: all.length, n: fmtInt(all.length) })}</span>
      <span>{isSvcList ? t('startup.ov.n_running', { n: fmtInt(counts.run) }) : t('startup.ov.n_on', { n: fmtInt(counts.on) })}</span>
    {/snippet}

    {#if !store.loaded}
      {#each Array(3) as _, i (i)}<div class="insp-card sk-card"><span class="sk" style="width:60%"></span><span class="sk" style="width:85%"></span></div>{/each}
    {:else}
      <div class="insp-counts">
        {#if isSvcList}
          <div><b>{fmtInt(counts.run)}</b><span>{STATE_LABELS.running}</span></div>
          <div><b>{fmtInt(counts.auto)}</b><span>{START_LABELS.auto}</span></div>
          <div><b>{fmtInt(counts.disabled)}</b><span>{START_LABELS.disabled}</span></div>
        {:else}
          <div><b>{fmtInt(counts.on)}</b><span>{t('startup.status.on')}</span></div>
          <div><b>{fmtInt(all.length - counts.on)}</b><span>{t('startup.status.disabled')}</span></div>
          <div><b>{fmtInt(counts.run)}</b><span>{t('startup.ov.svc_running')}</span></div>
        {/if}
      </div>

      {#if selCount > 1}
        <section class="insp-card res">
          <div class="res-head"><Glyph name="check" size={14} /><b>{t('startup.ov.selected', { count: selCount })}</b><span class="res-val" style="color:rgb({C.mem})">{fmtMB(selRam)}</span></div>
          <div class="res-sub muted">{t('startup.ov.sel_ram_hint')}</div>
        </section>
      {/if}

      <section class="insp-card res">
        <div class="res-head"><Glyph name="list" size={14} /><b>{isSvcList ? t('startup.ov.by_start') : t('startup.ov.by_source')}</b></div>
        {#each bars as [label, n, col] (label)}
          <div class="res-row"><span class="ellipsis">{label}</span><span class="mono muted">{fmtInt(n)}</span></div>
          <div class="res-bar thin"><span style="width:{(n / barMax) * 100}%; background:rgb({col})"></span></div>
        {/each}
      </section>

      <section class="insp-card res">
        <div class="res-head"><Glyph name="memory" size={14} /><b>{selCount > 1 ? t('startup.ov.heaviest_sel') : t('startup.ov.heaviest')}</b></div>
        {#each heavy as it (it.id)}
          <button type="button" class="res-row link-row" onclick={() => pick(it)} title={t('startup.ov.view_details')}>
            <span class="ellipsis">{it.name}</span><span class="mono muted"><RamCell id={it.id} /></span>
          </button>
        {:else}
          <div class="res-sub muted">{t('startup.ov.none_running')}</div>
        {/each}
      </section>
      <div class="faint small">{t('startup.ov.hint')}</div>
    {/if}
  </InspectorShell>
{/if}
