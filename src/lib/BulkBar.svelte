<script>
  // Thanh thao tác hàng loạt nổi ở dưới khi đã chọn ≥ 2 mục (1 mục thì dùng nút trong ngăn chi tiết).
  import { t } from './i18n.svelte.js';
  let { count, children, onclear } = $props();
</script>

{#if count > 1}
  <div class="bulkbar" role="toolbar" aria-label={t('shell.bulk.bar')}>
    <span class="bulk-count">{t('shell.bulk.selected')} <b>{count}</b></span>
    <span class="sep"></span>
    {@render children()}
    <span class="sep"></span>
    <button type="button" class="btn ghost" onclick={onclear}>{t('shell.bulk.clear')}</button>
  </div>
{/if}
