<script>
  import { store } from './store.svelte.js';
  import { fmtMB, ramClass } from './format.js';
  import { t } from './i18n.svelte.js';
  let { id, shared = 0 } = $props();
  // Đọc theo từng key của SvelteMap: chỉ ô này cập nhật khi RAM của id đổi.
  const mb = $derived(store.ram.get(id) ?? 0);
  const text = $derived(fmtMB(mb));
</script>

<span class="ram {ramClass(mb)}" title={shared > 1 ? t('shell.ram.shared_tip', { n: shared }) : undefined}>
  {text}{#if shared > 1 && mb >= 0.5}<span class="muted"> · {t('shell.ram.shared', { n: shared })}</span>{/if}
</span>
