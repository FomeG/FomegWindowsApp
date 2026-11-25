<script>
  // Trang Cài đặt kiểu RustWTM: thanh mục ở trên, mỗi tuỳ chọn một thẻ (icon · tiêu đề + mô tả · điều khiển bên phải).
  // Mỗi dòng phải là một tuỳ chọn thật; cái chưa làm thì để `soon` ("Sắp có", điều khiển bị khoá).
  // Nhãn đều qua t(): bảng tab / phím tắt / lựa chọn là $derived để đổi ngôn ngữ là đổi ngay.
  import { onMount } from 'svelte';
  import { store } from './store.svelte.js';
  import { prefs, loadPref, savePref } from './prefs.svelte.js';
  import { ai, PROVIDERS } from './ai.svelte.js';
  import { sys } from './sys.svelte.js';
  import * as api from './api.js';
  import { toasts } from './toasts.svelte.js';
  import { APP_VERSION } from './version.js';
  import { fmtGB, fmtBytes, copyText, errText } from './format.js';
  import { t, i18n, LOCALES } from './i18n.svelte.js';
  import { MCP_CLIENTS } from './mcpClients.js';
  import { updater } from './update.svelte.js';
  import Glyph from './Glyph.svelte';
  import FilterTabs from './FilterTabs.svelte';
  import OptionRow from './OptionRow.svelte';
  import AiSettings from './AiSettings.svelte';
  import IntervalSelect from './IntervalSelect.svelte';

  const TAB_IDS = ['look', 'perf', 'pages', 'ai', 'mcp', 'win', 'keys', 'about'];
  const TABS = $derived(TAB_IDS.map((id) => [id, t(`settings.tab.${id}`)]));
  const IDLE = $derived([[5000, t('settings.idle.5s')], [15000, t('settings.idle.15s')], [60000, t('settings.idle.1m')], [0, t('settings.idle.off')]]);
  const STALE = [30, 60, 90, 180, 365];
  // [nhóm, [[khoá-phím, mô tả]]]: khoá-phím có chữ (nhấp đúp, Phím Menu) cũng dịch
  const KEYS = $derived([
    [t('settings.keys.g.general'), [
      ['Ctrl + J', t('settings.keys.ai')],
      ['F5 · Ctrl + R', t('settings.keys.rescan')],
      ['Ctrl + F', t('settings.keys.search')],
      ['Esc', t('settings.keys.esc')],
    ]],
    [t('settings.keys.g.table'), [
      ['↑ ↓ · PgUp PgDn · Home End', t('settings.keys.move')],
      [t('settings.keys.open_k'), t('settings.keys.open')],
      ['Space', t('settings.keys.tick')],
      ['Ctrl + A', t('settings.keys.all')],
      [t('settings.keys.multi_k'), t('settings.keys.multi')],
      [t('settings.keys.menu_k'), t('settings.keys.menu')],
    ]],
    [t('settings.keys.g.ai'), [
      ['Enter', t('settings.keys.send')],
      ['Shift + Enter', t('settings.keys.newline')],
    ]],
  ]);

  const saved = loadPref('ui:settingsTab', 'look');
  let tab = $state(TAB_IDS.includes(saved) ? saved : 'look');
  let mcp = $state(null);
  let client = $state(MCP_CLIENTS.some((c) => c.id === loadPref('ui:mcpClient', '')) ? loadPref('ui:mcpClient', '') : MCP_CLIENTS[0].id);
  let askKill = $state(!loadPref('proc:noAskKill', false));
  let relaunching = $state(false);
  const cl = $derived(MCP_CLIENTS.find((c) => c.id === client));
  const info = $derived(sys.info);
  const dataDir = $derived((store.settings.log_path || '').replace(/[\\/][^\\/]*$/, ''));
  const LOCAL_DIR = '%LOCALAPPDATA%\\fomeg-windows-app';

  onMount(() => {
    api.mcpInfo().then((r) => (mcp = r)).catch(() => {});
    if (!ai.cfg && !ai.loadError) ai.loadConfig();
  });

  function pick(id) {
    tab = id;
    savePref('ui:settingsTab', id);
  }
  function pickClient(id) {
    client = id;
    savePref('ui:mcpClient', id);
  }
  async function copy(text, what) {
    if (await copyText(text)) toasts.success(t('settings.copied', { what }));
    else toasts.error(t('settings.copy_failed'));
  }
  function setTheme(v) {
    if (prefs.theme !== v) prefs.toggleTheme();
  }
  function setAskKill(v) {
    askKill = v;
    savePref('proc:noAskKill', !v);
  }
  function setStartupAll(v) {
    prefs.set('startupAll', v);
    store.startupUi.kinds = v ? [] : ['Run', 'Folder'];
  }
  async function relaunch() {
    relaunching = true;
    try {
      await api.relaunchAdmin();
    } catch (e) {
      toasts.error(t('settings.admin.relaunch_failed'), [errText(e)]);
    }
    relaunching = false;
  }

  const day = (ms) => (ms > 0 ? new Date(ms).toLocaleDateString(i18n.intl) : '–');
  const dayTime = (ms) => (ms > 0 ? new Date(ms).toLocaleString(i18n.intl, { hour: '2-digit', minute: '2-digit', day: '2-digit', month: '2-digit', year: 'numeric' }) : '–');
  function since(ms, now) {
    const m = Math.max(0, Math.floor((now - ms) / 60000));
    const d = Math.floor(m / 1440), h = Math.floor((m % 1440) / 60);
    return d ? t('common.dur.dh', { d, h }) : h ? t('common.dur.hm', { h, m: m % 60 }) : t('common.dur.m', { m });
  }
  const screenText = () => t('settings.m.screen_val', { w: Math.round(screen.width * devicePixelRatio), h: Math.round(screen.height * devicePixelRatio), zoom: Math.round(devicePixelRatio * 100) });
  const memText = (i) => `${fmtGB(i.mem_total)}${i.mem_slots ? ` · ${t('settings.m.slots', { used: i.mem_modules.length, total: i.mem_slots })}` : ''}`;
  const ghz = (mhz) => (mhz / 1000).toLocaleString(i18n.intl, { maximumFractionDigits: 2 });

  /** Toàn bộ cấu hình máy dạng văn bản (để gửi khi cần hỗ trợ). */
  function machineText(i) {
    const L = [
      t('settings.mt.machine', { system: i.system || '–', host: i.hostname }),
      t('settings.mt.board', { board: i.board || '–', bios: i.bios || '–' }),
      t('settings.mt.windows', { os: i.os, date: day(i.os_installed_ms) }),
      t('settings.mt.cpu', { name: i.cpu_name, cores: i.cores, threads: i.logical, mhz: i.base_mhz }),
      `RAM: ${memText(i)}`,
      ...i.mem_modules.map((m) => `  ${m.slot}: ${fmtBytes(m.size)} ${m.kind} ${m.form} ${m.speed ? m.speed + ' MT/s' : ''} ${m.maker} ${m.part}`.replace(/\s+/g, ' ').trimEnd()),
      ...(i.gpus || []).map((g) => `GPU: ${g.name} · VRAM ${fmtBytes(g.ded_total)}`),
      ...i.disks.map((d) => t('settings.mt.disk', { id: d.id, model: d.model || d.name, size: fmtBytes(d.size), type: d.ssd ? 'SSD' : 'HDD' })),
      t('settings.mt.screen', { screen: screenText() }),
      `App: Vietnamese's Ultimate W App ${APP_VERSION}`,
    ];
    return L.join('\n');
  }
