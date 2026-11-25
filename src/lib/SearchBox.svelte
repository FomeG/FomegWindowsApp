<script>
  import { untrack } from 'svelte';
  // Ô tìm kiếm có debounce (~120 ms). Ctrl+F để focus, Esc để xoá.
  import Icon from './Icon.svelte';
  import { t } from './i18n.svelte.js';
  let { value = '', placeholder, onsearch } = $props();
  const ph = $derived(placeholder ?? t('shell.search.placeholder'));
  let text = $state(untrack(() => value));
  let el = $state();
  let timer = 0;
  function input() {
    clearTimeout(timer);
    timer = setTimeout(() => onsearch(text.trim().toLowerCase()), 120);
  }
  function key(e) {
    if (e.key === 'Escape' && text) {
      e.stopPropagation();
      text = '';
      clearTimeout(timer);
      onsearch('');
    }
  }
  function winKey(e) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'f') {
      e.preventDefault();
      el?.focus();
      el?.select();
    }
  }
</script>

<svelte:window onkeydown={winKey} />

<label class="search">
  <Icon name="search" size={15} />
  <input bind:this={el} type="search" bind:value={text} oninput={input} onkeydown={key} placeholder={ph} aria-label={ph} spellcheck="false" />
  {#if text}
    <button type="button" class="icon-btn xs" title={t('shell.search.clear_tip')} aria-label={t('shell.search.clear')} onclick={() => { text = ''; onsearch(''); el?.focus(); }}>
      <Icon name="close" size={13} />
    </button>
  {:else}
    <kbd>Ctrl F</kbd>
  {/if}
</label>
