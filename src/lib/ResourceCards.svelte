<script>
  // Năm thẻ tài nguyên đầu trang Tổng quan (giống RustWTM): CPU, Bộ nhớ, GPU, Đĩa & I/O, Mạng.
  // Bấm thẻ -> trang Hiệu năng ở tài nguyên đó.
  import { sys } from './sys.svelte.js';
  import { prefs } from './prefs.svelte.js';
  import { store } from './store.svelte.js';
  import { C } from './colors.js';
  import { fmtPct, fmtRate, fmtGB, fmtBytes } from './format.js';
  import Chart from './Chart.svelte';
  import Ring from './Ring.svelte';
  import { t } from './i18n.svelte.js';

  const m = $derived(sys.m);
  const info = $derived(sys.info);
  const h = sys.h;

  // GPU bận nhất
  const gpu = $derived.by(() => {
    const gs = m?.gpus || [];
    let best = null;
    for (const g of gs) if (!best || g.util > best.util) best = g;
    return best;
  });
  const busiestEngine = $derived.by(() => {
    let e = null;
    for (const x of gpu?.engines || []) if (!e || x.util > e.util) e = x;
    return e;
  });
  const disk = $derived.by(() => {
    let r = 0, w = 0;
    for (const d of m?.disks || []) { r += d.read; w += d.write; }
    return { r, w };
  });
  const net = $derived.by(() => {
    let rx = 0, tx = 0;
    for (const n of m?.nets || []) { rx += n.rx; tx += n.tx; }
    return { rx, tx };
  });
  const memPct = $derived(m ? (m.mem.used / m.mem.total) * 100 : 0);

  function go(sel) {
    prefs.set('perfSel', sel);
    store.page = 'performance';
  }
  const coreColor = (v) => (v >= 85 ? '232,83,74' : v >= 60 ? '227,163,59' : C.cpu);
  const pctFmt = (v) => fmtPct(v);
  const rateFmt = (v) => fmtRate(v);
  const gbFmt = (v) => fmtGB(v);
</script>

