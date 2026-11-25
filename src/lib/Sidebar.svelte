<script>
  // Icon trang = bộ icon nét của RustWTM (NavIcon.svelte); mục không có trong RustWTM (Trợ lý AI, Mở log) dùng Glyph.
  // Thanh bên kiểu RustWTM: nhóm "GIÁM SÁT" / "HỆ THỐNG", dưới cùng là Trợ lý AI, Cài đặt, Mở log.
  // Thu gọn được (chỉ icon), trạng thái được nhớ.
  import { store } from './store.svelte.js';
  import { prefs } from './prefs.svelte.js';
  import { ai } from './ai.svelte.js';
  import Glyph from './Glyph.svelte';
  import NavIcon from './NavIcon.svelte';
  import { t } from './i18n.svelte.js';

  const GROUPS = [
    ['monitor', ['dashboard', 'processes', 'performance']],
    ['system', ['apps', 'startup', 'services', 'ports', 'disk', 'cleanup']],
  ];
  const ICON = { dashboard: 'overview' };
  // [nhóm, tên nhóm, [[id, nhãn, icon]]] – tính lại khi đổi ngôn ngữ
  const groups = $derived(GROUPS.map(([g, ids]) => [g, t(`shell.nav.group.${g}`), ids.map((id) => [id, t(`shell.nav.${id}`), ICON[id] || id])]));
  const badge = $derived({
    startup: store.loaded ? store.startup.filter((i) => i.on).length : null,
    services: store.loaded ? store.services.filter((s) => s.state === 'running').length : null,
  });
  const c = $derived(prefs.collapsed);
</script>

<nav class="sidebar" class:collapsed={c} aria-label={t('shell.nav.label')}>
  <div class="brand">
    <span class="logo"><Glyph name="power" size={16} /></span>
    {#if !c}<span class="brand-text"><b title="Vietnamese's Ultimate W App">Vietnamese's Ultimate W App</b><small>{t('shell.brand.sub')}</small></span>{/if}
    <button type="button" class="icon-btn collapse-btn" onclick={() => prefs.set('collapsed', !c)} title={c ? t('shell.nav.expand') : t('shell.nav.collapse')} aria-label={c ? t('shell.nav.expand') : t('shell.nav.collapse')} aria-expanded={!c}>
      <Glyph name={c ? 'chevronRight' : 'chevronLeft'} size={12} />
    </button>
  </div>

  {#each groups as [g, title, items] (g)}
    {#if c}<div class="nav-sep" role="presentation"></div>{:else}<div class="nav-group">{title}</div>{/if}
    <ul class="nav">
      {#each items as [id, label, icon] (id)}
        <li>
          <button type="button" class="nav-item" class:active={store.page === id} aria-current={store.page === id ? 'page' : undefined} title={c ? label : undefined} aria-label={c ? label : undefined} onclick={() => (store.page = id)}>
            <NavIcon name={icon} size={18} />
            {#if !c}
              <span class="grow">{label}</span>
              {#if badge[id] != null}<span class="badge" title={t(`shell.nav.badge.${id}`, { n: badge[id] })}>{badge[id]}</span>{/if}
            {/if}
          </button>
        </li>
      {/each}
    </ul>
  {/each}

  <ul class="nav side-bottom">
    <li>
      <button type="button" class="nav-item" class:on={prefs.aiOpen} aria-pressed={prefs.aiOpen} title={t('shell.nav.ai_tip')} aria-label={c ? t('shell.nav.ai') : undefined} onclick={() => ai.toggle()}>
        <Glyph name="chat" size={16} />
        {#if !c}<span class="grow">{t('shell.nav.ai')}</span><kbd>Ctrl J</kbd>{/if}
      </button>
    </li>
    <li>
      <button type="button" class="nav-item" class:active={store.page === 'settings'} aria-current={store.page === 'settings' ? 'page' : undefined} title={c ? t('shell.nav.settings') : undefined} aria-label={c ? t('shell.nav.settings') : undefined} onclick={() => (store.page = 'settings')}>
        <NavIcon name="settings" size={18} />
        {#if !c}<span class="grow">{t('shell.nav.settings')}</span>{/if}
      </button>
    </li>
    <li>
      <button type="button" class="nav-item" class:active={store.page === 'changes'} aria-current={store.page === 'changes' ? 'page' : undefined} title={c ? t('shell.nav.whatsnew') : t('shell.nav.whatsnew_tip')} aria-label={c ? t('shell.nav.whatsnew') : undefined} onclick={() => (store.page = 'changes')}>
        <Glyph name="history" size={16} />
        {#if !c}<span class="grow">{t('shell.nav.whatsnew')}</span>{/if}
      </button>
    </li>
  </ul>
</nav>
