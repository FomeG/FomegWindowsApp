<script>
  // Ngăn chi tiết bên phải trang Tiến trình (kiểu "adaptive inspector" của RustWTM):
  // - chưa chọn gì / chọn nhiều -> Tổng quan hệ thống (CPU, bộ nhớ, đĩa, GPU, mạng, số tiến trình…)
  // - chọn 1 hàng (tiến trình hoặc nhóm) -> chi tiết + biểu đồ 2 phút + thao tác nhanh.
  // Số liệu đọc theo procs.version / sys.m; lịch sử tiến trình lấy từ vòng đệm trong procs.svelte.js.
  import { procs } from './procs.svelte.js';
  import { sys } from './sys.svelte.js';
  import { ai } from './ai.svelte.js';
  import { C } from './colors.js';
  import { fmtPct, fmtBytes, fmtRate, fmtInt, fmtDuration, fmtGB, fmtGHz, copyText, memModuleLines } from './format.js';
  import { toasts } from './toasts.svelte.js';
  import { t, labels, i18n } from './i18n.svelte.js';
  import Glyph from './Glyph.svelte';
  import Chart from './Chart.svelte';
  import InspectorShell from './InspectorShell.svelte';
  import NavIcon from './NavIcon.svelte';

  let { target = null, selCount = 0, overlay = false, onaction, onmore, onclose } = $props();

  const RANGE = 120000;
  const KIND = labels({ app: 'procs.kind.app', background: 'procs.kind.background', windows: 'procs.kind.windows' });

  // ---------- chế độ tiến trình ----------
  const isGroup = $derived(target?.type === 'group');
  const pids = $derived(target ? (isGroup ? target.pids : [target.pid]) : []);
  const p = $derived.by(() => {
    procs.version;
    return target ? procs.map.get(target.pid) || null : null;
  });
  const live = $derived.by(() => {
    procs.version;
    if (!target) return null;
    let cpu = 0, mem = 0, disk = 0, gpu = 0, thr = 0, hnd = 0, n = 0;
    for (const pid of pids) {
      const o = procs.map.get(pid);
      if (!o) continue;
      n++; cpu += o.cpu; mem += o.mem; disk += o.disk; gpu += o.gpu; thr += o.thr; hnd += o.hnd;
    }
    return n ? { cpu, mem, disk, gpu: Math.min(100, gpu), thr, hnd, n } : null;
  });
  // lịch sử: dựng lại mỗi tick (≤ 240 điểm × số pid) – rẻ
  let seq = 0; // biến thường: tăng mỗi lần dựng lại để Chart biết cần vẽ lại
  const hist = $derived.by(() => {
    procs.version;
    return { ...procs.history(pids), v: ++seq };
  });
  const parent = $derived.by(() => {
    procs.version;
    return p ? procs.map.get(p.ppid) || null : null;
  });
  const children = $derived.by(() => {
    procs.version;
    if (!p) return 0;
    let n = 0;
    for (const c of procs.map.values()) if (c.ppid === p.pid && c.pid !== p.pid) n++;
    return n;
  });
  const exeIcon = $derived((procs.iconV, p?.exe ? procs.icons.get(p.exe) : null));
  const winTitle = $derived((procs.version, target ? procs.windows.get(target.pid) || target.title : ''));
  const kindLabel = $derived(p ? KIND[procs.windows.has(p.pid) ? 'app' : p.kind] || '' : '');
  // nhãn biểu đồ: đổi theo ngôn ngữ, không tạo lại mỗi tick
  const L = $derived({ mem: t('procs.insp.mem'), disk: t('procs.insp.disk') });

  async function copy(text, key) {
    if (text && (await copyText(text))) toasts.success(t(key));
    else toasts.error(t('procs.toast.copy_fail'));
  }

  const pctFmt = (v) => fmtPct(v, v > 0 && v < 10 ? 1 : 0);
  const bytesFmt = (v) => fmtBytes(v);
  const rateFmt = (v) => fmtRate(v);

  // ---------- chế độ tổng quan ----------
  const m = $derived(sys.m);
  const info = $derived(sys.info);
  const memPct = $derived(m ? (m.mem.used / m.mem.total) * 100 : 0);
  const net = $derived.by(() => {
    let rx = 0, tx = 0;
    for (const n of m?.nets || []) { rx += n.rx; tx += n.tx; }
    return { rx, tx };
  });
  const ramLines = $derived(memModuleLines(info));
  const diskInfo = (id) => info?.disks?.find((d) => d.id === id);
  const gpuMax = $derived(m?.gpus?.length ? Math.max(...m.gpus.map((g) => g.util)) : 0);

  function summary() {
    if (!m) return '';
    const L = [];
    L.push(t('procs.sum.machine', { host: info?.hostname || '', os: info?.os || '' }).trim());
    L.push(t('procs.sum.cpu', { name: info?.cpu_name || '', cores: info?.cores ?? '?', logical: info?.logical ?? m.cpu.cores.length, pct: fmtPct(m.cpu.total) }));
    L.push(t('procs.sum.ram', { used: fmtGB(m.mem.used), total: fmtGB(m.mem.total), pct: fmtPct(memPct), avail: fmtGB(m.mem.avail) }));
    for (const l of ramLines) L.push(`  - ${l}`);
    if (info?.board) L.push(t('procs.sum.board', { v: info.board }));
    for (const d of m.disks || []) {
      const di = diskInfo(d.id);
      L.push(t('procs.sum.disk', { name: `${d.name}${di?.model ? ' – ' + di.model : ''}`, active: fmtPct(d.active), read: fmtRate(d.read), write: fmtRate(d.write) }));
    }
    for (const g of m.gpus || []) L.push(`GPU: ${g.name} – ${fmtPct(g.util)}`);
    L.push(t('procs.sum.net', { rx: fmtRate(net.rx), tx: fmtRate(net.tx) }));
    if (m.counts) L.push(t('procs.sum.counts', { p: fmtInt(m.counts.processes), t: fmtInt(m.counts.threads), h: fmtInt(m.counts.handles) }));
    L.push(t('procs.sum.uptime', { v: fmtDuration(m.uptime_s) }));
    return L.join('\n');
  }
  function askAiMachine() {
    ai.ask(t('procs.ai.machine') + '\n\n' + summary());
  }
