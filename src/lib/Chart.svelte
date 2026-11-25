<script>
  // Biểu đồ canvas dùng chung (thay RamChart): nhiều chuỗi, tô vùng, max cố định hoặc tự co, lưới,
  // con trỏ dọc + tooltip khi rê chuột. Chế độ `spark` = sparkline nhỏ (không trục, không hover).
  //
  // Hiệu năng: chỉ vẽ khi `version` / kích thước / khoảng / theme / hover đổi, gộp theo khung hình (rAF).
  // Kích thước canvas chỉ đặt lại khi khung đổi kích thước (bind:clientWidth dùng ResizeObserver).
  // Khi số điểm > 2× số pixel thì gộp theo cột pixel (lấy max) để giữ đỉnh.
  import { prefs } from './prefs.svelte.js';
  import { fmtTime } from './format.js';
  import * as L from './i18n.svelte.js'; // prop `t` (mảng thời gian) trùng tên hàm t()

  let {
    t, // mảng thời gian (ms), dùng chung cho mọi chuỗi
    series, // [{ data, color: 'r,g,b', fill?: bool, width?, label?, dash? }]
    version = 0,
    range = 60000, // ms hiển thị (tính lùi từ điểm cuối)
    max = null, // số = trục cố định; null = tự co theo dữ liệu
    minMax = 1, // trần tối thiểu khi tự co
    bytes = false, // giá trị là byte: làm tròn trần theo bậc 1024 (KB/MB/GB) cho nhãn đẹp
    fmt = (v) => String(Math.round(v)), // định dạng giá trị (trục + tooltip)
    spark = false,
    grid = true,
    showMax = false, // nhãn giá trị trần ở góc trên phải
    label,
    class: cls = '',
  } = $props();

  let canvas = $state();
  let w = $state(0);
  let h = $state(0);
  let hover = $state(null); // { x } (px)
  let tip = $state(null); // { x, y, t, vals: [{label,color,v}] }
  let curMax = $state(0);

  let cw = 0, ch = 0, cdpr = 0; // kích thước canvas hiện tại
  let geom = null;

  function niceMax(v) {
    if (!(v > 0)) return minMax;
    v = Math.max(v * 1.1, minMax);
    if (bytes) {
      let unit = 1;
      while (v >= unit * 1024) unit *= 1024;
      return niceDec(v / unit) * unit;
    }
    return niceDec(v);
  }
  function niceDec(v) {
    const p = Math.pow(10, Math.floor(Math.log10(v)));
    const r = v / p;
    const m = r <= 1 ? 1 : r <= 2 ? 2 : r <= 2.5 ? 2.5 : r <= 5 ? 5 : 10;
    return m * p;
  }

  function lowerBound(a, x) {
    let lo = 0, hi = a.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (a[mid] < x) lo = mid + 1; else hi = mid;
    }
    return lo;
  }

  function draw() {
    if (!canvas || w < 2 || h < 2) return;
    const dpr = window.devicePixelRatio || 1;
    if (w !== cw || h !== ch || dpr !== cdpr) {
      canvas.width = Math.round(w * dpr);
      canvas.height = Math.round(h * dpr);
      cw = w; ch = h; cdpr = dpr;
    }
    const ctx = canvas.getContext('2d');
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, w, h);

    const n = t ? t.length : 0;
    const tEnd = n ? t[n - 1] : Date.now();
    const tStart = tEnd - range;
    const i0 = n ? Math.max(0, lowerBound(t, tStart) - 1) : 0;
    const base = series[0]?.color || '76,158,255';

    // trần trục Y
    let top = max;
    if (top == null) {
      let mx = 0;
      for (const s of series) {
        const d = s.data;
        if (!d) continue;
        for (let i = i0; i < n; i++) { const v = d[i]; if (v > mx) mx = v; }
      }
      top = niceMax(mx);
    }
    if (curMax !== top) curMax = top;

    const x = (tt) => ((tt - tStart) / range) * w;
    const y = (v) => h - 1 - (Math.min(v, top) / top) * (h - 2);
    geom = { tStart, i0, n, top };

    // lưới + viền
    const light = prefs.theme === 'light';
    if (grid && !spark) {
      ctx.strokeStyle = `rgba(${base},${light ? 0.16 : 0.13})`;
      ctx.lineWidth = 1;
      ctx.beginPath();
      for (let k = 1; k < 4; k++) { const yy = Math.round((h * k) / 4) + 0.5; ctx.moveTo(0, yy); ctx.lineTo(w, yy); }
      const cols = range <= 60000 ? 6 : range <= 300000 ? 5 : 6;
      // lưới dọc trôi theo thời gian (giống Task Manager)
      const stepT = range / cols;
      for (let gt = Math.ceil(tStart / stepT) * stepT; gt < tEnd; gt += stepT) { const xx = Math.round(x(gt)) + 0.5; ctx.moveTo(xx, 0); ctx.lineTo(xx, h); }
      ctx.stroke();
    }
    if (!spark) {
      ctx.strokeStyle = `rgba(${base},${light ? 0.55 : 0.5})`;
      ctx.lineWidth = 1;
      ctx.strokeRect(0.5, 0.5, w - 1, h - 1);
    }

    if (n >= 2) {
      const span = n - i0;
      const bucket = span > w * 2; // gộp theo cột pixel
      for (const s of series) {
        const d = s.data;
        if (!d) continue;
        const pts = [];
        if (bucket) {
          let col = -1, cm = NaN, ct = 0;
          for (let i = i0; i < n; i++) {
            const c = Math.floor(x(t[i]));
            if (c !== col) {
              if (col >= 0) pts.push(ct, cm);
              col = c; cm = d[i]; ct = t[i];
            } else if (d[i] > cm || cm !== cm) cm = d[i];
          }
          if (col >= 0) pts.push(ct, cm);
        } else {
          for (let i = i0; i < n; i++) pts.push(t[i], d[i]);
        }
        // đường + vùng tô theo từng đoạn liên tục (NaN = ngắt)
        const lw = s.width ?? (spark ? 1.25 : 1.5);
        const segs = [];
        let cur = null;
        for (let k = 0; k < pts.length; k += 2) {
          const v = pts[k + 1];
          if (v !== v || v == null) { cur = null; continue; }
          if (!cur) segs.push((cur = []));
          cur.push(x(pts[k]), y(v));
        }
        for (const sg of segs) {
          if (s.fill !== false && sg.length >= 4) {
            ctx.beginPath();
            ctx.moveTo(sg[0], h);
            for (let k = 0; k < sg.length; k += 2) ctx.lineTo(sg[k], sg[k + 1]);
            ctx.lineTo(sg[sg.length - 2], h);
            ctx.closePath();
            ctx.fillStyle = `rgba(${s.color},${spark ? (light ? 0.16 : 0.2) : light ? 0.13 : 0.18})`;
            ctx.fill();
          }
          ctx.beginPath();
          ctx.moveTo(sg[0], sg[1]);
          for (let k = 2; k < sg.length; k += 2) ctx.lineTo(sg[k], sg[k + 1]);
          ctx.strokeStyle = `rgb(${s.color})`;
          ctx.lineWidth = lw;
          ctx.lineJoin = 'round';
          if (s.dash) ctx.setLineDash(s.dash);
          ctx.stroke();
          if (s.dash) ctx.setLineDash([]);
        }
      }
    }

    // con trỏ dọc + tooltip
    if (hover && !spark && n) {
      const th = tStart + (hover.x / w) * range;
      let i = Math.min(n - 1, lowerBound(t, th));
      if (i > 0 && Math.abs(t[i - 1] - th) < Math.abs(t[i] - th)) i--;
      if (i >= i0) {
        const xx = Math.round(x(t[i])) + 0.5;
        ctx.strokeStyle = light ? 'rgba(0,0,0,.45)' : 'rgba(255,255,255,.5)';
        ctx.setLineDash([3, 3]);
        ctx.beginPath(); ctx.moveTo(xx, 0); ctx.lineTo(xx, h); ctx.stroke();
        ctx.setLineDash([]);
        const vals = [];
        let yMin = h;
        for (const s of series) {
          const v = s.data?.[i];
          if (v == null || v !== v) continue;
          const yy = y(v);
          yMin = Math.min(yMin, yy);
          ctx.fillStyle = `rgb(${s.color})`;
          ctx.beginPath(); ctx.arc(xx, yy, 3.2, 0, Math.PI * 2); ctx.fill();
          vals.push({ label: s.label, color: s.color, v });
        }
        tip = { x: xx, y: yMin, t: t[i], vals };
      } else tip = null;
    } else if (tip) tip = null;
  }

  // Vẽ lại khi dữ liệu / kích thước / khoảng / theme / hover / trần đổi.
  let raf = 0;
  $effect(() => {
    version; w; h; range; max; series; hover; prefs.theme;
    if (!raf) raf = requestAnimationFrame(() => { raf = 0; draw(); });
  });
  $effect(() => () => { if (raf) cancelAnimationFrame(raf); });

  let mvRaf = 0;
  let lastX = 0;
  function move(e) {
    if (spark) return;
    lastX = e.clientX - canvas.getBoundingClientRect().left;
    if (!mvRaf) mvRaf = requestAnimationFrame(() => { mvRaf = 0; hover = { x: lastX }; });
  }
  function leave() {
    if (mvRaf) { cancelAnimationFrame(mvRaf); mvRaf = 0; }
    hover = null;
    tip = null;
  }
</script>

<div class="chart2 {cls}" class:spark bind:clientWidth={w} bind:clientHeight={h} role="img" aria-label={label ?? L.t('shell.chart')}>
  <canvas bind:this={canvas} style="width:{w}px;height:{h}px" onpointermove={move} onpointerleave={leave}></canvas>
  {#if showMax && !spark}<span class="ch-max">{fmt(curMax)}</span>{/if}
  {#if tip && tip.vals.length}
    <div class="chart-tip2" style="left:{Math.min(w - 150, Math.max(0, tip.x + 10))}px; top:{Math.max(0, Math.min(h - 60, tip.y - 40))}px">
      <div class="muted small">{fmtTime(tip.t)}</div>
      {#each tip.vals as v, i (i)}
        <div class="tip-row"><span class="tip-dot" style="background:rgb({v.color})"></span>{#if v.label}<span class="muted">{v.label}</span>{/if}<b>{fmt(v.v)}</b></div>
      {/each}
    </div>
  {/if}
</div>
