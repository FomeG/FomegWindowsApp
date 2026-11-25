<script>
  import { store } from './store.svelte.js';
  import { t } from './i18n.svelte.js';
  let cancelBtn = $state();
  let box = $state();
  const c = $derived(store.confirmState);
  // focus nút Huỷ khi mở; trả focus về chỗ cũ khi đóng
  let prevFocus = null;
  $effect(() => {
    if (c && cancelBtn) {
      prevFocus = document.activeElement;
      cancelBtn.focus();
    } else if (!c && prevFocus) {
      const el = prevFocus;
      prevFocus = null;
      if (el.isConnected) el.focus({ preventScroll: true });
    }
  });
  function key(e) {
    if (!c) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      c.resolve(false);
    } else if (e.key === 'Tab' && box) {
      // giữ focus trong hộp thoại
      const nodes = [...box.querySelectorAll('button, input')];
      const i = nodes.indexOf(document.activeElement);
      e.preventDefault();
      nodes[(i + (e.shiftKey ? -1 : 1) + nodes.length) % nodes.length]?.focus();
    }
  }
</script>

<svelte:window onkeydown={key} />

{#if c}
  <div class="modal-backdrop" role="presentation" onclick={() => c.resolve(false)}></div>
  <div class="modal" bind:this={box} role="alertdialog" aria-modal="true" aria-labelledby="modal-title" aria-describedby="modal-msg">
    <h2 id="modal-title">{c.title || t('shell.confirm.title')}</h2>
    <p id="modal-msg">{c.message}</p>
    {#if c.checkLabel}
      <label class="check modal-check"><input type="checkbox" bind:checked={c.checked} /> {c.checkLabel}</label>
    {/if}
    <div class="modal-actions">
      <button type="button" class="btn {c.danger ? 'danger solid' : 'primary'}" onclick={() => c.resolve(true)}>{c.okLabel || t('shell.confirm.ok')}</button>
      <button type="button" class="btn" bind:this={cancelBtn} onclick={() => c.resolve(false)}>{t('shell.cancel')}</button>
    </div>
  </div>
{/if}
