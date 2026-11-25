<script>
  // Một tuỳ chọn trên thẻ riêng (kiểu RustWTM / Windows Settings): icon, tiêu đề + mô tả mờ bên trái, điều khiển bên phải.
  // Khung hẹp (< 520 px, vd. panel trợ lý) thì điều khiển xuống dưới chữ.
  // Có `on` mà không có children = công tắc kèm chữ "Bật/Tắt".
  // `below` = phần phụ dưới chữ (trạng thái, nút nhỏ…). `soon` = chưa làm: nhãn "Sắp có" + công tắc bị khoá.
  import Glyph from './Glyph.svelte';
  import Toggle from './Toggle.svelte';
  import { t } from './i18n.svelte.js';

  let { glyph = 'settings', title, desc = '', on, pending = false, disabled = false, onchange, children, below, wide = false, soon = false } = $props();
</script>

<div class="opt-row" class:wide class:is-soon={soon}>
  <span class="opt-ico"><Glyph name={glyph} size={16} /></span>
  <div class="opt-text">
    <span class="opt-title">{title}{#if soon}<span class="soon">{t('shell.opt.soon')}</span>{/if}</span>
    {#if desc}<span class="opt-desc">{desc}</span>{/if}
    {@render below?.()}
  </div>
  <div class="opt-ctl">
    {#if children}
      {@render children()}
    {:else if on !== undefined}
      <span class="opt-state">{on ? t('shell.opt.on') : t('shell.opt.off')}</span>
      <Toggle {on} {pending} disabled={disabled || soon} label={title} title={soon ? t('shell.opt.soon') : disabled ? t('shell.opt.locked') : ''} {onchange} />
    {/if}
  </div>
</div>
