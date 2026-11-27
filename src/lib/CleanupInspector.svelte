<script>
  // Ngăn chi tiết trang Dọn dẹp (khung InspectorShell dùng chung).
  // - Có mục đang trỏ -> dung lượng dọn được, phần giữ lại, mức an toàn, các thư mục, kết quả lần dọn trước.
  // - Không có -> tổng quan: tổng tìm thấy / đã chọn, theo nhóm, lớn nhất (bấm để xem).
  import { cleanup, GROUP_LABELS, SAFETY, partsText } from './cleanup.svelte.js';
  import { procs } from './procs.svelte.js';
  import { C } from './colors.js';
  import { fmtBytes, fmtInt } from './format.js';
  import { t, i18n } from './i18n.svelte.js';
  import InspectorShell from './InspectorShell.svelte';
  import NavIcon from './NavIcon.svelte';
  import Glyph from './Glyph.svelte';
  import FsIcon from './FsIcon.svelte';
  import Toggle from './Toggle.svelte';
  import CleanupTree from './CleanupTree.svelte';

  let { item = null, overlay = false, onclose, onpick, onopen, onask } = $props();

  const fmtDate = (ms) => (ms ? new Date(ms).toLocaleString(i18n.intl, { dateStyle: 'short', timeStyle: 'short' }) : '–');
  const canPick = (it) => !(it.error && !it.paths.length && it.id !== 'recycle');
  const result = $derived(item ? cleanup.results.get(item.id) : null);

  // ---------- tổng quan ----------
  const vis = $derived(cleanup.visible);
  const groups = $derived(
    ['temp', 'system', 'cache', 'dev', 'other'].map((g) => {
      const list = vis.filter((it) => it.group === g);
      const col = { temp: C.cpu, system: C.mem, cache: C.gpu, dev: C.disk, other: C.net || C.disk };
      return [GROUP_LABELS[g], list.reduce((n, it) => n + it.bytes, 0), list.length, col[g]];
    }).filter((g) => g[2] > 0)
  );
  const groupMax = $derived(Math.max(1, ...groups.map((g) => g[1])));
  const biggest = $derived(vis.filter((it) => !it.pending).sort((a, b) => b.bytes - a.bytes).slice(0, 6));
  const chosen = $derived(cleanup.chosenItems);
</script>

{#if item}
  <InspectorShell label={t('cleanup.insp.label')} kicker={t('cleanup.insp.label')} title={item.name} titleTip={item.paths[0] || item.name} {overlay} {onclose}>
    {#snippet icon()}
      {@const img = (procs.iconV, item.icon ? procs.icons.get(item.icon) : null)}
      {#if img}<img src={img} width="20" height="20" alt="" />{:else}<FsIcon kind={item.id === 'recycle' ? 'recycle' : 'folder'} size={18} />{/if}
    {/snippet}
    {#snippet sub()}
      <span>{GROUP_LABELS[item.group]}</span>
      {#if item.kinds?.length}<span title={t('cleanup.insp.parts_tip')}>{partsText(item)}</span>{/if}
      <span class="safety {SAFETY[item.safety][1]}">{SAFETY[item.safety][0]}</span>
      {#if item.default_on}<span>{t('cleanup.insp.preselected')}</span>{/if}
    {/snippet}
    {#snippet actions()}
      <span class="insp-ctl">
        <Toggle on={cleanup.chosen.has(item.id)} disabled={!canPick(item) || !!cleanup.phase} label={t('cleanup.insp.pick_label', { name: item.name })} onchange={(v) => cleanup.toggle(item.id, v)} />
        <span class="small" class:muted={!cleanup.chosen.has(item.id)}>{cleanup.parts.has(item.id) ? t('cleanup.insp.will_clean_part') : cleanup.chosen.has(item.id) ? t('cleanup.insp.will_clean') : t('cleanup.insp.wont_clean')}</span>
      </span>
      <span class="insp-act-break"></span>
      <button type="button" class="btn sm" onclick={() => onopen(item.paths[0])} disabled={!item.paths.length}><Glyph name="folder" size={12} /> {t('cleanup.insp.open')}</button>
      <button type="button" class="btn sm icon-only" onclick={() => onask(item)} title={t('cleanup.insp.ask_tip')} aria-label={t('cleanup.insp.ask_label')}><Glyph name="chat" size={12} /></button>
    {/snippet}

    <section class="insp-card">
      <div class="ic-stats two">
        <div><span>{t('cleanup.col.bytes')}</span><b style="color:rgb({C.disk})">{item.pending ? t('cleanup.cell.measuring') : canPick(item) ? fmtBytes(cleanup.freeable(item)) : '–'}</b></div>
        <div><span>{t('cleanup.insp.files')}</span><b>{item.pending ? '–' : fmtInt(item.files)}</b></div>
      </div>
      {#each cleanup.retainedDetails(item) as detail}<div class="res-sub muted">{detail}</div>{/each}
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

    {#if item.paths.length && item.id !== 'recycle' && canPick(item)}
      <!-- chọn từng loại / từng thư mục, xem tệp bên trong, nút dọn dính ở đáy ngăn -->
      <CleanupTree {item} {onopen} />
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
        <div><b class:warn-text={chosen.length > 0}>{cleanup.chosenSize}</b><span>{t('cleanup.ov.chosen')}</span></div>
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
          {@const img = (procs.iconV, it.icon ? procs.icons.get(it.icon) : null)}
          <button type="button" class="res-row link-row" onclick={() => onpick(it)} title={t('cleanup.ov.view')}>
            <span class="ellipsis cl-big">{#if img}<img src={img} width="14" height="14" alt="" />{/if}{it.name}</span><span class="mono muted">{fmtBytes(it.bytes)}</span>
          </button>
        {/each}
      </section>
      <div class="faint small">
        {t('cleanup.ov.hint')}
      </div>
    {/if}
  </InspectorShell>
{/if}
