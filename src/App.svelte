<script>
  import { onMount } from 'svelte';
  import { store } from './lib/store.svelte.js';
  import { sys } from './lib/sys.svelte.js';
  import { prefs } from './lib/prefs.svelte.js';
  import { ai } from './lib/ai.svelte.js';
  import Sidebar from './lib/Sidebar.svelte';
  import Dashboard from './lib/Dashboard.svelte';
  import StartupPage from './lib/StartupPage.svelte';
  import ServicesPage from './lib/ServicesPage.svelte';
  import ProcessesPage from './lib/ProcessesPage.svelte';
  import PerformancePage from './lib/PerformancePage.svelte';
  import SettingsPage from './lib/SettingsPage.svelte';
  import ChangesPage from './lib/ChangesPage.svelte';
  import DiskPage from './lib/DiskPage.svelte';
  import CleanupPage from './lib/CleanupPage.svelte';
  import AppsPage from './lib/AppsPage.svelte';
  import { apps } from './lib/apps.svelte.js';
  import AiPanel from './lib/AiPanel.svelte';
  import StatusBar from './lib/StatusBar.svelte';
  import PortsPage from './lib/PortsPage.svelte';
  import CleanupProgress from './lib/CleanupProgress.svelte';
  import UpdateBar from './lib/UpdateBar.svelte';
  import { updater } from './lib/update.svelte.js';
  import ConfirmModal from './lib/ConfirmModal.svelte';
  import ActionReview from './lib/ActionReview.svelte';
  import Toasts from './lib/Toasts.svelte';
  import Glyph from './lib/Glyph.svelte';
  import { isTauri } from './lib/api.js';
  import { t } from './lib/i18n.svelte.js';

  onMount(() => {
    store.init();
    updater.init();
    sys.init();
    if (prefs.aiOpen) ai.init();
  });

  function key(e) {
    const k = e.key.toLowerCase();
    if ((e.ctrlKey || e.metaKey) && k === 'j') {
      e.preventDefault();
      ai.toggle();
      return;
    }
    if (e.key === 'F5' || ((e.ctrlKey || e.metaKey) && k === 'r')) {
      e.preventDefault();
      if (store.page === 'startup' || store.page === 'services' || store.page === 'dashboard') store.scan();
      else if (store.page === 'apps') apps.load({ force: true });
    }
  }

</script>

<svelte:window onkeydown={key} />

<div class="app" class:collapsed={prefs.collapsed}>
  <Sidebar />
  <main class="main">
    {#if store.settingsLoaded && !store.settings.is_admin}
      <div class="banner warn" role="alert">
        <Glyph name="warning" size={14} /> {t('shell.banner.no_admin')}
      </div>
    {/if}
    <UpdateBar />
    {#if !isTauri}
      <div class="banner info">{t('shell.banner.mock')}</div>
    {/if}
    <div class="content">
      {#if store.page === 'dashboard'}
        <Dashboard />
      {:else if store.page === 'processes'}
        <ProcessesPage />
      {:else if store.page === 'performance'}
        <PerformancePage />
      {:else if store.page === 'apps'}
        <AppsPage />
      {:else if store.page === 'startup'}
        <StartupPage />
      {:else if store.page === 'services'}
        <ServicesPage />
      {:else if store.page === 'ports'}
        <PortsPage />
      {:else if store.page === 'disk'}
        <DiskPage />
      {:else if store.page === 'cleanup'}
        <CleanupPage />
      {:else if store.page === 'changes'}
        <ChangesPage />
      {:else}
        <SettingsPage />
      {/if}
    </div>
    {#if store.page === 'cleanup'}<CleanupProgress />{/if}
  </main>
  {#if prefs.aiOpen}<AiPanel />{/if}
  <StatusBar />
</div>
<ConfirmModal />
<ActionReview />
<Toasts />
