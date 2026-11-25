<script>
  // Ngăn chi tiết tab "Còn sót lại" của trang Ứng dụng (khung InspectorShell dùng chung).
  // - Có mục đang trỏ -> dung lượng, lần sửa cuối, vì sao bị coi là còn sót, chọn để dọn.
  // - Không có -> tổng quan: tổng còn sót / đã chọn, theo nơi, lớn nhất.
  import { apps, leftId, isStoreArea } from './apps.svelte.js';
  import { t, i18n } from './i18n.svelte.js';
  import { C } from './colors.js';
  import { fmtBytes, fmtInt } from './format.js';
  import InspectorShell from './InspectorShell.svelte';
  import NavIcon from './NavIcon.svelte';
  import Glyph from './Glyph.svelte';
  import FsIcon from './FsIcon.svelte';
  import Toggle from './Toggle.svelte';

  let { item = null, overlay = false, onclose, onpick, onopen, onask } = $props();

  const fmtDate = (ms) => (ms ? new Date(ms).toLocaleDateString(i18n.intl, { day: '2-digit', month: '2-digit', year: 'numeric' }) : '–');
  const on = $derived(!!item && apps.leftChosen.has(leftId(item)));

  // ---------- tổng quan ----------
  const all = $derived((apps.left || []).filter((f) => f.kind !== 'info'));
  const areas = $derived.by(() => {
    const m = new Map();
    for (const f of all) m.set(f.area, (m.get(f.area) || 0) + f.bytes);
    return [...m].sort((a, b) => b[1] - a[1]);
  });
  const areaMax = $derived(Math.max(1, ...areas.map((a) => a[1])));
  const biggest = $derived(all.slice().sort((a, b) => b.bytes - a.bytes).slice(0, 6));
  const chosen = $derived(all.filter((f) => apps.leftChosen.has(leftId(f))));
</script>

{#if item}
  <InspectorShell label={t('apps.left.insp.label')} kicker={item.area} title={item.owner} titleTip={item.path} {overlay} {onclose}>
    {#snippet icon()}<FsIcon kind="folder" size={18} />{/snippet}
    {#snippet sub()}
      <span>{item.kind === 'package' ? t('apps.left.insp.package') : item.kind === 'info' ? t('apps.left.insp.info') : t('apps.left.col.path')}</span>
      {#if item.sure}<span>{t('apps.left.insp.preselected')}</span>{/if}
    {/snippet}
    {#snippet actions()}
      {#if item.kind !== 'info'}
        <span class="insp-ctl">
          <Toggle {on} disabled={apps.leftBusy} label={t('apps.left.act.pick')} onchange={(v) => (v ? apps.leftChosen.add(leftId(item)) : apps.leftChosen.delete(leftId(item)))} />
          <span class="small" class:muted={!on}>{on ? t('apps.left.insp.will_clean') : t('apps.left.insp.wont_clean')}</span>
        </span>
        <span class="insp-act-break"></span>
      {/if}
      <button type="button" class="btn sm" onclick={() => onopen(item.path)}><Glyph name="folder" size={12} /> {t('apps.act.open_folder')}</button>
      <button type="button" class="btn sm icon-only" onclick={() => onask(item)} title={t('apps.left.act.ask_ai')} aria-label={t('apps.ai.ask_label')}><Glyph name="chat" size={12} /></button>
    {/snippet}

    <section class="insp-card">
      <div class="ic-stats two">
        <div><span>{t('apps.col.size')}</span><b style="color:rgb({C.disk})">{item.partial ? '≥ ' : ''}{fmtBytes(item.bytes)}</b></div>
        <div><span>{t('apps.left.insp.files')}</span><b>{fmtInt(item.files)}</b></div>
      </div>
      <div class="res-sub faint">{t('apps.left.insp.newest', { date: fmtDate(item.newest) })}</div>
    </section>

    <p class="small">{item.reason}.</p>
    {#if item.kind === 'package'}
      <p class="note warn"><Glyph name="warning" size={13} /> {t('apps.left.insp.package_note')}</p>
    {:else if item.kind === 'info'}
      <p class="note"><Glyph name="info" size={13} /> {t('apps.left.insp.info_note')}</p>
    {:else}
      <p class="faint small">{t('apps.left.insp.folder_note')}</p>
    {/if}

    <div class="cmd-block">
      <div class="cmd-head"><span class="lbl">{item.kind === 'package' ? t('apps.left.insp.package_folder') : t('apps.left.col.path')}</span></div>
      <div class="mono small break insp-path">{item.path}</div>
      {#if item.package}<div class="mono small break insp-path faint">{item.package}</div>{/if}
    </div>
    {#if !isStoreArea(item)}
      <button type="button" class="link-row small reg-link" onclick={() => apps.forget(item.owner)}><Glyph name="close" size={11} /><span class="ellipsis grow">{t('apps.left.insp.forget', { name: item.owner })}</span></button>
    {/if}
  </InspectorShell>
{:else}
  <InspectorShell label={t('apps.left.ov.label')} kicker={t('apps.left.ov.kicker')} title={t('apps.left.ov.title')} accent {overlay} {onclose}>
    {#snippet icon()}<NavIcon name="apps" size={19} />{/snippet}
    {#snippet sub()}<span>{t('apps.left.count', { n: fmtInt(all.length), count: all.length })}</span>{/snippet}

    {#if !apps.left}
      {#each Array(3) as _, i (i)}<div class="insp-card sk-card"><span class="sk" style="width:60%"></span><span class="sk" style="width:85%"></span></div>{/each}
    {:else}
      <div class="insp-counts">
        <div><b>{fmtBytes(all.reduce((n, f) => n + f.bytes, 0))}</b><span>{t('apps.left.ov.left')}</span></div>
        <div><b class:warn-text={chosen.length > 0}>{fmtBytes(chosen.reduce((n, f) => n + f.bytes, 0))}</b><span>{t('apps.left.ov.chosen')}</span></div>
        <div><b>{fmtInt(apps.leftInfo.removed_known)}</b><span>{t('apps.left.ov.remembered')}</span></div>
      </div>
      {#if areas.length}
        <section class="insp-card res">
          <div class="res-head"><Glyph name="list" size={14} /><b>{t('apps.left.ov.by_area')}</b></div>
          {#each areas as [label, bytes] (label)}
            <div class="res-row"><span class="ellipsis">{label}</span><span class="mono muted">{fmtBytes(bytes)}</span></div>
            <div class="res-bar thin"><span style="width:{(bytes / areaMax) * 100}%"></span></div>
          {/each}
        </section>
        <section class="insp-card res">
          <div class="res-head"><Glyph name="disk" size={14} /><b>{t('apps.ov.biggest')}</b></div>
          {#each biggest as f (leftId(f))}
            <button type="button" class="res-row link-row" onclick={() => onpick(f)} title={f.path}>
              <span class="ellipsis">{f.owner} <span class="faint">· {f.area}</span></span><span class="mono muted">{fmtBytes(f.bytes)}</span>
            </button>
          {/each}
        </section>
      {/if}
      <div class="faint small">
        {t('apps.left.ov.hint')}
      </div>
    {/if}
  </InspectorShell>
{/if}
