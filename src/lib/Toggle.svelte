<script>
  // Công tắc bật/tắt: chính nó hiển thị trạng thái (xanh = đang bật, xám = đã tắt).
  import { t } from './i18n.svelte.js';
  let { on = false, pending = false, disabled = false, label = '', title = '', onchange } = $props();
</script>

<button
  type="button"
  role="switch"
  class="switch"
  class:on
  class:pending
  aria-checked={on}
  aria-busy={pending}
  aria-label={label}
  title={title || (pending ? t('shell.toggle.pending') : on ? t('shell.toggle.on') : t('shell.toggle.off'))}
  disabled={disabled || pending}
  onclick={(e) => { e.stopPropagation(); onchange?.(!on); }}
>
  <span class="knob">{#if pending}<span class="spin"></span>{/if}</span>
</button>
