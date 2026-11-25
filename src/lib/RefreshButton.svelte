<script>
  // Nút làm mới / quét lại dùng chung cho mọi trang (cùng kiểu, cùng màu, cùng chỗ: cuối nhóm nút bên phải tiêu đề,
  // trước nút hành động chính). `info`: chữ nhỏ bên trái (thời gian quét…). Đang chạy -> icon xoay + `busyLabel`.
  // Trang Khởi động / Dịch vụ / Tổng quan: <RefreshButton {...store.refreshProps} />.
  import Glyph from './Glyph.svelte';
  import { t } from './i18n.svelte.js';
  let { onclick, busy = false, disabled = false, label: labelProp, busyLabel = '', info = '', title = '' } = $props();
  const label = $derived(labelProp ?? t('shell.refresh'));
</script>

<span class="refresh">
  {#if info}<span class="faint small" title={title}>{info}</span>{/if}
  <button type="button" class="btn" {onclick} disabled={busy || disabled} title={title || label}>
    <span class="refresh-ico" class:spinning={busy}><Glyph name="refresh" size={12} /></span>
    {busy && busyLabel ? busyLabel : label}
  </button>
</span>
