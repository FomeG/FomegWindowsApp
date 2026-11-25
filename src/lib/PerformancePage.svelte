<script>
  // Trang Hiệu năng kiểu Task Manager / RustWTM: cột trái là danh sách tài nguyên (sparkline + giá trị),
  // bên phải là biểu đồ lớn của tài nguyên đang chọn + lưới thông số.
  import { sys } from './sys.svelte.js';
  import { prefs } from './prefs.svelte.js';
  import { C } from './colors.js';
  import { fmtPct, fmtRate, fmtBits, fmtGB, fmtBytes, fmtInt, fmtUptime, fmtGHz, memModuleLines, memSpec } from './format.js';
  import Chart from './Chart.svelte';
  import Glyph from './Glyph.svelte';
  import { t } from './i18n.svelte.js';

  const RANGES = [1, 2, 5, 15, 60];
  const m = $derived(sys.m);
  const info = $derived(sys.info);
  const h = sys.h;

  // Danh sách tài nguyên (ổn định theo id thiết bị)
  const resources = $derived.by(() => {
    const list = [{ id: 'cpu', kind: 'cpu', name: 'CPU', glyph: 'processor', color: C.cpu }, { id: 'mem', kind: 'mem', name: t('perf.mem'), glyph: 'memory', color: C.mem }];
    for (const d of m?.disks || []) list.push({ id: 'disk:' + d.id, kind: 'disk', dev: d.id, name: d.name, glyph: 'disk', color: C.disk });
    for (const n of m?.nets || []) list.push({ id: 'net:' + n.id, kind: 'net', dev: n.id, name: n.name, glyph: n.kind === 'wifi' ? 'wifi' : 'ethernet', color: C.net });
    (m?.gpus || []).forEach((g, i) => list.push({ id: 'gpu:' + g.id, kind: 'gpu', dev: g.id, name: `GPU ${i}`, glyph: 'monitor', color: C.gpu }));
    if (m?.battery) list.push({ id: 'bat', kind: 'bat', name: t('perf.battery'), glyph: 'battery', color: C.bat });
    return list;
  });
  // id đã chọn còn hợp lệ? 'gpu' / 'disk' chung -> thiết bị đầu tiên cùng loại
  const sel = $derived.by(() => {
    const id = prefs.perfSel;
    return resources.find((r) => r.id === id) || resources.find((r) => r.kind === String(id).split(':')[0]) || resources[0];
  });

  const rangeMs = $derived(prefs.perfRange * 60000);
  const rangeLabel = $derived(prefs.perfRange === 1 ? t('perf.range.60s') : t('perf.range.min', { n: prefs.perfRange }));

  // Giá trị sống theo tài nguyên
  const disk = (id) => m?.disks?.find((d) => d.id === id);
  const nic = (id) => m?.nets?.find((n) => n.id === id);
  const gpuOf = (id) => m?.gpus?.find((g) => g.id === id);
  const gpuInfo = (id) => info?.gpus?.find((g) => g.id === id);
  const ramLines = $derived(memModuleLines(info));
  const ramSpec = $derived(memSpec(info));
  const diskInfo = (id) => info?.disks?.find((d) => d.id === id);

  function summary(r) {
    if (!m) return '';
    switch (r.kind) {
      case 'cpu': return `${fmtPct(m.cpu.total)} ${fmtGHz(m.cpu.mhz)}`;
      case 'mem': return `${fmtGB(m.mem.used)} / ${fmtGB(m.mem.total)} (${fmtPct((m.mem.used / m.mem.total) * 100)})`;
      case 'disk': { const d = disk(r.dev); return d ? `${diskInfo(r.dev)?.ssd === false ? 'HDD' : 'SSD'} · ${fmtPct(d.active)}` : ''; }
      case 'net': { const n = nic(r.dev); return n ? `↓ ${fmtBits(n.rx)} ↑ ${fmtBits(n.tx)}` : ''; }
      case 'gpu': { const g = gpuOf(r.dev); return g ? `${fmtPct(g.util)}` : ''; }
      case 'bat': return `${m.battery.percent}%${m.battery.charging ? ' · ' + t('perf.bat.charging_l') : m.battery.plugged ? ' · ' + t('perf.bat.plugged_l') : ''}`;
    }
    return '';
  }
  function sub(r) {
    if (r.kind === 'gpu') return gpuOf(r.dev)?.name || '';
    if (r.kind === 'net') return nic(r.dev)?.desc || '';
    return '';
  }
  function sparkSeries(r) {
    switch (r.kind) {
      case 'cpu': return { max: 100, s: [{ data: h.cpu, color: C.cpu }] };
      case 'mem': return { max: m?.mem.total || 1, s: [{ data: h.mem, color: C.mem }] };
      case 'disk': return h.disks[r.dev] ? { max: 100, s: [{ data: h.disks[r.dev].active, color: C.disk }] } : null;
      case 'net': return h.nets[r.dev] ? { max: null, s: [{ data: h.nets[r.dev].rx, color: C.rx }, { data: h.nets[r.dev].tx, color: C.tx, fill: false }] } : null;
      case 'gpu': return h.gpus[r.dev] ? { max: 100, s: [{ data: h.gpus[r.dev].util, color: C.gpu }] } : null;
    }
    return null;
  }

  const pctFmt = (v) => fmtPct(v, v < 10 ? 1 : 0);
  const gbFmt = (v) => fmtGB(v, 2);
  const rateFmt = (v) => fmtRate(v);
  const bitsFmt = (v) => fmtBits(v);
  const bytesFmt = (v) => fmtBytes(v);

  // CPU
  const coreCols = $derived.by(() => {
    const n = m?.cpu.cores.length || 12;
    return n <= 2 ? n : Math.ceil(n / Math.ceil(Math.sqrt(n / 1.5)));
  });

  // Bộ nhớ: thành phần
  const memParts = $derived.by(() => {
    if (!m) return null;
    const { used, total, avail, cached } = m.mem;
    const standby = Math.min(cached || 0, avail || 0);
    const free = Math.max(0, (avail || 0) - standby);
    return { used, standby, free, total, usedP: (used / total) * 100, standbyP: (standby / total) * 100, freeP: (free / total) * 100 };
  });