</script>

<section class="page settings-page">
  <header class="page-head">
    <div>
      <h1>{t('settings.title')}</h1>
      <p class="muted">{t(`settings.sum.${tab}`)}</p>
    </div>
  </header>

  <div class="toolbar wtm-bar">
    <FilterTabs items={TABS} value={tab} onchange={pick} label={t('settings.tabs_label')} />
  </div>

  <div class="set-body">
    <div class="set-col">
      {#if tab === 'look'}
        <h2 class="opt-group">{t('settings.look.group')}</h2>
        <OptionRow glyph={prefs.theme === 'dark' ? 'moon' : 'sun'} title={t('settings.look.theme')} desc={t('settings.look.theme_desc')}>
          <div class="seg" role="radiogroup" aria-label={t('settings.look.theme')}>
            <button type="button" role="radio" aria-checked={prefs.theme === 'dark'} class:active={prefs.theme === 'dark'} onclick={() => setTheme('dark')}><Glyph name="moon" size={12} /> {t('settings.look.dark')}</button>
            <button type="button" role="radio" aria-checked={prefs.theme === 'light'} class:active={prefs.theme === 'light'} onclick={() => setTheme('light')}><Glyph name="sun" size={12} /> {t('settings.look.light')}</button>
          </div>
        </OptionRow>
        <OptionRow glyph="chat" title={t('settings.look.lang')} desc={t('settings.look.lang_desc')}>
          <select class="inp" aria-label={t('settings.look.lang')} value={i18n.locale} onchange={(e) => i18n.set(e.currentTarget.value)}>
            {#each LOCALES as [id, name] (id)}<option value={id}>{name}</option>{/each}
          </select>
        </OptionRow>

        <h2 class="opt-group">{t('settings.look.layout')}</h2>
        <OptionRow glyph="sidebarLeft" title={t('settings.look.collapse')} desc={t('settings.look.collapse_desc')}
          on={prefs.collapsed} onchange={(v) => prefs.set('collapsed', v)} />
        <OptionRow glyph="chat" title={t('settings.look.ai_width')} desc={t('settings.look.ai_width_desc')}>
          <span class="mono small muted">{prefs.aiWidth} px</span>
          <button type="button" class="btn sm" disabled={prefs.aiWidth === 380} onclick={() => prefs.set('aiWidth', 380)}>{t('settings.default')}</button>
        </OptionRow>
      {:else if tab === 'perf'}
        <h2 class="opt-group">{t('settings.perf.live')}</h2>
        <OptionRow glyph="speed" title={t('settings.perf.interval')} desc={t('settings.perf.interval_desc')}>
          <IntervalSelect />
        </OptionRow>
        <OptionRow glyph="pause" title={t('settings.perf.idle')} desc={t('settings.perf.idle_desc')}>
          <select class="inp" aria-label={t('settings.perf.idle_label')} value={prefs.idleMs} onchange={(e) => sys.setIdle(Number(e.currentTarget.value))}>
            {#each IDLE as [ms, label] (ms)}<option value={ms}>{label}</option>{/each}
          </select>
        </OptionRow>
        <OptionRow glyph="history" title={t('settings.perf.history')} desc={t('settings.perf.history_desc')} soon>
          <select class="inp" aria-label={t('settings.perf.history')} disabled><option>{t('settings.perf.history_1h')}</option></select>
        </OptionRow>

        <h2 class="opt-group">{t('settings.perf.disk')}</h2>
        <OptionRow glyph="disk" title={t('settings.perf.measure')} desc={t('settings.perf.measure_desc')}
          on={prefs.appsMeasure} onchange={(v) => prefs.set('appsMeasure', v)} />
      {:else if tab === 'pages'}
        <h2 class="opt-group">{t('settings.pages.startup')}</h2>
        <OptionRow glyph="startup" title={t('settings.pages.all_sources')} desc={t('settings.pages.all_sources_desc')}
          on={prefs.startupAll} onchange={setStartupAll} />

        <h2 class="opt-group">{t('settings.pages.procs')}</h2>
        <OptionRow glyph="warning" title={t('settings.pages.ask_kill')} desc={t('settings.pages.ask_kill_desc')}
          on={askKill} onchange={setAskKill} />

        <h2 class="opt-group">{t('settings.pages.apps')}</h2>
        <OptionRow glyph="history" title={t('settings.pages.stale')} desc={t('settings.pages.stale_desc')}>
          <select class="inp" aria-label={t('settings.pages.stale_label')} value={prefs.staleDays} onchange={(e) => prefs.set('staleDays', Number(e.currentTarget.value))}>
            {#each STALE as d (d)}<option value={d}>{t('settings.pages.days', { count: d })}</option>{/each}
          </select>
        </OptionRow>
        <OptionRow glyph="delete" title={t('settings.pages.leftover')} desc={t('settings.pages.leftover_desc')} soon>
          <select class="inp" aria-label={t('settings.pages.leftover_label')} disabled><option>{t('settings.pages.recycle')}</option></select>
        </OptionRow>
      {:else if tab === 'ai'}
        <h2 class="opt-group">{t('settings.ai.connect')}</h2>
        {#if ai.saveStatus}
          <p class="small opt-status" class:ai-error={ai.saveStatus.error} class:ai-connected={!ai.saveStatus.error} role="status">
            <Glyph name={ai.saveStatus.error ? 'warning' : 'check'} size={12} /> {ai.saveStatus.text}
          </p>
        {/if}
        <AiSettings />

        <h2 class="opt-group">{t('settings.ai.usage')}</h2>
        <OptionRow glyph="chat" title={t('settings.ai.open')} desc={ai.cfg ? `${PROVIDERS[ai.cfg.provider]} · ${ai.cfg.providers[ai.cfg.provider]?.model || t('settings.ai.no_model')} · Ctrl+J` : t('settings.ai.open_desc')}>
          <button type="button" class="btn sm" onclick={() => ai.toggle(true)}><Glyph name="chat" size={12} /> {t('settings.open')}</button>
        </OptionRow>
        <OptionRow glyph="shield" title={t('settings.ai.confirm')} desc={t('settings.ai.confirm_desc')}
          on={true} disabled />
        <OptionRow glyph="history" title={t('settings.ai.keep')} desc={t('settings.ai.keep_desc')} soon on={false} />
      {:else if tab === 'mcp'}
        <h2 class="opt-group">{t('settings.mcp.server')}</h2>
        <OptionRow glyph="robot" title={t('settings.mcp.server_title')} desc={t('settings.mcp.server_desc')}>
          {#if mcp}
            {#if mcp.embedded}<span class="safety ok">{t('settings.mcp.embedded')}</span>
            {:else if mcp.exists}<span class="safety ok">{t('settings.mcp.exists')}</span>
            {:else}<span class="safety warn">{t('settings.mcp.missing')}</span>{/if}
          {/if}
        </OptionRow>
        {#if mcp}
          <OptionRow glyph="folder" title={t('settings.mcp.path')} desc={mcp.path}>
            {#snippet below()}
              <span class="faint small">{mcp.embedded ? t('settings.mcp.path_embedded', { dir: LOCAL_DIR }) : t('settings.mcp.path_dev', { path: LOCAL_DIR + '\\fomeg-windows-app-mcp.exe' })}</span>
            {/snippet}
            <button type="button" class="btn sm" onclick={() => copy(mcp.path, t('settings.what.path'))}><Glyph name="copy" size={12} /> {t('settings.copy')}</button>
          </OptionRow>
          {#if !mcp.embedded && !mcp.exists}
            <p class="note warn"><Glyph name="warning" size={13} /> {t('settings.mcp.no_exe')} <span class="mono">npm run build:mcp</span> {t('settings.mcp.no_exe_after')}</p>
          {/if}

          <h2 class="opt-group">{t('settings.mcp.connect')}</h2>
          <OptionRow glyph="apps" title={t('settings.mcp.client')} desc={t('settings.mcp.client_desc')}>
            <select class="inp" aria-label={t('settings.mcp.client_label')} value={client} onchange={(e) => pickClient(e.currentTarget.value)}>
              {#each MCP_CLIENTS as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
            </select>
          </OptionRow>
          {#if cl}
            <div class="opt-card mcp-guide">
              <ol class="mcp-steps">
                {#each cl.steps as s, i (i)}<li>{s}</li>{/each}
              </ol>
              {#each cl.blocks as b, bi (bi)}
                {@const text = b.text(mcp.path)}
                <div class="cmd-block">
                  <div class="cmd-head">
                    <span class="lbl">{b.label}</span>
                    <button type="button" class="btn xs" onclick={() => copy(text, b.label.toLocaleLowerCase(i18n.intl))}><Glyph name="copy" size={11} /> {t('settings.copy')}</button>
                  </div>
                  <pre class="mono">{text}</pre>
                </div>
              {/each}
              <div class="mcp-file small">
                <span class="muted">{t('settings.mcp.config_file')}</span> <span class="mono">{cl.file}</span>
              </div>
            </div>
          {/if}
        {:else}
          <p class="faint small">{t('settings.mcp.loading')}</p>
        {/if}
      {:else if tab === 'win'}
        <h2 class="opt-group">{t('settings.win.restart')}</h2>
        <OptionRow glyph="refresh" title={t('settings.win.restart_apps')} desc={t('settings.win.restart_apps_desc')}
          on={store.settings.restart_apps} pending={store.restartPending} disabled={!store.settingsLoaded} onchange={(v) => store.setRestartApps(v)} />

        <h2 class="opt-group">{t('settings.win.this_app')}</h2>
        <OptionRow glyph="admin" title={t('settings.admin.title')} desc={store.settings.is_admin ? t('settings.admin.yes_desc') : t('settings.admin.no_desc')}>
          {#if store.settingsLoaded}
            {#if store.settings.is_admin}
              <span class="safety ok">{t('settings.admin.yes')}</span>
            {:else}
              <button type="button" class="btn sm primary" disabled={relaunching} onclick={relaunch}><Glyph name="admin" size={12} /> {t('settings.admin.relaunch')}</button>
            {/if}
          {/if}
        </OptionRow>
        <OptionRow glyph="power" title={t('settings.win.autostart')} desc={t('settings.win.autostart_desc')} soon on={false} />
        <OptionRow glyph="close" title={t('settings.win.tray')} desc={t('settings.win.tray_desc')} soon on={false} />
      {:else if tab === 'keys'}
        {#each KEYS as [group, rows], gi (gi)}
          <h2 class="opt-group">{group}</h2>
          <div class="opt-card">
            {#each rows as [k, what], ki (ki)}
              <div class="key-row"><span class="key-what">{what}</span><span class="kbd-set mono">{k}</span></div>
            {/each}
          </div>
        {/each}
      {:else if tab === 'about'}
        <h2 class="opt-group">{t('settings.about.app')}</h2>
        <OptionRow glyph="info" title="Vietnamese's Ultimate W App" desc={t('settings.about.app_desc')}>
          <span class="mono small muted">{APP_VERSION}</span>
        </OptionRow>
        {#if updater.info?.enabled}
          <OptionRow glyph="refresh" title={t('settings.about.update')} desc={t('settings.about.update_desc')}
            on={prefs.updateAuto} onchange={(v) => prefs.set('updateAuto', v)} />
          <OptionRow glyph="download" title={t('update.set.check')} desc={updater.avail ? t('update.set.avail', { v: updater.avail.version }) : updater.lastCheck ? t('update.set.latest_at', { time: new Date(updater.lastCheck).toLocaleTimeString(i18n.intl, { hour: '2-digit', minute: '2-digit' }) }) : t('update.set.never')}>
            {#snippet below()}{#if updater.error}<span class="small ai-error">{updater.error}</span>{/if}{/snippet}
            {#if updater.avail}
              <button type="button" class="btn sm primary" disabled={updater.phase !== 'idle'} onclick={() => updater.install()}><Glyph name="download" size={12} /> {t('update.btn.install')}</button>
            {/if}
            <button type="button" class="btn sm" disabled={updater.checking || updater.phase !== 'idle'} onclick={() => updater.check({ manual: true })}>{#if updater.checking}<span class="spin xs"></span>{/if} {t('update.set.check_now')}</button>
          </OptionRow>
        {:else}
          <OptionRow glyph="refresh" title={t('settings.about.update')} desc={t('update.set.self_built')} on={false} disabled />
        {/if}
        <OptionRow glyph="document" title={t('settings.about.log')} desc={store.settings.log_path || LOCAL_DIR + '\\log.txt'}>
          <button type="button" class="btn sm" onclick={() => store.openLog()}><Glyph name="open" size={12} /> {t('settings.about.open_log')}</button>
        </OptionRow>
        <OptionRow glyph="folder" title={t('settings.about.data')} desc={dataDir || LOCAL_DIR}>
          {#snippet below()}<span class="faint small">{t('settings.about.data_desc')}</span>{/snippet}
          <button type="button" class="btn sm" disabled={!dataDir} onclick={() => api.duOpen(dataDir, true).catch((e) => toasts.error(t('settings.about.open_failed'), [errText(e)]))}><Glyph name="folder" size={12} /> {t('settings.open')}</button>
        </OptionRow>

        <h2 class="opt-group machine-head">
          {t('settings.m.this')}
          {#if info}<button type="button" class="btn xs" onclick={() => copy(machineText(info), t('settings.what.machine'))}><Glyph name="copy" size={11} /> {t('settings.copy_all')}</button>{/if}
        </h2>
        {#if info}
          <div class="machine-grid">
            <div class="opt-card">
              <h3 class="mc-head"><Glyph name="monitor" size={14} /> {t('settings.m.machine')}</h3>
              <dl class="props about">
                <dt>Model</dt><dd>{info.system || '–'}</dd>
                <dt>{t('settings.m.host')}</dt><dd>{info.hostname}</dd>
                <dt>{t('settings.m.board')}</dt><dd>{info.board || '–'}</dd>
                <dt>BIOS</dt><dd>{info.bios || '–'}</dd>
                <dt>{t('settings.m.screen')}</dt><dd>{screenText()}</dd>
              </dl>
            </div>
            <div class="opt-card">
              <h3 class="mc-head"><Glyph name="apps" size={14} /> Windows</h3>
              <dl class="props about">
                <dt>{t('settings.m.version')}</dt><dd>{info.os}</dd>
                <dt>{t('settings.m.installed')}</dt><dd>{day(info.os_installed_ms)}</dd>
                <dt>{t('settings.m.boot')}</dt><dd>{dayTime(info.boot_ms)}</dd>
                <dt>{t('settings.m.uptime')}</dt><dd>{since(info.boot_ms, sys.now)}</dd>
                <dt>{t('settings.m.account')}</dt><dd>{info.user || '–'}{store.settings.is_admin ? ' · Admin' : ''}</dd>
              </dl>
            </div>
            <div class="opt-card">
              <h3 class="mc-head"><Glyph name="processor" size={14} /> {t('settings.m.cpu')}</h3>
              <dl class="props about">
                <dt>CPU</dt><dd>{info.cpu_name}</dd>
                <dt>{t('settings.m.cores')}</dt><dd>{t('settings.m.cores_val', { cores: info.cores, threads: info.logical })}</dd>
                <dt>{t('settings.m.base')}</dt><dd>{info.base_mhz ? `${ghz(info.base_mhz)} GHz` : '–'}</dd>
              </dl>
            </div>
            <div class="opt-card">
              <h3 class="mc-head"><Glyph name="memory" size={14} /> {t('settings.m.memory')}</h3>
              <dl class="props about">
                <dt>{t('settings.m.total')}</dt><dd>{memText(info)}</dd>
                {#each info.mem_modules as m, i (i)}
                  <dt title={m.slot}>{m.slot}</dt><dd>{fmtBytes(m.size)} {m.kind} {m.form}{m.speed ? ` · ${m.speed} MT/s` : ''}{m.maker ? ` · ${m.maker}` : ''}</dd>
                {/each}
              </dl>
            </div>
            <div class="opt-card">
              <h3 class="mc-head"><Glyph name="gpu" size={14} /> {t('settings.m.graphics')}</h3>
              <dl class="props about">
                {#each info.gpus || [] as g (g.id)}
                  <dt>GPU {g.id}</dt><dd>{g.name}{g.ded_total ? ` · ${fmtBytes(g.ded_total)} VRAM` : ''}</dd>
                {:else}
                  <dt>GPU</dt><dd class="faint">{t('settings.m.unreadable')}</dd>
                {/each}
              </dl>
            </div>
            <div class="opt-card">
              <h3 class="mc-head"><Glyph name="disk" size={14} /> {t('settings.m.disks')}</h3>
              <dl class="props about">
                {#each info.disks as d (d.id)}
                  <dt>{t('settings.m.disk', { id: d.id })}</dt><dd>{d.model || d.name} · {fmtBytes(d.size)} · {d.ssd ? 'SSD' : 'HDD'}</dd>
                {/each}
              </dl>
            </div>
          </div>
        {:else}
          <p class="faint small">{t('settings.m.loading')}</p>
        {/if}
      {/if}
    </div>
  </div>
</section>