</script>

{#if target}
  <!-- ================= CHI TIẾT TIẾN TRÌNH ================= -->
  <InspectorShell label={t('procs.insp.kicker')} kicker={t('procs.insp.kicker')} title={isGroup ? target.label : p?.desc || target.label} titleTip={p?.exe || target.label} {overlay} {onclose}>
    {#snippet icon()}{#if exeIcon}<img src={exeIcon} width="28" height="28" alt="" />{:else}<Glyph name="app" size={22} />{/if}{/snippet}
    {#snippet sub()}
      {#if p || live}
        <span>PID <span class="mono">{target.pid}</span></span>
        {#if isGroup}<span>{t('procs.insp.n_procs', { n: live?.n ?? 0 })}</span>{/if}
        {#if p?.start_ms}<span>{t('procs.insp.running_for', { v: fmtDuration((procs.t - p.start_ms) / 1000) })}</span>{/if}
      {:else}
        <span class="warn-text">{t('procs.insp.ended')}</span>
      {/if}
    {/snippet}
    {#snippet actions()}
      {#if live}
        <button type="button" class="btn sm danger" onclick={() => onaction('kill')} title={t('procs.insp.kill_tip')}><Glyph name="close" size={10} /> {t('procs.kill.ok')}</button>
        <button type="button" class="btn sm" onclick={() => onaction('open')} disabled={!p?.exe}><Glyph name="folder" size={12} /> {t('procs.menu.open')}</button>
        <button type="button" class="btn sm icon-only" onclick={() => onaction('restart')} title={t('procs.menu.restart')} aria-label={t('procs.menu.restart')}><Glyph name="refresh" size={12} /></button>
        <button type="button" class="btn sm icon-only" onclick={(e) => onmore(e)} title={t('procs.insp.more')} aria-label={t('procs.insp.more')}><Glyph name="more" size={12} /></button>
      {/if}
    {/snippet}
      {#if live}
        <section class="insp-card">
          <div class="ic-stats">
            <div><span>CPU</span><b style="color:rgb({C.cpu})">{pctFmt(live.cpu)}</b></div>
            <div><span>{L.mem}</span><b style="color:rgb({C.mem})">{fmtBytes(live.mem)}</b></div>
            <div><span>{L.disk}</span><b style="color:rgb({C.disk})">{fmtRate(live.disk)}</b></div>
            <div><span>GPU</span><b style="color:rgb({C.gpu})">{pctFmt(live.gpu)}</b></div>
          </div>
          <div class="ic-chart">
            <div class="ic-lbl"><span><i style="background:rgb({C.cpu})"></i>CPU</span></div>
            <Chart t={hist.t} version={hist.v} range={RANGE} minMax={5} series={[{ data: hist.cpu, color: C.cpu, fill: true, label: 'CPU' }]} fmt={pctFmt} showMax label={t('procs.insp.chart_2m', { name: 'CPU' })} />
          </div>
          <div class="ic-chart">
            <div class="ic-lbl"><span><i style="background:rgb({C.mem})"></i>{L.mem}</span></div>
            <Chart t={hist.t} version={hist.v} range={RANGE} bytes minMax={1048576} series={[{ data: hist.mem, color: C.mem, fill: true, label: L.mem }]} fmt={bytesFmt} showMax label={t('procs.insp.chart_2m', { name: L.mem })} />
          </div>
          <div class="ic-chart">
            <div class="ic-lbl"><span><i style="background:rgb({C.disk})"></i>{t('procs.insp.disk_rw')}</span></div>
            <Chart t={hist.t} version={hist.v} range={RANGE} bytes minMax={65536} series={[{ data: hist.disk, color: C.disk, fill: true, label: L.disk }]} fmt={rateFmt} showMax label={t('procs.insp.chart_2m', { name: L.disk })} />
          </div>
          <div class="faint small">{t('procs.insp.last_2m')}</div>
        </section>
      {/if}

      {#if p}
        <dl class="props insp-props">
          {#if kindLabel}<dt>{t('procs.insp.kind')}</dt><dd>{kindLabel}{#if p.elevated} <span class="tag">{t('procs.status.admin')}</span>{/if}</dd>{/if}
          {#if winTitle}<dt>{t('procs.insp.window')}</dt><dd class="ellipsis" title={winTitle}>{winTitle}</dd>{/if}
          <dt>{t('procs.insp.file')}</dt><dd>{p.name}</dd>
          <dt>{t('procs.col.publisher')}</dt><dd>{p.publisher || '–'}</dd>
          <dt>{t('procs.col.user')}</dt><dd>{p.user || '–'}</dd>
          <dt>{t('procs.insp.parent')}</dt><dd>{#if parent}{parent.name} <span class="mono muted">({p.ppid})</span>{:else}<span class="mono">{p.ppid || '–'}</span>{/if}</dd>
          <dt>{t('procs.insp.children')}</dt><dd>{children}</dd>
          {#if live}<dt>{t('procs.insp.thr_hnd')}</dt><dd>{fmtInt(live.thr)} · {fmtInt(live.hnd)}</dd>{/if}
          <dt>{t('procs.insp.started')}</dt><dd>{p.start_ms ? new Date(p.start_ms).toLocaleString(i18n.intl) : '–'}</dd>
        </dl>

        <div class="cmd-block">
          <div class="cmd-head">
            <span class="lbl">{t('procs.menu.copy_path')}</span>
            <button type="button" class="icon-btn xs" title={t('procs.insp.copy_path')} aria-label={t('procs.insp.copy_path')} onclick={() => copy(p.exe, 'procs.copied.path')} disabled={!p.exe}><Glyph name="copy" size={12} /></button>
          </div>
          <div class="mono small break insp-path">{p.exe || t('procs.insp.no_path')}</div>
        </div>
        <div class="cmd-block">
          <div class="cmd-head">
            <span class="lbl">{t('procs.col.cmd')}</span>
            <button type="button" class="icon-btn xs" title={t('procs.insp.copy_cmd')} aria-label={t('procs.insp.copy_cmd')} onclick={() => copy(p.cmd, 'procs.copied.cmd')} disabled={!p.cmd}><Glyph name="copy" size={12} /></button>
          </div>
          <pre class="mono">{p.cmd || '–'}</pre>
        </div>
      {:else if !live}
        <p class="muted">{t('procs.insp.ended_pid', { pid: target.pid })}</p>
      {/if}
  </InspectorShell>
{:else}
  <!-- ================= TỔNG QUAN HỆ THỐNG ================= -->
  <InspectorShell label={t('procs.ov.title')} kicker={selCount > 1 ? t('procs.ov.selected', { n: selCount }) : t('procs.ov.pick')} title={t('procs.ov.title')} accent {overlay} {onclose}>
    {#snippet icon()}<NavIcon name="overview" size={19} />{/snippet}
    {#snippet sub()}{info?.hostname || ''}{info?.os ? ' · ' + info.os : ''}{/snippet}
    {#snippet actions()}
      <button type="button" class="btn sm" disabled={!m} onclick={() => copy(summary(), 'procs.copied.summary')}><Glyph name="copy" size={12} /> {t('procs.ov.copy')}</button>
      <button type="button" class="btn sm" disabled={!m} onclick={askAiMachine}><Glyph name="chat" size={12} /> {t('procs.ov.ask_ai')}</button>
    {/snippet}
      {#if !m}
        {#each Array(4) as _, i (i)}<div class="insp-card sk-card"><span class="sk" style="width:60%"></span><span class="sk" style="width:85%"></span></div>{/each}
      {:else}
        <section class="insp-card res">
          <div class="res-head"><Glyph name="processor" size={15} /><b>CPU</b><span class="res-val" style="color:rgb({C.cpu})">{fmtPct(m.cpu.total)}</span></div>
          {#if info?.cpu_name}<div class="res-line ellipsis" title={info.cpu_name}>{info.cpu_name}</div>{/if}
          <div class="res-sub muted">{info?.cores ? t('procs.ov.cores', { n: info.cores }) + ' · ' : ''}{t('procs.ov.threads', { n: info?.logical || m.cpu.cores.length })}{m.cpu.mhz ? ` · ${fmtGHz(m.cpu.mhz)}` : ''}</div>
          <div class="res-bar"><span style="width:{Math.min(100, m.cpu.total)}%; background:rgb({C.cpu})"></span></div>
        </section>

        <section class="insp-card res">
          <div class="res-head"><Glyph name="memory" size={15} /><b>{L.mem}</b><span class="res-val" style="color:rgb({C.mem})">{fmtGB(m.mem.used)}</span></div>
          {#each ramLines as l (l)}<div class="res-line" title={l}>{l}</div>{/each}
          <div class="res-sub muted">{t('procs.ov.mem_sub', { avail: fmtGB(m.mem.avail), total: fmtGB(m.mem.total), pct: fmtPct(memPct) })}</div>
          <div class="res-bar"><span style="width:{memPct}%; background:rgb({C.mem})"></span></div>
        </section>

        {#each m.disks || [] as d (d.id)}
          {@const di = diskInfo(d.id)}
          <section class="insp-card res">
            <div class="res-head"><Glyph name="disk" size={15} /><b class="ellipsis">{d.name}</b><span class="res-val" style="color:rgb({C.disk})">{fmtPct(d.active)}</span></div>
            {#if di?.model}<div class="res-line ellipsis" title={di.model}>{di.model}{di.size ? ` · ${fmtGB(di.size, 0)}` : ''}</div>{/if}
            <div class="res-sub muted">{t('procs.ov.disk_sub', { read: fmtRate(d.read), write: fmtRate(d.write) })}</div>
            <div class="res-bar"><span style="width:{Math.min(100, d.active)}%; background:rgb({C.disk})"></span></div>
          </section>
        {/each}

        {#if m.gpus?.length}
          <section class="insp-card res">
            <div class="res-head"><Glyph name="gpu" size={15} /><b>GPU</b><span class="res-val" style="color:rgb({C.gpu})">{fmtPct(gpuMax)}</span></div>
            {#each m.gpus as g (g.id)}
              <div class="res-row"><span class="ellipsis" title={g.name}>{g.name}</span><span class="mono muted">{fmtPct(g.util)}</span></div>
            {/each}
            <div class="res-sub muted">{t('procs.total_tip.gpu')}</div>
            <div class="res-bar"><span style="width:{Math.min(100, gpuMax)}%; background:rgb({C.gpu})"></span></div>
          </section>
        {/if}

        <section class="insp-card res">
          <div class="res-head"><Glyph name="network" size={15} /><b>{t('procs.ov.net')}</b><span class="res-val" style="color:rgb({C.net})">{fmtRate(net.rx + net.tx)}</span></div>
          {#each m.nets || [] as n (n.id)}
            <div class="res-row"><span class="ellipsis" title={n.desc || n.name}>{n.name}</span><span class="mono muted">↓ {fmtRate(n.rx)} · ↑ {fmtRate(n.tx)}</span></div>
          {:else}
            <div class="res-sub muted">{t('procs.ov.no_net')}</div>
          {/each}
        </section>

        {#if info?.board}
          <section class="insp-card res">
            <div class="res-head"><Glyph name="apps" size={15} /><b>{t('procs.ov.board')}</b></div>
            <div class="res-line">{info.board}</div>
          </section>
        {/if}

        {#if m.counts}
          <div class="insp-counts">
            <div><b>{fmtInt(m.counts.processes)}</b><span>{t('procs.ov.processes')}</span></div>
            <div><b>{fmtInt(m.counts.threads)}</b><span>{t('procs.col.thr')}</span></div>
            <div><b>{fmtInt(m.counts.handles)}</b><span>Handle</span></div>
          </div>
        {/if}
        <div class="faint small">{t('procs.ov.uptime', { v: fmtDuration(m.uptime_s) })}{m.battery ? (m.battery.charging ? t('procs.ov.battery_charging', { pct: m.battery.percent }) : t('procs.ov.battery', { pct: m.battery.percent })) : ''}</div>
      {/if}
  </InspectorShell>
{/if}