</script>

<section class="page perf">
  <div class="perf-left" role="listbox" aria-label={t('perf.resources')}>
    {#each resources as r (r.id)}
      {@const sp = sparkSeries(r)}
      <button type="button" role="option" aria-selected={sel?.id === r.id} class="perf-item" class:active={sel?.id === r.id} onclick={() => prefs.set('perfSel', r.id)}>
        <span class="pi-spark" style="--c:{r.color}">
          {#if sp}<Chart spark t={h.t} version={sys.hv} max={sp.max} series={sp.s} minMax={10240} label={t('perf.spark_label', { name: r.name })} />{:else}<Glyph name={r.glyph} size={18} />{/if}
        </span>
        <span class="pi-text">
          <span class="pi-name">{r.name}</span>
          {#if sub(r)}<span class="pi-sub faint ellipsis">{sub(r)}</span>{/if}
          <span class="pi-val ellipsis" style="color:rgb({r.color})">{summary(r)}</span>
        </span>
      </button>
    {/each}
  </div>

  <div class="perf-right">
    {#if !m || !sel}
      <div class="placeholder">{#each Array(5) as _, i (i)}<span class="sk" style="width:{90 - i * 12}%"></span>{/each}</div>
    {:else}
      <header class="perf-head">
        <h1>{sel.kind === 'gpu' ? sel.name : sel.name}</h1>
        <span class="perf-model ellipsis">
          {#if sel.kind === 'cpu'}{info?.cpu_name || ''}
          {:else if sel.kind === 'mem'}{fmtGB(m.mem.total)}
          {:else if sel.kind === 'disk'}{diskInfo(sel.dev)?.model || ''}
          {:else if sel.kind === 'net'}{sel.dev}
          {:else if sel.kind === 'gpu'}{gpuOf(sel.dev)?.name || ''}{/if}
        </span>
        <span class="grow"></span>
        {#if sel.kind !== 'bat'}
          <div class="seg" role="radiogroup" aria-label={t('perf.range.label')}>
            {#each RANGES as r (r)}
              <button type="button" role="radio" aria-checked={prefs.perfRange === r} class:active={prefs.perfRange === r} onclick={() => prefs.set('perfRange', r)}>{t('perf.range.min', { n: r })}</button>
            {/each}
          </div>
        {/if}
      </header>

      {#if sel.kind === 'cpu'}
        <div class="ch-labels"><span>{t('perf.cpu.util_pct')}</span>
          <div class="seg sm" role="radiogroup" aria-label={t('perf.cpu.chart_kind')}>
            <button type="button" role="radio" aria-checked={!prefs.perfCores} class:active={!prefs.perfCores} onclick={() => prefs.set('perfCores', false)}>{t('perf.cpu.overall')}</button>
            <button type="button" role="radio" aria-checked={prefs.perfCores} class:active={prefs.perfCores} onclick={() => prefs.set('perfCores', true)}>{t('perf.cpu.per_core')}</button>
          </div>
          <span>100%</span></div>
        {#if !prefs.perfCores}
          <div class="big-chart"><Chart t={h.t} version={sys.hv} range={rangeMs} max={100} series={[{ data: h.cpu, color: C.cpu, label: 'CPU' }]} fmt={pctFmt} label={t('perf.cpu.chart')} /></div>
        {:else}
          <div class="cores-grid" style="grid-template-columns:repeat({coreCols}, minmax(0,1fr))">
            {#each h.cores as data, i (i)}
              <div class="core-cell">
                <span class="core-lbl faint">CPU {i} · {fmtPct(m.cpu.cores[i] ?? 0)}</span>
                <Chart t={h.t} version={sys.hv} range={rangeMs} max={100} series={[{ data, color: C.cpu, label: 'CPU ' + i, width: 1.2 }]} fmt={pctFmt} label={t('perf.cpu.core_chart', { i })} />
              </div>
            {/each}
          </div>
        {/if}
        <div class="ch-labels foot"><span>{rangeLabel}</span><span>0</span></div>
        <div class="stats">
          <div class="st-live">
            <div><span>{t('perf.util')}</span><b>{fmtPct(m.cpu.total)}</b></div>
            <div><span>{t('perf.speed')}</span><b>{fmtGHz(m.cpu.mhz)}</b></div>
            <div><span>{t('perf.processes')}</span><b>{fmtInt(m.counts.processes)}</b></div>
            <div><span>{t('perf.threads')}</span><b>{fmtInt(m.counts.threads)}</b></div>
            <div><span>Handle</span><b>{fmtInt(m.counts.handles)}</b></div>
            <div><span>{t('perf.uptime')}</span><b class="mono-n">{fmtUptime(m.uptime_s)}</b></div>
          </div>
          <dl class="st-static">
            <dt>{t('perf.cpu.base_speed')}</dt><dd>{info ? fmtGHz(info.base_mhz) : '–'}</dd>
            <dt>{t('perf.cpu.cores')}</dt><dd>{info?.cores ?? '–'}</dd>
            <dt>{t('perf.cpu.logical')}</dt><dd>{info?.logical ?? m.cpu.cores.length}</dd>
          </dl>
        </div>

      {:else if sel.kind === 'mem'}
        <div class="ch-labels"><span>{t('perf.mem.usage')}</span><span>{fmtGB(m.mem.total)}</span></div>
        <div class="big-chart"><Chart t={h.t} version={sys.hv} range={rangeMs} max={m.mem.total} series={[{ data: h.mem, color: C.mem, label: t('perf.mem.used_series') }]} fmt={gbFmt} label={t('perf.mem.chart')} /></div>
        <div class="ch-labels foot"><span>{rangeLabel}</span><span>0</span></div>
        <div class="ch-labels"><span>{t('perf.mem.composition')}</span></div>
        {#if memParts}
          <div class="mem-comp" aria-label={t('perf.mem.composition')}>
            <span class="mc-used" style="width:{memParts.usedP}%" title={t('perf.mem.in_use_tip', { v: fmtGB(memParts.used) })}></span>
            <span class="mc-standby" style="width:{memParts.standbyP}%" title={t('perf.mem.cached_tip', { v: fmtGB(memParts.standby) })}></span>
            <span class="mc-free" style="width:{memParts.freeP}%" title={t('perf.mem.free_tip', { v: fmtGB(memParts.free) })}></span>
          </div>
          <div class="mem-legend small muted">
            <span><i class="mc-used"></i>{t('perf.mem.in_use_l', { v: fmtGB(memParts.used) })}</span>
            <span><i class="mc-standby"></i>{t('perf.mem.cached_l', { v: fmtGB(memParts.standby) })}</span>
            <span><i class="mc-free"></i>{t('perf.mem.free_l', { v: fmtGB(memParts.free) })}</span>
          </div>
        {/if}
        <div class="stats">
          <div class="st-live">
            <div><span>{t('perf.mem.in_use')}</span><b>{fmtGB(m.mem.used)}</b></div>
            <div><span>{t('perf.mem.available')}</span><b>{fmtGB(m.mem.avail)}</b></div>
            <div><span>{t('perf.mem.committed')}</span><b>{fmtGB(m.mem.committed)} / {fmtGB(m.mem.commit_limit)}</b></div>
            <div><span>{t('perf.mem.cached')}</span><b>{fmtGB(m.mem.cached)}</b></div>
            <div><span>{t('perf.mem.paged')}</span><b>{fmtBytes(m.mem.paged_pool)}</b></div>
            <div><span>{t('perf.mem.nonpaged')}</span><b>{fmtBytes(m.mem.nonpaged_pool)}</b></div>
          </div>
          {#if ramSpec.used}
            <dl class="st-static">
              {#if ramSpec.speed}<dt>{t('perf.speed')}</dt><dd>{ramSpec.speed} MT/s</dd>{/if}
              {#if ramSpec.slots}<dt>{t('perf.mem.slots')}</dt><dd>{ramSpec.used} / {ramSpec.slots}</dd>{/if}
              {#if ramSpec.kind}<dt>{t('perf.mem.kind')}</dt><dd>{ramSpec.kind}</dd>{/if}
              {#if ramSpec.form}<dt>{t('perf.mem.form')}</dt><dd>{ramSpec.form}</dd>{/if}
              {#each ramLines as l, i (l)}<dt>{i === 0 ? t('perf.mem.modules') : ''}</dt><dd>{l}</dd>{/each}
            </dl>
          {/if}
        </div>

      {:else if sel.kind === 'disk'}
        {@const d = disk(sel.dev)}
        {@const di = diskInfo(sel.dev)}
        {@const hd = h.disks[sel.dev]}
        {#if d && hd}
          <div class="ch-labels"><span>{t('perf.disk.active')}</span><span>100%</span></div>
          <div class="big-chart mid"><Chart t={h.t} version={sys.hv} range={rangeMs} max={100} series={[{ data: hd.active, color: C.disk, label: t('perf.disk.active_series') }]} fmt={pctFmt} label={t('perf.disk.active_chart')} /></div>
          <div class="ch-labels foot"><span>{rangeLabel}</span><span>0</span></div>
          <div class="ch-labels"><span>{t('perf.disk.transfer')} <i class="lg" style="background:rgb({C.read})"></i>{t('perf.disk.read_l')} <i class="lg" style="background:rgb({C.write})"></i>{t('perf.disk.write_l')}</span></div>
          <div class="big-chart mid"><Chart t={h.t} version={sys.hv} range={rangeMs} showMax bytes minMax={102400} series={[{ data: hd.read, color: C.read, label: t('perf.disk.read') }, { data: hd.write, color: C.write, label: t('perf.disk.write'), fill: false }]} fmt={rateFmt} label={t('perf.disk.rw_chart')} /></div>
          <div class="stats">
            <div class="st-live">
              <div><span>{t('perf.disk.active')}</span><b>{fmtPct(d.active)}</b></div>
              <div><span>{t('perf.disk.read_speed')}</span><b style="color:rgb({C.read})">{fmtRate(d.read)}</b></div>
              <div><span>{t('perf.disk.write_speed')}</span><b style="color:rgb({C.write})">{fmtRate(d.write)}</b></div>
            </div>
            <dl class="st-static">
              <dt>{t('perf.disk.capacity')}</dt><dd>{di ? fmtBytes(di.size) : '–'}</dd>
              <dt>{t('perf.disk.type')}</dt><dd>{di ? (di.ssd ? 'SSD' : 'HDD') : '–'}</dd>
              <dt>Model</dt><dd>{di?.model || '–'}</dd>
            </dl>
          </div>
        {/if}

      {:else if sel.kind === 'net'}
        {@const n = nic(sel.dev)}
        {@const hn = h.nets[sel.dev]}
        {#if n && hn}
          <div class="ch-labels"><span>{t('perf.net.throughput')} <i class="lg" style="background:rgb({C.rx})"></i>{t('perf.net.recv_l')} <i class="lg" style="background:rgb({C.tx})"></i>{t('perf.net.send_l')}</span></div>
          <div class="big-chart"><Chart t={h.t} version={sys.hv} range={rangeMs} showMax minMax={1250} series={[{ data: hn.rx, color: C.rx, label: t('perf.net.recv') }, { data: hn.tx, color: C.tx, label: t('perf.net.send'), fill: false }]} fmt={bitsFmt} label={t('perf.net.chart')} /></div>
          <div class="ch-labels foot"><span>{rangeLabel}</span><span>0</span></div>
          <div class="stats">
            <div class="st-live">
              <div><span>{t('perf.net.send')}</span><b style="color:rgb({C.tx})">{fmtBits(n.tx)}</b></div>
              <div><span>{t('perf.net.recv')}</span><b style="color:rgb({C.rx})">{fmtBits(n.rx)}</b></div>
              <div><span>{t('perf.net.send_bytes')}</span><b>{fmtRate(n.tx)}</b></div>
              <div><span>{t('perf.net.recv_bytes')}</span><b>{fmtRate(n.rx)}</b></div>
            </div>
            <dl class="st-static">
              <dt>{t('perf.net.adapter')}</dt><dd>{n.desc || n.name}</dd>
              <dt>{t('perf.net.conn_name')}</dt><dd>{n.name}</dd>
              <dt>{t('perf.net.conn_type')}</dt><dd>{n.kind === 'wifi' ? 'Wi-Fi' : n.kind === 'ethernet' ? 'Ethernet' : n.kind || '–'}</dd>
              <dt>{t('perf.net.link_speed')}</dt><dd>{n.link_bps ? fmtBits(n.link_bps / 8) : '–'}</dd>
            </dl>
          </div>
        {/if}

      {:else if sel.kind === 'gpu'}
        {@const g = gpuOf(sel.dev)}
        {@const gi = gpuInfo(sel.dev)}
        {@const hg = h.gpus[sel.dev]}
        {#if g && hg}
          <div class="gpu-grid">
            {#each g.engines as e (e.name)}
              <div class="gpu-cell">
                <div class="ch-labels"><span>{e.name}</span><span>{fmtPct(e.util)}</span></div>
                <div class="eng-chart"><Chart t={h.t} version={sys.hv} range={rangeMs} max={100} series={[{ data: hg.eng[e.name], color: C.gpu, label: e.name }]} fmt={pctFmt} label="GPU {e.name}" /></div>
              </div>
            {/each}
          </div>
          <div class="ch-labels"><span>{t('perf.gpu.ded')}</span><span>{fmtBytes(g.ded_total || gi?.ded_total)}</span></div>
          <div class="mem-chart"><Chart t={h.t} version={sys.hv} range={rangeMs} max={g.ded_total || gi?.ded_total || null} series={[{ data: hg.ded, color: C.ded, label: t('perf.gpu.ded_series') }]} fmt={bytesFmt} label={t('perf.gpu.ded')} /></div>
          <div class="ch-labels"><span>{t('perf.gpu.shared')}</span><span>{fmtBytes(g.shared_total || gi?.shared_total)}</span></div>
          <div class="mem-chart"><Chart t={h.t} version={sys.hv} range={rangeMs} max={g.shared_total || gi?.shared_total || null} series={[{ data: hg.shared, color: C.shared, label: t('perf.gpu.shared_series') }]} fmt={bytesFmt} label={t('perf.gpu.shared')} /></div>
          <div class="stats">
            <div class="st-live">
              <div><span>{t('perf.util')}</span><b>{fmtPct(g.util)}</b></div>
              <div><span>{t('perf.gpu.ded')}</span><b>{fmtBytes(g.ded_used)} / {fmtBytes(g.ded_total)}</b></div>
              <div><span>{t('perf.gpu.shared')}</span><b>{fmtBytes(g.shared_used)} / {fmtBytes(g.shared_total)}</b></div>
              <div><span>{t('perf.gpu.mem')}</span><b>{fmtBytes(g.ded_used + g.shared_used)} / {fmtBytes(g.ded_total + g.shared_total)}</b></div>
            </div>
            <dl class="st-static"><dt>{t('perf.gpu.name')}</dt><dd>{g.name}</dd></dl>
          </div>
        {/if}

      {:else if sel.kind === 'bat' && m.battery}
        <div class="bat-big">
          <div class="bat-shell"><span style="width:{m.battery.percent}%; background:rgb({m.battery.percent <= 15 ? C.write : C.bat})"></span></div>
          <b>{m.battery.percent}%</b>
        </div>
        <div class="stats">
          <div class="st-live">
            <div><span>{t('perf.bat.level')}</span><b>{m.battery.percent}%</b></div>
            <div><span>{t('perf.bat.status')}</span><b>{m.battery.charging ? t('perf.bat.charging') : m.battery.plugged ? t('perf.bat.plugged') : t('perf.bat.on_battery')}</b></div>
          </div>
        </div>
      {/if}
    {/if}
  </div>
</section>