<div class="rcards">
  <button type="button" class="rcard" onclick={() => go('cpu')} title={t('perf.rc.cpu_tip')}>
    <div class="rc-head"><span class="dot" style="background:rgb({C.cpu})"></span><span class="rc-title" style="color:rgb({C.cpu})">CPU</span></div>
    <div class="rc-main">
      <div class="rc-val"><span class="big">{m ? fmtPct(m.cpu.total) : '–'}</span><span class="muted small">{t('perf.rc.logical', { n: info ? info.logical : m?.cpu.cores.length || '' })}</span></div>
      <Ring value={m?.cpu.total} color={C.cpu} />
    </div>
    <div class="cores-strip" aria-hidden="true">
      {#each m?.cpu.cores || [] as v, i (i)}<span style="--v:{Math.max(4, v)}%; --c:{coreColor(v)}"></span>{/each}
    </div>
    <div class="rc-spark"><Chart spark t={h.t} version={sys.hv} max={100} series={[{ data: h.cpu, color: C.cpu }]} label={t('perf.spark_label', { name: 'CPU' })} fmt={pctFmt} /></div>
  </button>

  <button type="button" class="rcard" onclick={() => go('mem')} title={t('perf.rc.mem_tip')}>
    <div class="rc-head"><span class="dot" style="background:rgb({C.mem})"></span><span class="rc-title" style="color:rgb({C.mem})">{t('perf.mem')}</span></div>
    <div class="rc-main">
      <div class="rc-val"><span class="big">{m ? fmtPct(memPct) : '–'}</span><span class="muted small">{m ? `${fmtGB(m.mem.used)} / ${fmtGB(m.mem.total)}` : ''}</span></div>
      <Ring value={memPct} color={C.mem} />
    </div>
    <div class="usebar" aria-hidden="true"><span style="width:{memPct}%; background:rgb({C.mem})"></span></div>
    <div class="rc-spark"><Chart spark t={h.t} version={sys.hv} max={m?.mem.total || info?.mem_total || 1} series={[{ data: h.mem, color: C.mem }]} label={t('perf.spark_label', { name: t('perf.mem') })} fmt={gbFmt} /></div>
  </button>

  <button type="button" class="rcard" onclick={() => go(gpu ? 'gpu:' + gpu.id : 'gpu')} title={t('perf.rc.gpu_tip')} disabled={m && !m.gpus?.length}>
    <div class="rc-head"><span class="dot" style="background:rgb({C.gpu})"></span><span class="rc-title" style="color:rgb({C.gpu})">GPU</span></div>
    <div class="rc-main">
      <div class="rc-val"><span class="big">{gpu ? fmtPct(gpu.util) : '–'}</span><span class="muted small ellipsis">{busiestEngine ? t('perf.rc.busiest_engine', { name: busiestEngine.name }) : t('perf.rc.no_gpu')}</span></div>
      <Ring value={gpu?.util} color={C.gpu} />
    </div>
    <div class="eng-mini">
      {#each (gpu?.engines || []).slice(0, 2) as e (e.name)}
        <div><span class="small muted">{e.name}</span><span class="small">{fmtPct(e.util)}</span><i><b style="width:{e.util}%"></b></i></div>
      {/each}
    </div>
    <div class="rc-spark">{#if gpu && h.gpus[gpu.id]}<Chart spark t={h.t} version={sys.hv} max={100} series={[{ data: h.gpus[gpu.id].util, color: C.gpu }]} label={t('perf.spark_label', { name: 'GPU' })} fmt={pctFmt} />{/if}</div>
  </button>

  <button type="button" class="rcard" onclick={() => go(m?.disks?.[0] ? 'disk:' + m.disks[0].id : 'cpu')} title={t('perf.rc.disk_tip')}>
    <div class="rc-head"><span class="dot" style="background:rgb({C.disk})"></span><span class="rc-title" style="color:rgb({C.disk})">{t('perf.rc.disk_io')}</span></div>
    <div class="rc-val"><span class="big">{m ? fmtRate(disk.r + disk.w) : '–'}</span><span class="muted small">{t('perf.rc.disk_rw', { r: fmtRate(disk.r), w: fmtRate(disk.w) })}</span></div>
    <div class="disk-rows">
      {#each m?.disks || [] as d (d.id)}
        <div><span class="small muted ellipsis">{d.name}</span><span class="small">{fmtPct(d.active)}</span><i><b style="width:{d.active}%"></b></i></div>
      {/each}
    </div>
    <div class="rc-spark">
      {#if m?.disks?.[0] && h.disks[m.disks[0].id]}
        <Chart spark t={h.t} version={sys.hv} series={[{ data: h.disks[m.disks[0].id].read, color: C.read }, { data: h.disks[m.disks[0].id].write, color: C.write, fill: false }]} minMax={1048576} label={t('perf.spark_label', { name: t('perf.disk') })} fmt={rateFmt} />
      {/if}
    </div>
  </button>

  <button type="button" class="rcard" onclick={() => go(m?.nets?.[0] ? 'net:' + m.nets[0].id : 'cpu')} title={t('perf.rc.net_tip')}>
    <div class="rc-head"><span class="dot" style="background:rgb({C.net})"></span><span class="rc-title" style="color:rgb({C.net})">{t('perf.net')}</span></div>
    <div class="rc-val"><span class="big">{m ? fmtRate(net.rx + net.tx) : '–'}</span><span class="muted small">↓ {fmtRate(net.rx)} · ↑ {fmtRate(net.tx)}</span></div>
    <div class="disk-rows">
      {#each (m?.nets || []).slice(0, 2) as n (n.id)}
        <div class="net-row"><span class="small muted ellipsis">{n.name}</span><span class="small faint ellipsis">{n.desc || ""}</span></div>
      {:else}
        <div class="small muted">{t('perf.rc.no_net')}</div>
      {/each}
    </div>
    <div class="rc-spark">
      {#if m?.nets?.[0] && h.nets[m.nets[0].id]}
        <Chart spark t={h.t} version={sys.hv} series={[{ data: h.nets[m.nets[0].id].rx, color: C.rx }, { data: h.nets[m.nets[0].id].tx, color: C.tx, fill: false }]} minMax={10240} label={t('perf.spark_label', { name: t('perf.net') })} fmt={rateFmt} />
      {/if}
    </div>
  </button>
</div>
