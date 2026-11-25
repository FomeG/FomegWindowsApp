<script>
  // Tab lọc kiểu RustWTM: chữ thường, tab đang chọn có nền nhẹ + gạch chân màu nhấn, kèm số đếm.
  // items: [[id, nhãn, số đếm?]]
  import { t } from './i18n.svelte.js';
  let { items, value, onchange, label } = $props();
</script>

<div class="ftabs" role="radiogroup" aria-label={label ?? t('shell.filter')}>
  {#each items as [id, text, count] (id)}
    <button type="button" class="ftab" class:active={value === id} role="radio" aria-checked={value === id} onclick={() => onchange(id)}>
      {text}{#if count != null}<span class="ftab-n">{count}</span>{/if}
    </button>
  {/each}
</div>
