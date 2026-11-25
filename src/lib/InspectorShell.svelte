<script>
  // Khung chung của ngăn chi tiết: dòng nhỏ (kicker) + nút đóng, icon + tiêu đề + dòng phụ, hàng thao tác, thân cuộn.
  // Mọi ngăn chi tiết (tiến trình, mục khởi động, dịch vụ, tổng quan) dùng khung này để giống nhau.
  import Glyph from './Glyph.svelte';
  import { t } from './i18n.svelte.js';
  let {
    label,
    kicker,
    title,
    titleTip = '',
    accent = false, // icon nền màu nhấn (chế độ tổng quan)
    overlay = false,
    onclose,
    icon, // snippet: nội dung ô icon
    sub, // snippet: dòng phụ dưới tiêu đề
    actions, // snippet: hàng nút
    children, // thân
  } = $props();
</script>

<aside class="insp" aria-label={label}>
  <header class="insp-head">
    <div class="insp-kicker">
      <span class="ellipsis">{kicker}</span>
      <button type="button" class="icon-btn xs" title={overlay ? t('shell.insp.close_tip') : t('shell.insp.hide')} aria-label={overlay ? t('shell.close') : t('shell.insp.hide')} onclick={onclose}>
        <Glyph name={overlay ? 'close' : 'paneClose'} size={12} />
      </button>
    </div>
    <div class="insp-title">
      <span class="insp-icon" class:accent>{@render icon?.()}</span>
      <div class="min0">
        <h2 class="ellipsis" title={titleTip || title}>{title}</h2>
        {#if sub}<div class="muted small insp-sub">{@render sub()}</div>{/if}
      </div>
    </div>
    {#if actions}<div class="insp-actions">{@render actions()}</div>{/if}
  </header>
  <div class="insp-body">{@render children?.()}</div>
</aside>
