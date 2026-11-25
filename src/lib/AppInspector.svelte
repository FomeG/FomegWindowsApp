<script>
  // Ngăn chi tiết trang Ứng dụng (khung InspectorShell dùng chung).
  // - Có app đang trỏ -> nút Gỡ cài đặt / Sửa đổi (như Windows), dung lượng đo được và khai báo, thông tin, thư mục, lệnh gỡ.
  // - Không có -> tổng quan: số app, tổng dung lượng, theo loại, lớn nhất (bấm để xem).
  import { untrack } from 'svelte';
  import * as api from './api.js';
  import { apps, ago, KIND_LABELS, SCOPE_LABELS } from './apps.svelte.js';

  import { procs } from './procs.svelte.js';
  import { toasts } from './toasts.svelte.js';
  import { t, i18n } from './i18n.svelte.js';
  import { C } from './colors.js';
  import { fmtBytes, fmtInt, copyText, errText } from './format.js';
  import InspectorShell from './InspectorShell.svelte';
  import NavIcon from './NavIcon.svelte';
  import Glyph from './Glyph.svelte';

  let { app = null, overlay = false, onclose, onpick, onask } = $props();

  const fmtDay = (ms) => (ms ? new Date(ms).toLocaleDateString(i18n.intl, { day: '2-digit', month: '2-digit', year: 'numeric' }) : '–');
  const img = $derived((procs.iconV, app?.icon ? procs.icons.get(app.icon) : null));
  const sz = $derived((apps.sizeV, app ? apps.size(app) : null));
  const removing = $derived(!!app && apps.removing === app.id);

  // thư mục liên quan: tìm khi dừng ở một app ~250 ms (lướt phím mũi tên qua nhiều dòng không gọi backend)
  $effect(() => {
    const a = app;
    if (!a) return;
    const t = setTimeout(() => untrack(() => apps.loadRelated(a)), 250);
    return () => clearTimeout(t);
  });
  const rel = $derived((apps.relV, app ? apps.related.get(app.id) : undefined));
  const relTotal = $derived(rel ? rel.reduce((n, f) => n + f.bytes, 0) : 0);
  async function openPath(p) {
    try { await api.duOpen(p, true); } catch (e) { toasts.error(t('apps.toast.open_folder_failed'), [errText(e)]); }
  }

  /** okKey: khoá thông báo "Đã sao chép …" */
  async function copy(text, okKey) {
    if (text && (await copyText(text))) toasts.success(t(okKey));
    else toasts.error(t('apps.toast.copy_failed'));
  }

  // ---------- tổng quan ----------
  const sizeOf = (a) => apps.size(a)?.bytes ?? 0;
  const groups = $derived.by(() => {
    apps.sizeV;
    return [['win32', C.cpu], ['store', C.gpu]].map(([k, col]) => {
      const list = apps.list.filter((a) => a.kind === k);
      return [KIND_LABELS[k], list.reduce((n, a) => n + sizeOf(a), 0), list.length, col];
    });
  });
  const groupMax = $derived(Math.max(1, ...groups.map((g) => g[1])));
  const biggest = $derived((apps.sizeV, apps.list.slice().sort((a, b) => sizeOf(b) - sizeOf(a)).slice(0, 6)));
  const total = $derived(groups.reduce((n, g) => n + g[1], 0));
  const recent = $derived(apps.list.filter((a) => a.installed > Date.now() - 30 * 864e5).length);
</script>

