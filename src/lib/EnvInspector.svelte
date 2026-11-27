<script>
  // Ngăn chi tiết trang Biến môi trường (khung InspectorShell dùng chung).
  // - Biến danh sách (Path…): từng mục một dòng, đánh dấu ✓ còn / ⚠ không tồn tại / ⧉ trùng; sửa tại chỗ (bấm vào mục),
  //   lên / xuống / bỏ, thêm bằng ô nhập hoặc hộp chọn thư mục, "Bỏ các mục lỗi". Chỉ ghi khi bấm Lưu.
  // - Biến thường: ô văn bản + bản mở rộng (%…%). Biến bí mật (KEY, TOKEN…) bị che cho tới khi bấm Hiện.
  // - Thêm biến mới (adding). Không có gì -> tổng quan: số biến, sức khoẻ Path, biến cần xem, thay đổi gần đây (Hoàn tác).
  import { untrack } from 'svelte';
  import { env, SCOPE_LABELS, badEntry, entryKey, varId } from './env.svelte.js';
  import { ai } from './ai.svelte.js';
  import * as api from './api.js';
  import { fmtInt, copyText } from './format.js';
  import { t, i18n } from './i18n.svelte.js';
  import InspectorShell from './InspectorShell.svelte';
  import NavIcon from './NavIcon.svelte';
  import Glyph from './Glyph.svelte';

  let { item = null, adding = false, overlay = false, onclose, onpick, onask, ondelete, oncreated, oncanceladd } = $props();

  const MAX = 32766;
  const canWrite = (v) => v.scope === 'user' || env.canSystem;

  // ---------- sửa một biến (bản nháp, chỉ ghi khi bấm Lưu) ----------
  let draft = $state.raw([]); // biến danh sách: [{ id, raw }]
  let text = $state(''); // biến thường
  let reveal = $state(false);
  let editing = $state(-1);
  let editText = $state('');
  let addText = $state('');
  let fresh = $state.raw(new Map()); // mục mới thêm: đường dẫn (thường) -> còn tồn tại
  let seq = 0;

  function reset() {
    draft = item?.list ? item.entries.map((e) => ({ id: ++seq, raw: e.raw })) : [];
    text = item && !item.list ? item.value : '';
    editing = -1;
    addText = '';
  }
  // đổi biến / giá trị trên đĩa đổi (sau khi lưu) -> nháp mới
  const itemKey = $derived(item ? `${item.id}\u0000${item.value}` : '');
  let lastId = '';
  $effect(() => {
    itemKey;
    untrack(() => {
      if (item?.id !== lastId) reveal = false;
      lastId = item?.id || '';
      reset();
    });
  });

  const info = $derived(new Map((item?.entries || []).map((e) => [e.raw.toLowerCase(), e])));
  const rows = $derived.by(() => {
    const seen = new Set();
    return draft.map((d) => {
      const e = info.get(d.raw.toLowerCase());
      const expanded = e?.expanded ?? d.raw;
      const k = entryKey(expanded);
      const dup = seen.has(k);
      seen.add(k);
      return { ...d, expanded, exists: e ? e.exists : fresh.get(d.raw.toLowerCase()), other: !!e?.other, dup, isNew: !e };
    });
  });
  const badRows = $derived(rows.filter(badEntry));
  const joined = $derived(draft.map((d) => d.raw).join(';'));
  const dirty = $derived(!!item && (item.list ? joined !== item.entries.map((e) => e.raw).join(';') : text !== item.value));
  const length = $derived(item ? (item.list ? joined.length : text.length) : 0);
  const writable = $derived(!!item && canWrite(item));

  function move(i, d) {
    const j = i + d;
    if (j < 0 || j >= draft.length) return;
    const next = draft.slice();
    [next[i], next[j]] = [next[j], next[i]];
    draft = next;
  }
  function removeAt(i) {
    draft = draft.filter((_, k) => k !== i);
    editing = -1;
  }
  function startEdit(i) {
    if (!writable) return;
    editing = i;
    editText = draft[i].raw;
  }
  function commitEdit() {
    const i = editing;
    if (i < 0) return;
    const v = editText.trim().replace(/;+/g, '');
    editing = -1;
    if (!v) return removeAt(i);
    const next = draft.slice();
    next[i] = { ...next[i], raw: v };
    draft = next;
    check(v);
  }
  function editKey(e) {
    if (e.key === 'Enter') { e.preventDefault(); commitEdit(); }
    else if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); editing = -1; }
  }
  /** Mục mới: đường dẫn thường (không %…%) thì hỏi backend còn tồn tại không. */
  async function check(raw) {
    if (info.has(raw.toLowerCase()) || raw.includes('%') || !/^[a-z]:[\\/]/i.test(raw)) return;
    try {
      const ok = await api.duExists(raw);
      fresh = new Map(fresh).set(raw.toLowerCase(), !!ok);
    } catch {}
  }
  function addEntry(raw = addText) {
    const v = (raw || '').trim().replace(/;+/g, '');
    if (!v) return;
    draft = [...draft, { id: ++seq, raw: v }];
    addText = '';
    check(v);
  }
  async function pickFolder() {
    try {
      const p = await api.duPickFolder();
      if (p) addEntry(p);
    } catch {}
  }
  function removeBad() {
    const keep = new Set(rows.filter((r) => !badEntry(r)).map((r) => r.id));
    draft = draft.filter((d) => keep.has(d.id));
  }
  async function save() {
    if (!item || !dirty) return;
    try {
      await env.set(item.scope, item.name, item.list ? joined : text);
    } catch {}
  }
  const statusOf = (r) =>
    r.dup ? ['dup', 'copy', t('env.entry.dup')]
    : r.other ? ['dup', 'copy', t('env.entry.other')]
    : r.exists === false ? ['bad', 'warning', t('env.entry.missing')]
    : r.exists === true ? ['ok', 'check', t('env.entry.ok')]
    : r.isNew ? ['new', 'add', t('env.entry.new')]
    : ['na', 'info', t('env.entry.na')];

  // ---------- thêm biến ----------
  let nName = $state('');
  let nScope = $state('user');
  let nValue = $state('');
  const nTrim = $derived(nName.trim());
  const nExisting = $derived(nTrim ? env.find(nScope, nTrim) : null);
  const nError = $derived(!nTrim ? '' : nTrim.includes('=') || nName !== nName.trimStart() ? t('env.new.bad_name') : '');
  async function create() {
    if (!nTrim || nError || nExisting || (nScope === 'system' && !env.canSystem)) return;
    try {
      await env.set(nScope, nTrim, nValue);
      oncreated?.(varId(nScope, nTrim));
      nName = '';
      nValue = '';
    } catch {}
  }

  // ---------- tổng quan ----------
  const sysPath = $derived(env.find('system', 'Path'));
  const userPath = $derived(env.find('user', 'Path'));
  const pathEntries = $derived((sysPath?.entries.length || 0) + (userPath?.entries.length || 0));
  const pathLen = $derived((sysPath ? sysPath.expanded.length + 1 : 0) + (userPath ? userPath.expanded.length : 0));
  const pathBad = $derived([sysPath, userPath].reduce((n, v) => n + (v?.entries.filter((e) => e.exists === false).length || 0), 0));
  const pathDup = $derived([sysPath, userPath].reduce((n, v) => n + (v?.entries.filter((e) => e.dup || e.other).length || 0), 0));
  const needLook = $derived(env.vars.filter((v) => v.issues > 0).sort((a, b) => b.issues - a.issues).slice(0, 6));
  const fmtTime = (ms) => new Date(ms).toLocaleString(i18n.intl, { dateStyle: 'short', timeStyle: 'short' });
  const histText = (c) => (c.undo_of ? t('env.hist.undo', { name: c.name }) : !c.old ? t('env.hist.created', { name: c.name }) : !c.new ? t('env.hist.deleted', { name: c.name }) : t('env.hist.edited', { name: c.name }));
  function openVar(scope, name) {
    const v = env.find(scope, name);
    if (v) onpick(v);
  }
