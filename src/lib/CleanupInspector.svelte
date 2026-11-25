<script>
  // Ngăn chi tiết trang Dọn dẹp (khung InspectorShell dùng chung).
  // - Có mục đang trỏ -> dung lượng, phần mới < 24 giờ, mức an toàn, giải thích, các thư mục, kết quả lần dọn trước.
  // - Không có -> tổng quan: tổng tìm thấy / đã chọn, theo nhóm, lớn nhất (bấm để xem).
  import { cleanup, GROUP_LABELS, SAFETY } from './cleanup.svelte.js';
  import { C } from './colors.js';
  import { fmtBytes, fmtInt } from './format.js';
  import { t, i18n } from './i18n.svelte.js';
  import InspectorShell from './InspectorShell.svelte';
  import NavIcon from './NavIcon.svelte';
  import Glyph from './Glyph.svelte';
  import FsIcon from './FsIcon.svelte';
  import Toggle from './Toggle.svelte';

  let { item = null, overlay = false, onclose, onpick, onopen, onask } = $props();

  const fmtDate = (ms) => (ms ? new Date(ms).toLocaleString(i18n.intl, { dateStyle: 'short', timeStyle: 'short' }) : '–');
  const canPick = (it) => !(it.error && !it.paths.length && it.id !== 'recycle');
  const result = $derived(item ? cleanup.results.get(item.id) : null);

  // ---------- tổng quan ----------
  const vis = $derived(cleanup.visible);
  const groups = $derived(
    ['temp', 'system', 'cache'].map((g) => {
      const list = vis.filter((it) => it.group === g);
      return [GROUP_LABELS[g], list.reduce((n, it) => n + it.bytes, 0), list.length, g === 'cache' ? C.gpu : g === 'temp' ? C.cpu : C.mem];
    })
  );
  const groupMax = $derived(Math.max(1, ...groups.map((g) => g[1])));
  const biggest = $derived(vis.filter((it) => !it.pending).sort((a, b) => b.bytes - a.bytes).slice(0, 6));
  const chosen = $derived(cleanup.chosenItems);
  const chosenBytes = $derived(chosen.reduce((n, it) => n + cleanup.freeable(it), 0));
</script>

