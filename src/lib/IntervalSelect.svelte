<script>
  // Chọn chu kỳ cập nhật số liệu (set_interval). 0 = tạm dừng.
  import { sys, INTERVALS } from './sys.svelte.js';
  import { prefs } from './prefs.svelte.js';
  import { t, i18n } from './i18n.svelte.js';
  let { compact = false } = $props();
  const secs = (ms) => (ms / 1000).toLocaleString(i18n.intl);
  const label = (ms) => (ms === 0 ? t('shell.interval.paused') : t('shell.interval.secs', { s: secs(ms) }));
</script>

{#if compact}
  <select class="sb-select" value={prefs.interval} onchange={(e) => sys.setInterval(Number(e.currentTarget.value))} aria-label={t('shell.interval.label')} title={t('shell.interval.label')}>
    {#each INTERVALS as ms (ms)}<option value={ms}>{ms === 0 ? t('shell.interval.paused') : t('shell.interval.every', { s: secs(ms) })}</option>{/each}
  </select>
{:else}
  <div class="seg" role="radiogroup" aria-label={t('shell.interval.label')}>
    {#each INTERVALS as ms (ms)}
      <button type="button" role="radio" aria-checked={prefs.interval === ms} class:active={prefs.interval === ms} onclick={() => sys.setInterval(ms)}>{label(ms)}</button>
    {/each}
  </div>
{/if}