</script>

{#if adding}
  <InspectorShell label={t('env.new.title')} kicker={t('env.insp.kicker_new')} title={t('env.new.title')} accent {overlay} {onclose}>
    {#snippet icon()}<Glyph name="add" size={16} />{/snippet}
    {#snippet sub()}<span>{t('env.new.sub')}</span>{/snippet}

    <div class="env-form">
      <label class="env-field">
        <span class="lbl">{t('env.new.name')}</span>
        <input class="input mono" bind:value={nName} placeholder="JAVA_HOME" spellcheck="false" autocomplete="off" maxlength="255" onkeydown={(e) => e.key === 'Enter' && create()} />
      </label>
      {#if nError}<p class="note warn"><Glyph name="warning" size={13} /> {nError}</p>{/if}
      {#if nExisting}
        <p class="note warn"><Glyph name="info" size={13} /> {t('env.new.exists')} <button type="button" class="link-btn small" onclick={() => onpick(nExisting)}>{t('env.new.open_existing')}</button></p>
      {/if}
      <div class="env-field">
        <span class="lbl">{t('env.new.scope')}</span>
        <div class="seg" role="radiogroup" aria-label={t('env.new.scope')}>
          {#each ['user', 'system'] as s (s)}
            <button type="button" role="radio" aria-checked={nScope === s} class:active={nScope === s} onclick={() => (nScope = s)}><Glyph name={s === 'user' ? 'person' : 'monitor'} size={12} /> {SCOPE_LABELS[s]}</button>
          {/each}
        </div>
        <span class="faint small">{nScope === 'user' ? t('env.new.scope_user_desc') : t('env.new.scope_system_desc')}</span>
      </div>
      {#if nScope === 'system' && !env.canSystem}<p class="note warn"><Glyph name="admin" size={13} /> {t('env.need_admin')}</p>{/if}
      <label class="env-field">
        <span class="lbl">{t('env.new.value')}</span>
        <textarea class="input mono env-text" bind:value={nValue} rows="3" spellcheck="false" placeholder={String.raw`C:\Program Files\Java\jdk-21`}></textarea>
        <span class="faint small">{t('env.new.value_hint')}</span>
      </label>
      <div class="env-save">
        <span class="grow"></span>
        <button type="button" class="btn sm" onclick={() => oncanceladd?.()}>{t('shell.cancel')}</button>
        <button type="button" class="btn sm primary" disabled={!nTrim || !!nError || !!nExisting || env.saving || (nScope === 'system' && !env.canSystem)} onclick={create}><Glyph name="check" size={12} /> {t('env.new.create')}</button>
      </div>
    </div>
  </InspectorShell>
{:else if item}
  <InspectorShell label={t('env.insp.label')} kicker={item.scope === 'user' ? t('env.insp.kicker_user') : t('env.insp.kicker_system')} title={item.name} titleTip={item.name} {overlay} {onclose}>
    {#snippet icon()}<Glyph name={item.secret ? 'key' : item.list ? 'bullets' : 'document'} size={16} />{/snippet}
    {#snippet sub()}
      <span>{SCOPE_LABELS[item.scope]}</span>
      <span>{item.list ? t('env.insp.type_list', { count: item.entries.length, n: fmtInt(item.entries.length) }) : t('env.insp.type_text')}</span>
      {#if item.expand}<span title={t('env.insp.expand_tip')}>REG_EXPAND_SZ</span>{/if}
      {#if item.protected}<span title={t('env.protected_tip')}><Glyph name="lock" size={10} /> {t('env.insp.protected')}</span>{/if}
    {/snippet}
    {#snippet actions()}
      <button type="button" class="btn sm" onclick={() => copyText(item.value)}><Glyph name="copy" size={12} /> {t('env.insp.copy')}</button>
      <button type="button" class="btn sm danger" disabled={item.protected || !writable || env.saving} title={item.protected ? t('env.protected_tip') : ''} onclick={() => ondelete(item)}><Glyph name="delete" size={12} /> {t('env.insp.delete')}</button>
      <span class="insp-act-break"></span>
      <button type="button" class="btn sm icon-only" onclick={() => onask(item)} title={t('env.insp.ask_tip')} aria-label={t('env.insp.ask_tip')}><Glyph name="chat" size={12} /></button>
    {/snippet}

    {#if !writable}<p class="note warn"><Glyph name="admin" size={13} /> {t('env.need_admin')}</p>{/if}

    {#if item.list}
      <section class="insp-card">
        <div class="ic-stats two">
          <div><span>{t('env.insp.entries')}</span><b>{fmtInt(rows.length)}</b></div>
          <div><span>{t('env.insp.problems')}</span><b class:warn-text={badRows.length > 0}>{fmtInt(badRows.length)}</b></div>
        </div>
        <div class="res-sub muted">{t('env.insp.length', { n: fmtInt(length), max: fmtInt(MAX) })}</div>
      </section>

      {#if badRows.length}
        <p class="note warn env-bad">
          <Glyph name="warning" size={13} />
          <span class="grow">{t('env.insp.bad_hint', { count: badRows.length, n: fmtInt(badRows.length) })}</span>
          <button type="button" class="btn xs" disabled={!writable} onclick={removeBad}>{t('env.insp.remove_bad')}</button>
        </p>
      {/if}

      <ol class="env-entries" aria-label={t('env.insp.entries')}>
        {#each rows as r, i (r.id)}
          {@const st = statusOf(r)}
          <li class="env-entry {st[0]}">
            <span class="num faint mono">{i + 1}</span>
            <span class="st" title={st[2]}><Glyph name={st[1]} size={12} /></span>
            {#if editing === i}
              <!-- svelte-ignore a11y_autofocus -->
              <input class="input sm mono grow" bind:value={editText} onkeydown={editKey} onblur={commitEdit} autofocus spellcheck="false" aria-label={t('env.entry.edit')} />
            {:else}
              <button type="button" class="ent-text" disabled={!writable} onclick={() => startEdit(i)} title={(r.expanded !== r.raw ? `${r.raw}\n→ ${r.expanded}\n` : '') + st[2] + (writable ? `\n${t('env.entry.click_edit')}` : '')}>
                <span class="raw mono ellipsis">{r.raw}</span>
                {#if r.expanded !== r.raw}<span class="exp mono faint ellipsis">{r.expanded}</span>{/if}
              </button>
            {/if}
            {#if writable && editing !== i}
              <span class="ent-act">
                <button type="button" class="icon-btn xs" disabled={i === 0} onclick={() => move(i, -1)} title={t('env.entry.up')} aria-label={t('env.entry.up')}><Glyph name="chevronUp" size={10} /></button>
                <button type="button" class="icon-btn xs" disabled={i === rows.length - 1} onclick={() => move(i, 1)} title={t('env.entry.down')} aria-label={t('env.entry.down')}><Glyph name="chevronDown" size={10} /></button>
                <button type="button" class="icon-btn xs danger" onclick={() => removeAt(i)} title={t('env.entry.remove')} aria-label={t('env.entry.remove')}><Glyph name="close" size={10} /></button>
              </span>
            {/if}
          </li>
        {/each}
      </ol>

      {#if writable}
        <div class="env-add">
          <input class="input sm mono grow" bind:value={addText} placeholder={t('env.entry.add_ph')} spellcheck="false" onkeydown={(e) => e.key === 'Enter' && addEntry()} aria-label={t('env.entry.add_ph')} />
          <button type="button" class="btn sm" onclick={pickFolder} title={t('env.entry.pick_tip')}><Glyph name="folder" size={12} /> {t('env.entry.pick')}</button>
          <button type="button" class="btn sm" disabled={!addText.trim()} onclick={() => addEntry()}><Glyph name="add" size={12} /> {t('env.entry.add')}</button>
        </div>
      {/if}
    {:else}
      <div class="env-field">
        <span class="lbl">{t('env.insp.value')}</span>
        {#if item.secret && !reveal}
          <div class="env-secret">
            <span class="mono muted grow">••••••••••••</span>
            <span class="faint small">{t('env.insp.secret_len', { n: fmtInt(item.value.length) })}</span>
            <button type="button" class="btn xs" onclick={() => (reveal = true)}>{t('env.insp.reveal')}</button>
          </div>
          <span class="faint small">{t('env.secret_tip')}</span>
        {:else}
          <textarea class="input mono env-text" bind:value={text} rows={Math.min(8, Math.max(2, Math.ceil(text.length / 48)))} spellcheck="false" readonly={!writable} aria-label={t('env.insp.value')}></textarea>
          {#if item.expand || text.includes('%')}
            <span class="faint small mono env-exp" title={item.expanded}>→ {dirty ? t('env.insp.expand_after') : item.expanded}</span>
          {/if}
        {/if}
      </div>
    {/if}

    {#if dirty}
      <div class="env-save">
        <Glyph name="edit" size={12} />
        <span class="small grow">{t('env.insp.unsaved')}</span>
        <button type="button" class="btn sm" onclick={reset} disabled={env.saving}>{t('env.insp.discard')}</button>
        <button type="button" class="btn sm primary" onclick={save} disabled={env.saving || (item.name.toLowerCase() === 'path' && !(item.list ? joined : text).replace(/;/g, '').trim())}><Glyph name="check" size={12} /> {env.saving ? t('env.insp.saving') : t('env.insp.save')}</button>
      </div>
    {/if}
    <div class="faint small">{t('env.insp.hint')}</div>
  </InspectorShell>
{:else}
  <InspectorShell label={t('env.ov.label')} kicker={t('env.ov.kicker')} title={t('env.ov.label')} accent {overlay} {onclose}>
    {#snippet icon()}<NavIcon name="env" size={19} />{/snippet}
    {#snippet sub()}<span>{t('env.ov.sub', { count: env.vars.length, n: fmtInt(env.vars.length) })}</span>{/snippet}

    {#if !env.loaded}
      {#each Array(3) as _, i (i)}<div class="insp-card sk-card"><span class="sk" style="width:60%"></span><span class="sk" style="width:85%"></span></div>{/each}
    {:else}
      <div class="insp-counts">
        <div><b>{fmtInt(env.vars.filter((v) => v.scope === 'user').length)}</b><span>{t('env.ov.user')}</span></div>
        <div><b>{fmtInt(env.vars.filter((v) => v.scope === 'system').length)}</b><span>{t('env.ov.system')}</span></div>
        <div><b class:warn-text={env.issueCount > 0}>{fmtInt(env.issueCount)}</b><span>{t('env.ov.issues')}</span></div>
      </div>

      <section class="insp-card res">
        <div class="res-head"><Glyph name="bullets" size={14} /><b>{t('env.ov.path')}</b><span class="res-val">{t('env.ov.path_entries', { count: pathEntries, n: fmtInt(pathEntries) })}</span></div>
        <div class="res-row"><span class="muted">{t('env.ov.path_len')}</span><span class="mono muted">{fmtInt(pathLen)} / {fmtInt(MAX)}</span></div>
        <div class="res-bar thin"><span style="width:{Math.min(100, (pathLen / MAX) * 100)}%"></span></div>
        <div class="res-row"><span class="muted">{t('env.ov.path_missing')}</span><span class="mono" class:warn-text={pathBad > 0}>{fmtInt(pathBad)}</span></div>
        <div class="res-row"><span class="muted">{t('env.ov.path_dup')}</span><span class="mono" class:warn-text={pathDup > 0}>{fmtInt(pathDup)}</span></div>
        <div class="env-row-btns">
          {#if userPath}<button type="button" class="btn xs" onclick={() => openVar('user', 'Path')}>{t('env.ov.open_user_path')}</button>{/if}
          {#if sysPath}<button type="button" class="btn xs" onclick={() => openVar('system', 'Path')}>{t('env.ov.open_system_path')}</button>{/if}
          {#if pathBad + pathDup > 0}<button type="button" class="btn xs" onclick={() => ai.askEnv(t('ai.ask.env_clean'))}><Glyph name="chat" size={11} /> {t('env.ov.ai_clean')}</button>{/if}
        </div>
      </section>

      {#if needLook.length}
        <section class="insp-card res">
          <div class="res-head"><Glyph name="warning" size={14} /><b>{t('env.ov.need_look')}</b></div>
          {#each needLook as v (v.id)}
            <button type="button" class="res-row link-row" onclick={() => onpick(v)} title={t('env.ov.view')}>
              <span class="ellipsis">{v.name} <span class="faint">({SCOPE_LABELS[v.scope]})</span></span><span class="mono warn-text">{fmtInt(v.issues)}</span>
            </button>
          {/each}
        </section>
      {/if}

      <section class="insp-card res">
        <div class="res-head"><Glyph name="history" size={14} /><b>{t('env.ov.history')}</b></div>
        {#each env.history.slice(0, 8) as c (c.id)}
          <div class="res-row env-hist">
            <button type="button" class="link-row ellipsis" onclick={() => openVar(c.scope, c.name)} title={t('env.ov.view')}>
              {histText(c)} <span class="faint">· {SCOPE_LABELS[c.scope] || c.scope} · {fmtTime(c.at)}</span>
            </button>
            <button type="button" class="btn xs" disabled={env.saving || (c.scope === 'system' && !env.canSystem)} onclick={() => env.restore(c)} title={t('env.ov.undo_tip')}>{t('env.ov.undo')}</button>
          </div>
        {:else}
          <div class="faint small">{t('env.ov.no_history')}</div>
        {/each}
      </section>
      <div class="faint small">{t('env.ov.hint')}</div>
    {/if}
  </InspectorShell>
{/if}
