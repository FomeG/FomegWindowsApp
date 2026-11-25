<script>
  // Dải "Có bản mới" ở đầu vùng nội dung (cùng kiểu .banner với dải "không có quyền Admin"); khi cài thì thành thanh
  // tiến trình 4 px kéo bằng transform (giống CleanupProgress), không có hoạt ảnh chạy liên tục.
  import { updater } from './update.svelte.js';
  import { store } from './store.svelte.js';
  import { fmtBytes } from './format.js';
  import { renderMarkdown } from './markdown.js';
  import { t } from './i18n.svelte.js';
  import Glyph from './Glyph.svelte';

  let notes = $state(false);
  const a = $derived(updater.avail);
  const ratio = $derived(updater.phase === 'install' ? 1 : updater.total ? Math.min(1, updater.got / updater.total) : 0);

  async function install() {
    // đang dọn dẹp / gỡ ứng dụng: hỏi lại, vì app sẽ đóng giữa chừng
    const ok = await store.confirm({
      title: t('update.confirm.title', { v: a.version }),
      message: t('update.confirm.message'),
      okLabel: t('update.btn.install'),
    });
    if (ok) updater.install();
  }
</script>

{#if updater.show && a}
  <div class="banner update-bar" role="status" aria-live="polite">
    {#if updater.phase === 'idle'}
      <Glyph name="download" size={14} />
      <span class="ub-text ellipsis">{t('update.bar.available', { v: a.version, cur: a.current })}</span>
      {#if a.notes}
        <button type="button" class="link-btn small" aria-expanded={notes} onclick={() => (notes = !notes)}>
          {t('update.bar.notes')} <Glyph name={notes ? 'chevronUp' : 'chevronDown'} size={10} />
        </button>
      {/if}
      <span class="grow"></span>
      <button type="button" class="btn xs" onclick={() => updater.skip()}>{t('update.btn.skip')}</button>
      <button type="button" class="btn xs" onclick={() => updater.later()}>{t('update.btn.later')}</button>
      <button type="button" class="btn xs primary" onclick={install}><Glyph name="download" size={11} /> {t('update.btn.install')}</button>
    {:else}
      <span class="cp-track" aria-hidden="true"><span class="cp-fill" style="transform:scaleX({ratio})"></span></span>
      <span class="ub-text ellipsis">
        {#if updater.phase === 'install'}
          {t('update.bar.installing', { v: a.version })}
        {:else if updater.total}
          {t('update.bar.downloading', { got: fmtBytes(updater.got), total: fmtBytes(updater.total) })}
        {:else}
          {t('update.bar.downloading_unknown', { got: fmtBytes(updater.got) })}
        {/if}
      </span>
    {/if}
  </div>
  {#if notes && updater.phase === 'idle' && a.notes}
    <div class="update-notes md">{@html renderMarkdown(a.notes)}</div>
  {/if}
{/if}
