<script>
  // Trạng thái chạy: MỘT nút dạng viên thuốc – hiển thị trạng thái và bấm để chạy/dừng.
  import { store } from './store.svelte.js';
  import { STATE_LABELS, CRITICAL } from './format.js';
  import Icon from './Icon.svelte';
  import { t } from './i18n.svelte.js';
  let { svc } = $props();
  const pending = $derived(store.pending.has(svc.id));
  const running = $derived(svc.state === 'running');
  const transitional = $derived(['starting', 'stopping', 'pending'].includes(svc.state));
  const locked = $derived(running && svc.critical);
  const disabledStart = $derived(!running && svc.start_type === 'disabled');
  const title = $derived(
    pending ? t('startup.svc.pill.working')
    : locked ? CRITICAL.tip
    : transitional ? t('startup.svc.pill.transitioning')
    : disabledStart ? t('startup.svc.pill.disabled_tip')
    : running ? t('startup.svc.pill.click_stop') : t('startup.svc.pill.click_start')
  );
</script>

<button
  type="button"
  class="pill st-{svc.state || 'stopped'}"
  class:locked
  disabled={pending || transitional || locked || disabledStart}
  {title}
  aria-label="{STATE_LABELS[svc.state] || svc.state}. {title}"
  onclick={(e) => { e.stopPropagation(); store.control([svc], running ? 'stop' : 'start'); }}
>
  {#if pending}<span class="spin sm"></span>{:else}<span class="dot"></span>{/if}
  <span class="pill-label">{pending ? (running ? t('startup.svc.pill.stopping') : t('startup.svc.pill.starting')) : STATE_LABELS[svc.state] || svc.state || '–'}</span>
  {#if locked}<Icon name="lock" size={12} />{:else if !pending && !transitional && !disabledStart}<span class="pill-act">{running ? t('startup.svc.stop') : t('startup.svc.start')}</span>{/if}
</button>
