<script>
  // Kiểu khởi động của dịch vụ: MỘT dropdown, đổi là áp dụng ngay.
  import { store } from './store.svelte.js';
  import { START_LABELS, MODE_OPTIONS, CRITICAL } from './format.js';
  import Icon from './Icon.svelte';
  import { t } from './i18n.svelte.js';
  let { svc } = $props();
  const pending = $derived(store.pending.has(svc.id));
  const fixed = $derived(svc.start_type === 'boot' || svc.start_type === 'system');
  let el = $state();

  async function change(e) {
    const mode = e.currentTarget.value;
    await store.setMode([svc], mode);
    // nếu bị huỷ/lỗi, đặt lại giá trị DOM theo dữ liệu
    if (el) el.value = store.servicesById.get(svc.id)?.start_type ?? svc.start_type;
  }
</script>

<span class="mode-wrap" class:pending>
  <select
    bind:this={el}
    class="mode mode-{svc.start_type}"
    value={svc.start_type}
    disabled={pending || fixed}
    aria-label={t('startup.svc.mode.aria', { name: svc.name })}
    title={fixed ? t('startup.svc.mode.fixed_tip') : svc.critical ? CRITICAL.tip : t('startup.svc.mode.change_tip')}
    onchange={change}
    onclick={(e) => e.stopPropagation()}
  >
    {#if fixed}<option value={svc.start_type}>{START_LABELS[svc.start_type]}</option>{/if}
    {#each MODE_OPTIONS as m (m)}
      <option value={m} disabled={svc.critical && (m === 'demand' || m === 'disabled')}>{START_LABELS[m]}</option>
    {/each}
  </select>
  {#if pending}<span class="spin sm"></span>{:else if svc.critical}<span class="lock" title={CRITICAL.tip}><Icon name="lock" size={13} /></span>{/if}
</span>
