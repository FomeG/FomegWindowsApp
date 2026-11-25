<script>
  // Thanh trạng thái dưới cùng (giống RustWTM): phiên bản, CPU, bộ nhớ, số tiến trình, độ trễ dữ liệu, chu kỳ.
  import { sys } from './sys.svelte.js';
  import { prefs } from './prefs.svelte.js';
  import { fmtPct, fmtGB, fmtInt } from './format.js';
  import { APP_VERSION } from './version.js';
  import IntervalSelect from './IntervalSelect.svelte';
  import { t } from './i18n.svelte.js';

  const m = $derived(sys.m);
  const ago = $derived(sys.lastT ? Math.max(0, Math.round((sys.now - sys.lastT) / 1000)) : -1);
</script>

<footer class="statusbar" aria-label={t('shell.status.label')}>
  <span class="faint">v{APP_VERSION}</span>
  {#if m}
    <span><span class="sb-dot" style="background:rgb(56,139,230)"></span>CPU <b>{fmtPct(m.cpu.total)}</b></span>
    <span><span class="sb-dot" style="background:rgb(160,108,213)"></span>{t('shell.status.mem')} <b>{fmtGB(m.mem.used)}</b> / {fmtGB(m.mem.total)}</span>
    <span>{t('shell.status.procs', { n: fmtInt(m.counts?.processes), count: m.counts?.processes ?? 0 })}</span>
  {/if}
  <span class="grow"></span>
  <span class="muted" aria-live="off">
    {#if prefs.interval === 0}{t('shell.status.paused')}{:else if ago < 0}{t('shell.status.waiting')}{:else if ago <= 1}{t('shell.status.just_now')}{:else}{t('shell.status.ago', { s: ago })}{/if}
  </span>
  <IntervalSelect compact />
</footer>
