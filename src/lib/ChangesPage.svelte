<script>
  // Trang "Có gì mới" (nút cuối thanh bên): nhật ký thay đổi của từng phiên bản theo ngôn ngữ đang chọn.
  // Nguồn: CHANGELOG.vi.md / CHANGELOG.md (Keep a Changelog), nhúng lúc build trong chunk riêng, tải khi mở trang
  // (changelog.js). Có bản mới chưa cài -> thẻ của bản đó (ghi chú từ latest.json) ở trên cùng, kèm nút Cập nhật.
  import { updater } from './update.svelte.js';
  import { loadChangelog, notesFor } from './changelog.js';
  import { renderMarkdown } from './markdown.js';
  import { APP_VERSION } from './version.js';
  import { t, i18n } from './i18n.svelte.js';
  import Glyph from './Glyph.svelte';

  /** null = đang tải */
  let changes = $state.raw(null);
  $effect(() => {
    const loc = i18n.locale;
    let live = true;
    loadChangelog(loc).then((c) => live && (changes = c));
    return () => (live = false);
  });
  const avail = $derived(updater.avail);
  const canCheck = $derived(!!updater.info?.enabled);
</script>

<section class="page changes-page">
  <header class="page-head">
    <div>
      <h1>{t('changes.title')}</h1>
      <p class="muted">
        {t('changes.sum', { v: APP_VERSION })}{#if changes?.length}<span class="dot-sep">|</span>{t('changes.count', { count: changes.length })}{/if}
      </p>
    </div>
    <div class="ph-actions">
      {#if canCheck}
        <button type="button" class="btn" disabled={updater.checking || updater.phase !== 'idle'} onclick={() => updater.check({ manual: true })}>
          {#if updater.checking}<span class="spin xs"></span>{:else}<Glyph name="refresh" size={13} />{/if}
          {t('update.set.check_now')}
        </button>
      {/if}
    </div>
  </header>

  <div class="set-body">
    <div class="set-col">
      {#if avail}
        <div class="opt-card cl-card cl-next">
          <div class="cl-head">
            <b class="mono">v{avail.version}</b>
            <span class="cl-tag new">{t('changes.not_installed')}</span>
            <span class="grow"></span>
            <button type="button" class="btn sm primary" disabled={updater.phase !== 'idle'} onclick={() => updater.install()}><Glyph name="download" size={12} /> {t('update.btn.install')}</button>
          </div>
          <div class="md cl-body">{@html renderMarkdown(notesFor(avail.notes, i18n.locale))}</div>
        </div>
      {/if}

      {#if !changes}
        <p class="faint small"><span class="spin xs"></span> {t('changes.loading')}</p>
      {:else if !changes.length}
        <p class="faint small">{t('changes.empty')}</p>
      {:else}
        {#each changes as c (c.version)}
          <div class="opt-card cl-card">
            <div class="cl-head">
              <b class="mono">v{c.version}</b>
              {#if c.version === APP_VERSION}<span class="cl-tag">{t('changes.current')}</span>{/if}
              <span class="grow"></span>
              {#if c.date}<span class="faint small mono">{c.date}</span>{/if}
            </div>
            {#if c.summary}<p class="cl-sum">{c.summary}</p>{/if}
            <div class="md cl-body">{@html renderMarkdown(c.body)}</div>
          </div>
        {/each}
      {/if}
    </div>
  </div>
</section>