{#if item}
  <InspectorShell label={t('cleanup.insp.label')} kicker={t('cleanup.insp.label')} title={item.name} titleTip={item.paths[0] || item.name} {overlay} {onclose}>
    {#snippet icon()}<FsIcon kind={item.id === 'recycle' ? 'recycle' : 'folder'} size={18} />{/snippet}
    {#snippet sub()}
      <span>{GROUP_LABELS[item.group]}</span>
      <span class="safety {SAFETY[item.safety][1]}">{SAFETY[item.safety][0]}</span>
      {#if item.default_on}<span>{t('cleanup.insp.preselected')}</span>{/if}
    {/snippet}
    {#snippet actions()}
      <span class="insp-ctl">
        <Toggle on={cleanup.chosen.has(item.id)} disabled={!canPick(item) || !!cleanup.phase} label={t('cleanup.insp.pick_label', { name: item.name })} onchange={(v) => cleanup.toggle(item.id, v)} />
        <span class="small" class:muted={!cleanup.chosen.has(item.id)}>{cleanup.chosen.has(item.id) ? t('cleanup.insp.will_clean') : t('cleanup.insp.wont_clean')}</span>
      </span>
      <span class="insp-act-break"></span>
      <button type="button" class="btn sm" onclick={() => onopen(item.paths[0])} disabled={!item.paths.length}><Glyph name="folder" size={12} /> {t('cleanup.insp.open')}</button>
      <button type="button" class="btn sm icon-only" onclick={() => onask(item)} title={t('cleanup.insp.ask_tip')} aria-label={t('cleanup.insp.ask_label')}><Glyph name="chat" size={12} /></button>
    {/snippet}

    <section class="insp-card">
      <div class="ic-stats two">
        <div><span>{t('cleanup.insp.size')}</span><b style="color:rgb({C.disk})">{item.pending ? t('cleanup.insp.measuring') : fmtBytes(item.bytes)}</b></div>
        <div><span>{t('cleanup.insp.files')}</span><b>{item.pending ? '–' : fmtInt(item.files)}</b></div>
      </div>
      {#if !item.pending && item.recent_files}
        <div class="res-sub muted">
          {t(cleanup.keepRecent ? 'cleanup.insp.recent_keep' : 'cleanup.insp.recent_del', { bytes: fmtBytes(item.recent_bytes), n: fmtInt(item.recent_files) })}
        </div>
      {/if}
      {#if item.newest}<div class="res-sub faint">{t('cleanup.insp.newest', { date: fmtDate(item.newest) })}</div>{/if}
    </section>

    <p class="small">{item.note}</p>
    {#if item.error}<p class="note warn"><Glyph name="warning" size={13} /> {item.error}</p>{/if}

    {#if result}
      <section class="insp-card res">
        <div class="res-head"><Glyph name="check" size={14} /><b>{t('cleanup.insp.last')}</b><span class="res-val">{fmtBytes(result.bytes)}</span></div>
        <div class="res-sub muted">
          {t('cleanup.insp.removed', { n: fmtInt(result.removed) })}{#if result.skipped} · {t('cleanup.insp.skipped', { n: fmtInt(result.skipped) })}{/if}{#if result.kept_recent} · {t('cleanup.insp.kept', { n: fmtInt(result.kept_recent) })}{/if}
        </div>
        {#if result.error}<div class="res-sub warn-text">{result.error}</div>{/if}
      </section>
    {/if}

    {#if item.paths.length}
      <div class="cmd-block">
        <div class="cmd-head"><span class="lbl">{item.paths.length > 1 ? t('cleanup.insp.folders', { n: fmtInt(item.paths.length) }) : t('cleanup.insp.folder')}</span></div>
        <div class="clean-paths">
          {#each item.paths as p (p)}
            <button type="button" class="link-row mono small" onclick={() => onopen(p)} title={t('cleanup.insp.open_tip')}><FsIcon kind="folder" size={12} /><span class="ellipsis grow">{p}</span><Glyph name="open" size={11} /></button>
          {/each}
        </div>
      </div>
    {/if}
  </InspectorShell>
{:else}
  <InspectorShell label={t('cleanup.ov.label')} kicker={t('cleanup.ov.kicker')} title={t('cleanup.ov.label')} accent {overlay} {onclose}>
    {#snippet icon()}<NavIcon name="cleanup" size={19} />{/snippet}
    {#snippet sub()}
      <span>{t('cleanup.ov.items', { count: vis.length, n: fmtInt(vis.length) })}</span>
      {#if cleanup.pendingCount}<span>{t('cleanup.ov.pending', { n: fmtInt(cleanup.pendingCount) })}</span>{/if}
    {/snippet}

    {#if !cleanup.scanId}
      {#each Array(3) as _, i (i)}<div class="insp-card sk-card"><span class="sk" style="width:60%"></span><span class="sk" style="width:85%"></span></div>{/each}
    {:else}
      <div class="insp-counts">
        <div><b>{fmtBytes(vis.reduce((n, it) => n + it.bytes, 0))}</b><span>{t('cleanup.ov.found')}</span></div>
        <div><b class:warn-text={chosen.length > 0}>{fmtBytes(chosenBytes)}</b><span>{t('cleanup.ov.chosen')}</span></div>
        <div><b>{fmtInt(chosen.length)}</b><span>{t('cleanup.ov.to_clean')}</span></div>
      </div>

      <section class="insp-card res">
        <div class="res-head"><Glyph name="list" size={14} /><b>{t('cleanup.ov.by_group')}</b></div>
        {#each groups as [label, bytes, n, col], gi (gi)}
          <div class="res-row"><span class="ellipsis">{label} <span class="faint">({fmtInt(n)})</span></span><span class="mono muted">{fmtBytes(bytes)}</span></div>
          <div class="res-bar thin"><span style="width:{(bytes / groupMax) * 100}%; background:rgb({col})"></span></div>
        {/each}
      </section>

      <section class="insp-card res">
        <div class="res-head"><Glyph name="disk" size={14} /><b>{t('cleanup.ov.biggest')}</b></div>
        {#each biggest as it (it.id)}
          <button type="button" class="res-row link-row" onclick={() => onpick(it)} title={t('cleanup.ov.view')}>
            <span class="ellipsis">{it.name}</span><span class="mono muted">{fmtBytes(it.bytes)}</span>
          </button>
        {/each}
      </section>
      <div class="faint small">
        {t('cleanup.ov.hint')}
      </div>
    {/if}
  </InspectorShell>
{/if}