{#if app}
  <InspectorShell label={t('apps.insp.label')} kicker={KIND_LABELS[app.kind]} title={app.name} titleTip={app.name} {overlay} {onclose}>
    {#snippet icon()}
      {#if img}<img src={img} width="28" height="28" alt="" />{:else}<NavIcon name="apps" size={19} />{/if}
    {/snippet}
    {#snippet sub()}
      {#if app.publisher}<span>{app.publisher}</span>{/if}
      {#if app.version}<span class="mono">{app.version}</span>{/if}
      <span>{fmtDay(app.installed)}</span>
    {/snippet}
    {#snippet actions()}
      <button type="button" class="btn sm danger" onclick={() => apps.uninstall(app)} disabled={!app.can_uninstall || !!apps.removing} title={app.can_uninstall ? t('apps.insp.uninstall_tip') : t('apps.insp.no_uninstall_tip')}>
        {#if removing}<span class="spin sm" aria-hidden="true"></span> {t('apps.act.uninstalling')}{:else}<Glyph name="delete" size={12} /> {t('apps.act.uninstall')}{/if}
      </button>
      {#if app.can_modify}<button type="button" class="btn sm" onclick={() => apps.modify(app)}><Glyph name="edit" size={12} /> {t('apps.act.modify')}</button>{/if}
      <span class="insp-act-break"></span>
      <button type="button" class="btn sm" onclick={() => apps.open(app, 'folder')} disabled={!app.location}><Glyph name="folder" size={12} /> {t('apps.act.open_folder')}</button>
      <button type="button" class="btn sm icon-only" onclick={() => onask(app)} title={t('apps.act.ask_ai_app')} aria-label={t('apps.ai.ask_label')}><Glyph name="chat" size={12} /></button>
    {/snippet}

    <section class="insp-card">
      <div class="ic-stats two">
        <div>
          <span>{t('apps.insp.install_folder')}</span>
          <b style="color:rgb({C.disk})">{#if sz?.measured}{sz.partial ? '≥ ' : ''}{fmtBytes(sz.bytes)}{:else if apps.measuring && app.location}{t('apps.size.measuring')}{:else}–{/if}</b>
        </div>
        <div><span>{t('apps.insp.declared')}</span><b>{app.est_bytes ? fmtBytes(app.est_bytes) : '–'}</b></div>
      </div>
      {#if sz?.measured}<div class="res-sub muted">{t('apps.insp.files', { n: fmtInt(sz.files), count: sz.files })}{sz.partial ? ` · ${t('apps.insp.partial')}` : ''}</div>{/if}
    </section>

    <section class="insp-card res">
      <div class="res-head"><Glyph name="folder" size={14} /><b>{t('apps.insp.related')}</b><span class="res-val">{rel?.length ? fmtBytes(relTotal) : ''}</span></div>
      {#if !rel}
        <div class="res-sub faint"><span class="spin xs"></span> {t('apps.insp.related_searching')}</div>
      {:else if !rel.length}
        <div class="res-sub faint">{t('apps.insp.related_none')}</div>
      {:else}
        {#each rel as f (f.path)}
          <button type="button" class="res-row link-row rel-row" onclick={() => openPath(f.path)} title={`${f.path}\n${f.area} · ${f.reason}`}>
            <span class="rel-main">
              <span class="ellipsis mono small">{f.path}</span>
              <span class="small ellipsis" class:faint={!f.shared} class:warn-text={!!f.shared}>{f.area}{f.shared ? ` · ${t('apps.insp.shared_with', { name: f.shared })}` : ''}</span>
            </span>
            <span class="mono muted small">{f.partial ? '≥ ' : ''}{fmtBytes(f.bytes)}</span>
          </button>
        {/each}
        {#if app.kind !== 'store'}<div class="res-sub faint">{t('apps.insp.related_hint')}</div>{/if}
      {/if}
    </section>

    <p class="small">
      {#if app.kind === 'store'}
        {t('apps.insp.store_note')}
      {:else}
        {t('apps.insp.win32_note')}
      {/if}
    </p>
    {#if !app.can_uninstall}<p class="note warn"><Glyph name="lock" size={13} /> {t('apps.insp.blocked_note')}</p>{/if}
    {#if app.location_guessed}<p class="note"><Glyph name="info" size={13} /> {t('apps.insp.guessed_note')}</p>{/if}

    <dl class="props insp-props">
      <dt>{t('apps.col.kind')}</dt><dd>{KIND_LABELS[app.kind]}{app.msi ? ' (Windows Installer)' : ''}</dd>
      <dt>{t('apps.insp.scope')}</dt><dd>{SCOPE_LABELS[app.scope]}</dd>
      {#if app.arch}<dt>{t('apps.insp.arch')}</dt><dd>{app.arch}</dd>{/if}
      <dt>{t('apps.col.publisher')}</dt><dd>{app.publisher || '–'}</dd>
      <dt>{t('apps.col.version')}</dt><dd class="mono">{app.version || '–'}</dd>
      <dt>{t('apps.col.installed')}</dt><dd>{fmtDay(app.installed)}</dd>
      <dt>{t('apps.col.used')}</dt>
      <dd>
        {#if app.last_used}{fmtDay(app.last_used)} ({ago(app.last_used)}){:else}{apps.usageFull ? t('apps.insp.not_used') : t('apps.insp.unknown_admin')}{/if}{#if app.launches} · {t('apps.insp.launches', { n: fmtInt(app.launches), count: app.launches })}{/if}
      </dd>
      {#if app.url}<dt>{t('apps.insp.website')}</dt><dd class="ellipsis" title={app.url}>{app.url}</dd>{/if}
    </dl>

    {#if app.location}
      <div class="cmd-block">
        <div class="cmd-head">
          <span class="lbl">{t('apps.insp.install_folder')}</span>
          <button type="button" class="icon-btn xs" title={t('apps.insp.copy_path')} aria-label={t('apps.insp.copy_path')} onclick={() => copy(app.location, 'apps.toast.copied_path')}><Glyph name="copy" size={12} /></button>
        </div>
        <div class="mono small break insp-path">{app.location}</div>
      </div>
    {/if}
    <div class="cmd-block">
      <div class="cmd-head">
        <span class="lbl">{app.kind === 'store' ? t('apps.insp.package_name') : t('apps.insp.uninstall_cmd')}</span>
        <button type="button" class="icon-btn xs" title={t('apps.insp.copy')} aria-label={t('apps.insp.copy')} onclick={() => copy(app.kind === 'store' ? app.key : app.command, app.kind === 'store' ? 'apps.toast.copied_package' : 'apps.toast.copied_uninstall')}><Glyph name="copy" size={12} /></button>
      </div>
      <pre class="mono">{app.kind === 'store' ? app.key : app.command}</pre>
    </div>
    {#if app.kind === 'win32'}
      <button type="button" class="link-row small reg-link" onclick={() => apps.open(app, 'registry')} title={app.key}><Glyph name="open" size={11} /><span class="ellipsis grow">{t('apps.insp.open_registry_key')}</span></button>
    {/if}
  </InspectorShell>
{:else}
  <InspectorShell label={t('apps.ov.label')} kicker={t('apps.ov.kicker')} title={t('apps.table_label')} accent {overlay} {onclose}>
    {#snippet icon()}<NavIcon name="apps" size={19} />{/snippet}
    {#snippet sub()}
      <span>{t('apps.count', { n: fmtInt(apps.list.length), count: apps.list.length })}</span>
      {#if apps.measuring}<span>{t('apps.ov.measuring', { n: fmtInt(apps.measuring) })}</span>{/if}
    {/snippet}

    {#if !apps.loaded}
      {#each Array(3) as _, i (i)}<div class="insp-card sk-card"><span class="sk" style="width:60%"></span><span class="sk" style="width:85%"></span></div>{/each}
    {:else}
      <div class="insp-counts">
        <div><b>{fmtInt(apps.list.length)}</b><span>{t('apps.ov.apps')}</span></div>
        <div><b>{fmtBytes(total)}</b><span>{t('apps.col.size')}</span></div>
        <div><b>{fmtInt(recent)}</b><span>{t('apps.ov.recent')}</span></div>
      </div>

      <section class="insp-card res">
        <div class="res-head"><Glyph name="list" size={14} /><b>{t('apps.ov.by_kind')}</b></div>
        {#each groups as [label, bytes, n, col] (label)}
          <div class="res-row"><span class="ellipsis">{label} <span class="faint">({fmtInt(n)})</span></span><span class="mono muted">{fmtBytes(bytes)}</span></div>
          <div class="res-bar thin"><span style="width:{(bytes / groupMax) * 100}%; background:rgb({col})"></span></div>
        {/each}
      </section>

      <section class="insp-card res">
        <div class="res-head"><Glyph name="disk" size={14} /><b>{t('apps.ov.biggest')}</b></div>
        {#each biggest as a (a.id)}
          <button type="button" class="res-row link-row" onclick={() => onpick(a)} title={t('apps.ov.view')}>
            <span class="ellipsis">{a.name}</span><span class="mono muted">{fmtBytes(sizeOf(a))}</span>
          </button>
        {/each}
      </section>
      <div class="faint small">
        {t('apps.ov.hint')}
      </div>
    {/if}
  </InspectorShell>
{/if}
